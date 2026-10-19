//! Ammonit Meteo-40 statistics files, as Ammonit's Meteo-40 plus manual
//! describes them (chapter "Data Evaluation"): a CSV part (comma, UTF-8,
//! point decimal; the first column the start of each interval, each other
//! column one statistic named `Sensor;Evaluation;Statistic`, or
//! `Channel;Statistic` for a raw channel, and a last column `addr`), one
//! blank line, then an ini part with `[System]` (latitude, longitude,
//! timezone) and a section per sensor and evaluation (`sensor_height`,
//! `unit`, …).
//!
//! [`adapt`] rewrites one as the CSV the importer reads: the site as
//! preamble lines, each statistic renamed so its height and statistic are
//! recognised ("wind_speed 80m Avg (Anemometer)"), the data part as it is.
//! Each column's type comes from its evaluation (`humidity` is relative
//! humidity, whatever the sensor's label says) and its units from the
//! section's `unit`, so neither is guessed (checked on a real Meteo-40L
//! export, 2026-10-18). Statistics other than mean, maximum, minimum and standard deviation
//! (count, sum, median, last value) and the `addr` column are left out:
//! read as columns they would pass for means. Files come gzip-compressed
//! (`.gz`) from the logger: unpack them first.
use super::layout::reader;
use super::{Adapted, ColumnOverride};
use crate::model::ColumnType;
use std::collections::HashMap;

/// Separator of the parts of a statistic's name.
const NAME_PARTS: char = ';';
/// The statistics kept, as the logger names them.
const KEPT_STATISTICS: [&str; 4] = ["Avg", "Max", "Min", "StdDev"];
/// Every statistic the logger writes, to recognise its names.
const STATISTICS: [&str; 8] = ["Avg", "Max", "Min", "StdDev", "Count", "Sum", "Med", "Val"];
/// The memory-address column, left out.
const ADDRESS_FIELD: &str = "addr";
/// Sections and keys of the ini part.
const SYSTEM_SECTION: &str = "System";
const LATITUDE_KEY: &str = "latitude";
const LONGITUDE_KEY: &str = "longitude";
const TIMEZONE_KEY: &str = "timezone";
const HEIGHT_KEY: &str = "sensor_height";
const UNIT_KEY: &str = "unit";
/// The evaluations whose column type is known, as the logger names them.
const EVALUATION_TYPES: [(&str, ColumnType); 8] = [
    ("wind_speed", ColumnType::WindSpeed),
    ("wind_direction", ColumnType::WindDirection),
    ("temperature", ColumnType::Temperature),
    ("humidity", ColumnType::RelativeHumidity),
    ("air_pressure", ColumnType::Pressure),
    ("vertical_wind_speed", ColumnType::VerticalWindSpeed),
    ("solar_radiation", ColumnType::SolarRadiation),
    ("battery_voltage", ColumnType::BatteryVoltage),
];
/// Header of the timestamp column written.
const TIME_HEADER: &str = "Data/Hora";
/// Field separator of the file.
const SEPARATOR: char = ',';
/// Minutes in an hour, for the timezone.
const MINUTES_PER_HOUR: i32 = 60;

/// `text` rewritten as a plain CSV when it is a Meteo-40 statistics file;
/// `None` otherwise. O(text).
pub(super) fn adapt(text: &str) -> Option<Adapted> {
    let (data, ini) = split_parts(text);
    let mut records = reader(data, SEPARATOR).into_records().filter_map(Result::ok);
    let header = records.next()?;
    let names: Vec<&str> = header.iter().map(str::trim).collect();
    let is_statistic = |name: &str| {
        let parts: Vec<&str> = name.split(NAME_PARTS).collect();
        parts.len() >= 2 && STATISTICS.contains(parts.last().expect("split gives a part"))
    };
    if names.len() < 2 || !names[1..].iter().any(|name| name.split(NAME_PARTS).count() == 3 && is_statistic(name)) {
        return None;
    }
    let sections = sections(ini);
    let kept: Vec<usize> = (1..names.len())
        .filter(|&index| {
            let name = names[index];
            name != ADDRESS_FIELD && !(is_statistic(name) && !KEPT_STATISTICS.contains(&name.rsplit(NAME_PARTS).next().expect("split gives a part")))
        })
        .collect();

    let mut adapted = String::with_capacity(text.len());
    let system = sections.get(SYSTEM_SECTION);
    for (label, key) in [("Latitude", LATITUDE_KEY), ("Longitude", LONGITUDE_KEY)] {
        if let Some(value) = system.and_then(|keys| keys.get(key)).filter(|value| !value.is_empty()) {
            adapted.push_str(&format!("{label}: {value}\n"));
        }
    }
    adapted.push_str(TIME_HEADER);
    for &index in &kept {
        adapted.push(SEPARATOR);
        adapted.push_str(&recognisable(names[index], &sections));
    }
    adapted.push('\n');
    for record in records {
        adapted.push_str(record.get(0).unwrap_or_default());
        for &index in &kept {
            adapted.push(SEPARATOR);
            adapted.push_str(record.get(index).unwrap_or_default());
        }
        adapted.push('\n');
    }
    let utc_offset_min = system.and_then(|keys| keys.get(TIMEZONE_KEY)).and_then(|zone| utc_offset_min(zone));
    // Column 0 of the rewritten file is the time.
    let columns = kept.iter().enumerate().filter_map(|(position, &index)| declared(position + 1, names[index], &sections)).collect();
    Some(Adapted { text: adapted, utc_offset_min, columns })
}

/// What the ini part says of a statistic's column: the type of its
/// evaluation and the section's unit. `None` when it says neither.
fn declared(index: usize, name: &str, sections: &HashMap<String, HashMap<String, String>>) -> Option<ColumnOverride> {
    let parts: Vec<&str> = name.split(NAME_PARTS).collect();
    let section = &parts[..parts.len() - 1];
    let kind = match section {
        [_, evaluation] => EVALUATION_TYPES.iter().find(|(name, _)| name == evaluation).map(|&(_, kind)| kind),
        _ => None,
    };
    let units = sections.get(&section.join(&NAME_PARTS.to_string())).and_then(|keys| keys.get(UNIT_KEY)).filter(|unit| !unit.is_empty()).cloned();
    (kind.is_some() || units.is_some()).then(|| ColumnOverride { index, kind, units, ..Default::default() })
}

/// A line with nothing but the separators a spreadsheet pads lines with
/// when it saves the file again (",,,,").
fn padding_only(line: &str) -> bool {
    line.trim().chars().all(|char| char == SEPARATOR)
}

/// The CSV part and the ini part: before and after the first blank line.
fn split_parts(text: &str) -> (&str, &str) {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if padding_only(line) && offset > 0 {
            return (&text[..offset], &text[offset + line.len()..]);
        }
        offset += line.len();
    }
    (text, "")
}

/// The ini part's sections, each a map of its keys.
fn sections(ini: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    // A file saved again from a spreadsheet pads every line with separators.
    for line in ini.lines().map(|line| line.trim().trim_end_matches(SEPARATOR).trim()) {
        if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            current = name.to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            sections.entry(current.clone()).or_default().insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    sections
}

/// A statistic's name as the importer recognises it: "Anemometer;wind_speed;Avg"
/// with an 80 m height in the ini part -> "wind_speed 80m Avg (Anemometer)"; a
/// raw channel "A2;Avg" -> "A2 Avg".
fn recognisable(name: &str, sections: &HashMap<String, HashMap<String, String>>) -> String {
    let parts: Vec<&str> = name.split(NAME_PARTS).collect();
    let [sensor, evaluation, statistic] = parts[..] else { return parts.join(" ").replace(SEPARATOR, " ") };
    let keys = sections.get(name).or_else(|| sections.get(&format!("{sensor}{NAME_PARTS}{evaluation}")));
    let height = keys.and_then(|keys| keys.get(HEIGHT_KEY)).and_then(|height| leading_number(height)).map_or(String::new(), |height| format!(" {height}m"));
    format!("{evaluation}{height} {statistic} ({sensor})").replace(SEPARATOR, " ")
}

/// The number a value starts with ("80", "80.5 m").
fn leading_number(value: &str) -> Option<f64> {
    let end = value.find(|char: char| !(char.is_ascii_digit() || char == '.' || char == '-' || char == '+')).unwrap_or(value.len());
    value[..end].parse().ok()
}

/// Minutes from UTC of a timezone written "UTC+01:00", "UTC-03:00" or "UTC".
pub(super) fn utc_offset_min(zone: &str) -> Option<i32> {
    let offset = zone.trim().strip_prefix("UTC")?;
    if offset.is_empty() {
        return Some(0);
    }
    let (sign, rest) = match offset.chars().next()? {
        '+' => (1, &offset[1..]),
        '-' => (-1, &offset[1..]),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
    Some(sign * (hours.parse::<i32>().ok()? * MINUTES_PER_HOUR + minutes.parse::<i32>().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A statistics file in the manual's layout: two evaluators with four
    /// statistics, a count, a raw channel and the address; then the ini part.
    const FILE: &str = "Date/time,Anemometer;wind_speed;Avg,Anemometer;wind_speed;Max,Anemometer;wind_speed;Min,Anemometer;wind_speed;StdDev,\
                        Anemometer;wind_speed;Count,WindVane10bit;wind_direction;Avg,A2;Avg,addr\n\
                        2014-07-17 15:50:00,6.21,8.10,4.02,0.62,600,95.4,2.31,1024\n\
                        2014-07-17 16:00:00,6.50,8.40,4.31,0.65,600,93.0,2.30,1088\n\
                        \n\
                        [System]\n\
                        serial=D401234\n\
                        timezone=UTC-03:00\n\
                        latitude=-5.1234\n\
                        longitude=-36.4567\n\
                        \n\
                        [Anemometer;wind_speed]\n\
                        unit=m/s\n\
                        sensor_height=80\n\
                        \n\
                        [WindVane10bit;wind_direction]\n\
                        unit=°\n\
                        sensor_height=78 m\n";

    #[test]
    fn statistics_are_renamed_with_their_heights_and_the_site_kept() {
        let adapted = adapt(FILE).expect("a Meteo-40 file");
        let lines: Vec<&str> = adapted.text.lines().collect();
        assert_eq!(&lines[..2], ["Latitude: -5.1234", "Longitude: -36.4567"]);
        assert_eq!(
            lines[2],
            "Data/Hora,wind_speed 80m Avg (Anemometer),wind_speed 80m Max (Anemometer),wind_speed 80m Min (Anemometer),\
             wind_speed 80m StdDev (Anemometer),wind_direction 78m Avg (WindVane10bit),A2 Avg",
            "the count and the address are left out"
        );
        assert_eq!(lines[3], "2014-07-17 15:50:00,6.21,8.10,4.02,0.62,95.4,2.31");
        assert_eq!(lines.len(), 5, "the ini part is not data");
        assert_eq!(adapted.utc_offset_min, Some(-180));
        assert!(adapt("t,ws\n2023-01-01 00:00,5\n").is_none(), "an ordinary CSV is left alone");
        assert_eq!((utc_offset_min("UTC+05:30"), utc_offset_min("UTC"), utc_offset_min("CET")), (Some(330), Some(0), None));
    }

    /// The layout of a real Meteo-40L daily export (AKE Balcarce, 2018): a
    /// thermo-hygrometer labelled "Temp-Hum", a barometer whose deviation is
    /// a few tenths, a raw analog channel and an empty latitude.
    const REAL: &str = "Date/time,Temp-Hum;humidity;Avg,Temp-Hum;temperature;Avg,Barómetro;air_pressure;Avg,\
                        Barómetro;air_pressure;StdDev,A1;Avg,addr\n\
                        2018-01-01 00:00:00,50.06,14.41,1002.10,0.08,0.44,1\n\
                        2018-01-01 00:10:00,50.64,14.39,1002.12,0.07,0.44,2\n\
                        2018-01-01 00:20:00,52.00,14.37,1002.15,0.09,0.44,3\n\
                        \n\
                        [System]\n\
                        type=Meteo-40L\n\
                        timezone=UTC-03:00\n\
                        latitude=\n\
                        [Temp-Hum;humidity]\n\
                        unit=%\n\
                        sensor_height=97.5\n\
                        [Temp-Hum;temperature]\n\
                        unit=°C\n\
                        sensor_height=97.5\n\
                        [Barómetro;air_pressure]\n\
                        unit=hPa\n\
                        sensor_height=97\n\
                        [A1]\n\
                        unit=V\n";

    #[test]
    fn a_file_saved_again_from_a_spreadsheet_keeps_its_ini_part() {
        let pad = |line: &str| if line.starts_with("2018") || line.starts_with("Date") { format!("{line}\n") } else { format!("{line},,,,\n") };
        let padded: String = REAL.lines().map(pad).collect();
        let adapted = adapt(&padded).expect("still a Meteo-40 file");
        let header = adapted.text.lines().next().unwrap();
        assert!(header.contains("humidity 97.5m Avg (Temp-Hum)"), "the heights are still read: {header}");
        assert_eq!(adapted.utc_offset_min, Some(-180));
    }

    #[test]
    fn types_and_units_come_from_the_ini_part() {
        let (dataset, report) = super::super::import_text("D140096", REAL, &super::super::ImportConfig::default()).unwrap();
        let facts: Vec<(ColumnType, &str)> = dataset.columns.iter().map(|column| (column.kind, column.units.as_str())).collect();
        assert_eq!(
            facts,
            [
                (ColumnType::RelativeHumidity, "%"),
                (ColumnType::Temperature, "°C"),
                (ColumnType::Pressure, "hPa"),
                (ColumnType::Pressure, "hPa"),
                (ColumnType::Other, "V"),
            ],
            "\"Temp\" in the label does not make the humidity a temperature, nor a small deviation kilopascals"
        );
        assert_eq!(report.format.as_deref(), Some(super::super::AMMONIT_FORMAT));
        assert_eq!((dataset.latitude, dataset.utc_offset_min), (None, Some(-180)), "an empty latitude is no latitude");
    }
}

//! EPE export: the file the Brazilian Energy Research Agency (Empresa de
//! Pesquisa Energética) takes wind measurements in through its AMA system,
//! as its technical note specifies (NT EPE-DEE-RE-057/2016-R4, March 2023,
//! section 5, "Protocolo de envio e formato dos dados"):
//! - named `<station>_<first day>_<last day>.txt`, days as AAAAMMDD (5.9);
//! - a header with the station, the first and last interval, one line
//!   describing each of the 22 channels and the channel list (5.8, 5.15);
//! - the records between a line `dados` and a line `fimdados` (5.14), one
//!   per 10-minute interval, 22 fields separated by `|` and ended by `|`,
//!   with a decimal comma (5.13), in the order and formats of 5.10: day,
//!   time of the interval's start, error code, pressure (hPa), temperature
//!   (°C), relative humidity (%), the upper anemometer (mean, max, min, SD),
//!   the upper vane (mean, SD), the lower anemometer and vane, and the
//!   intermediate anemometer;
//! - an implausible value (5.4), a missing one or one excluded by a flag is
//!   written as `-` (5.3) and sets its sensor's bit in the error code (5.5),
//!   any of the sensor's fields counting; a sensor the dataset lacks is all
//!   `-` with its bit set.
//!
//! Pressure is converted to hPa and temperature to °C when the columns'
//! units are known. The note asks for times in UTC−3 without daylight saving
//! (5.11); the dataset's clock is written as it is.
use crate::derived::{kelvin_offset, pascals_per_unit, CELSIUS_ZERO_K};
use crate::error::{CoreError, CoreResult};
use crate::filter::{Cursor, FilterView};
use crate::model::{Dataset, Subtype};
use chrono::DateTime;
use std::fmt::Write;
use std::ops::{Range, RangeInclusive};

/// The interval the note fixes, s.
pub const EPE_STEP_S: i64 = 600;
/// Field separator, also ending every record (5.13).
const SEPARATOR: char = '|';
/// The decimal comma (5.13).
const DECIMAL_COMMA: char = ',';
/// Written for an invalid value (5.3).
const INVALID: &str = "-";
/// Lines around the records (5.14).
const RECORDS_START: &str = "dados";
const RECORDS_END: &str = "fimdados";
/// Ends every line.
const LINE_END: &str = "\r\n";
/// Pascals in a hectopascal.
const PA_PER_HPA: f64 = 100.0;
/// Extension of the file name (5.7).
const EXTENSION: &str = ".txt";
/// Plausible values (5.4): pressure, temperature, humidity, mean and
/// maximum speed.
const PRESSURE_HPA: RangeInclusive<f64> = 800.0..=1060.0;
const TEMPERATURE_C: RangeInclusive<f64> = -15.0..=50.0;
const HUMIDITY_PERCENT: RangeInclusive<f64> = 0.0..=110.0;
const MEAN_SPEED_M_S: RangeInclusive<f64> = 0.0..=50.0;
const MAX_SPEED_M_S: RangeInclusive<f64> = 0.0..=70.0;
/// The three channels before the measurements (5.10, items 1–3).
const LEADING_CHANNELS: [&str; 3] = [
    "Dia do início do intervalo (de 10 minutos) de medição [AAAAMMDD]",
    "Horário do início do intervalo (de 10 minutos) de medição [hhmmss]",
    "Código de erro do intervalo, com “0” indicando medição sem erro",
];

/// A sensor of the EPE layout, in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Barometer.
    Pressure,
    /// Thermometer.
    Temperature,
    /// Hygrometer.
    Humidity,
    /// Upper anemometer.
    UpperSpeed,
    /// Upper vane.
    UpperVane,
    /// Lower anemometer.
    LowerSpeed,
    /// Lower vane.
    LowerVane,
    /// Intermediate anemometer.
    MiddleSpeed,
}

impl Group {
    /// Every sensor, in the order of the file's channels.
    pub const ALL: [Group; 8] =
        [Group::Pressure, Group::Temperature, Group::Humidity, Group::UpperSpeed, Group::UpperVane, Group::LowerSpeed, Group::LowerVane, Group::MiddleSpeed];

    /// The sensor's bit in the error code (5.5).
    pub fn bit(self) -> u32 {
        match self {
            Group::Pressure => 1,
            Group::UpperSpeed => 2,
            Group::UpperVane => 4,
            Group::LowerSpeed => 8,
            Group::LowerVane => 16,
            Group::MiddleSpeed => 32,
            Group::Temperature => 64,
            Group::Humidity => 128,
        }
    }

    /// The statistics written for the sensor, in order (5.10).
    fn statistics(self) -> &'static [Subtype] {
        match self {
            Group::UpperSpeed | Group::LowerSpeed | Group::MiddleSpeed => &[Subtype::Mean, Subtype::Max, Subtype::Min, Subtype::StdDev],
            Group::UpperVane | Group::LowerVane => &[Subtype::Mean, Subtype::StdDev],
            Group::Pressure | Group::Temperature | Group::Humidity => &[Subtype::Mean],
        }
    }

    /// The sensor's name on its header lines, as the note's example writes them (5.15).
    fn name(self) -> &'static str {
        match self {
            Group::Pressure => "Pressão do ar [hPa]",
            Group::Temperature => "Temperatura do ar [°C] ",
            Group::Humidity => "Umidade relativa do ar [%rel]",
            Group::UpperSpeed => "Anemômetro superior",
            Group::UpperVane => "Wind Vane superior",
            Group::LowerSpeed => "Anemômetro 2",
            Group::LowerVane => "Wind Vane 2",
            Group::MiddleSpeed => "Anemômetro 3",
        }
    }

    fn is_speed(self) -> bool {
        matches!(self, Group::UpperSpeed | Group::LowerSpeed | Group::MiddleSpeed)
    }

    fn is_vane(self) -> bool {
        matches!(self, Group::UpperVane | Group::LowerVane)
    }
}

/// What to export.
#[derive(Debug, Clone)]
pub struct EpeOptions {
    /// The station's numeric code (5.8).
    pub station: String,
    /// The steps exported.
    pub steps: Range<usize>,
    /// The mean column of each sensor the dataset has, in any order.
    pub groups: Vec<(Group, usize)>,
}

/// The file name the note asks for: `<station>_<first day>_<last day>.txt` (5.9).
pub fn file_name(dataset: &Dataset, options: &EpeOptions) -> String {
    let steps = clamped(&options.steps, dataset.len);
    let day = |step: usize| stamp(dataset.time_at(step), "%Y%m%d");
    format!("{}_{}_{}{EXTENSION}", options.station, day(steps.start), day(last_step(&steps)))
}

/// The EPE file of `dataset`, flags read through `view`. O(steps × 22).
///
/// # Errors
/// [`CoreError::EpeStationInvalid`] for a station that is not a number;
/// [`CoreError::EpeNeedsTenMinutes`] for another time step;
/// [`CoreError::NoSuchColumn`] for a column past the dataset's.
pub fn to_epe(dataset: &Dataset, view: &FilterView, options: &EpeOptions) -> CoreResult<String> {
    if options.station.is_empty() || !options.station.chars().all(|char| char.is_ascii_digit()) {
        return Err(CoreError::EpeStationInvalid { station: options.station.clone() });
    }
    if dataset.step_s != EPE_STEP_S {
        return Err(CoreError::EpeNeedsTenMinutes { step_s: dataset.step_s });
    }
    // Every sensor, with the column of each of its statistics (none when the dataset lacks it).
    let mut sensors: Vec<(Group, Vec<Option<usize>>)> = Vec::with_capacity(Group::ALL.len());
    for group in Group::ALL {
        let mean = options.groups.iter().find(|(chosen, _)| *chosen == group).map(|&(_, column)| column);
        if let Some(mean) = mean {
            dataset.columns.get(mean).ok_or(CoreError::NoSuchColumn { column: mean })?;
        }
        let columns = group
            .statistics()
            .iter()
            .map(|&subtype| mean.and_then(|mean| if subtype == Subtype::Mean { Some(mean) } else { child(dataset, mean, subtype) }))
            .collect();
        sensors.push((group, columns));
    }

    let steps = clamped(&options.steps, dataset.len);
    let mut text = String::with_capacity((steps.len() + 32) * 128);
    header(&mut text, dataset, &options.station, &steps, &sensors);
    // One flag cursor per column written, in the same order as the fields.
    let mut cursors: Vec<Option<Cursor>> = sensors.iter().flat_map(|(_, columns)| columns.iter().map(|column| column.map(|column| view.cursor(column)))).collect();
    let mut fields: Vec<String> = Vec::with_capacity(cursors.len());
    for step in steps {
        let mut error = 0;
        fields.clear();
        let mut cursor = cursors.iter_mut();
        for (group, columns) in &sensors {
            let mut faulty = false;
            for (&subtype, &column) in group.statistics().iter().zip(columns) {
                let excluded = cursor.next().expect("one cursor per field").as_mut().map_or(true, |cursor| cursor.excludes(step));
                let field = column.filter(|_| !excluded).and_then(|column| formatted(dataset, *group, subtype, column, step));
                faulty |= field.is_none();
                fields.push(field.unwrap_or_else(|| INVALID.to_string()));
            }
            if faulty {
                error += group.bit();
            }
        }
        write!(text, "{}{SEPARATOR}{}{SEPARATOR}{error:03}", stamp(dataset.time_at(step), "%Y%m%d"), stamp(dataset.time_at(step), "%H%M%S"))
            .expect("writing to a String does not fail");
        for field in &fields {
            text.push(SEPARATOR);
            text.push_str(field);
        }
        text.push(SEPARATOR);
        text.push_str(LINE_END);
    }
    text.push_str(RECORDS_END);
    text.push_str(LINE_END);
    Ok(text)
}

/// The header of 5.15: station, first and last interval, a line per
/// channel, the channel list, and the line that opens the records.
fn header(text: &mut String, dataset: &Dataset, station: &str, steps: &Range<usize>, sensors: &[(Group, Vec<Option<usize>>)]) {
    let instant = |step: usize| stamp(dataset.time_at(step), "%Y%m%d%H%M%S");
    for line in [format!("Estação {station}"), format!("Início {}", instant(steps.start)), format!("Fim {}", instant(last_step(steps)))] {
        text.push_str(&line);
        text.push_str(LINE_END);
    }
    let mut descriptions: Vec<String> = LEADING_CHANNELS.iter().map(|description| description.to_string()).collect();
    for (group, columns) in sensors {
        let height = columns[0].and_then(|column| dataset.columns[column].height_m).map_or(String::new(), |height| format!(" ({height}m)"));
        for &subtype in group.statistics() {
            let statistic = match subtype {
                Subtype::Max => "máximo",
                Subtype::Min => "mínimo",
                Subtype::StdDev => "desvio padrão",
                _ => "média",
            };
            descriptions.push(if group.is_speed() {
                format!("{}{height}, velocidade do vento [m/s]: {statistic} do intervalo", group.name())
            } else if group.is_vane() {
                format!("{}{height}, direção de vento [°]: {statistic} do intervalo", group.name())
            } else {
                format!("{}: {statistic} do intervalo", group.name())
            });
        }
    }
    let channel = |number: usize| format!("CH{number:02}");
    for (index, description) in descriptions.iter().enumerate() {
        text.push_str(&format!("{} {description}{LINE_END}", channel(index + 1)));
    }
    for index in 0..descriptions.len() {
        text.push_str(&channel(index + 1));
        text.push(SEPARATOR);
    }
    text.push_str(LINE_END);
    text.push_str(RECORDS_START);
    text.push_str(LINE_END);
}

/// The child of mean column `mean` of statistic `subtype`, if the dataset has one.
fn child(dataset: &Dataset, mean: usize, subtype: Subtype) -> Option<usize> {
    dataset.columns.iter().position(|column| column.parent == Some(mean) && column.subtype == subtype && column.derived.is_none())
}

/// `column`'s value at `step` in the sensor's unit and format of 5.10, or
/// `None` when it is missing or implausible (5.4).
fn formatted(dataset: &Dataset, group: Group, subtype: Subtype, column: usize, step: usize) -> Option<String> {
    let data = &dataset.columns[column];
    let value = data.values[step];
    if value.is_nan() {
        return None;
    }
    let (value, plausible) = match (group, subtype) {
        (Group::Pressure, _) => {
            let hpa = pascals_per_unit(&data.units).map_or(value, |pascals| value * pascals / PA_PER_HPA);
            (hpa, PRESSURE_HPA.contains(&hpa))
        }
        (Group::Temperature, _) => {
            let celsius = kelvin_offset(&data.units).map_or(value, |offset| value + offset - CELSIUS_ZERO_K);
            (celsius, TEMPERATURE_C.contains(&celsius))
        }
        (Group::Humidity, _) => (value, HUMIDITY_PERCENT.contains(&value)),
        (_, Subtype::Mean) if group.is_speed() => (value, MEAN_SPEED_M_S.contains(&value)),
        (_, Subtype::Max) if group.is_speed() => (value, MAX_SPEED_M_S.contains(&value)),
        _ => (value, true),
    };
    if !plausible {
        return None;
    }
    Some(match group {
        // [xxxx,xx], [xxx,xx], [xx,xx]: whole digits zero-padded, two decimals.
        Group::Pressure => decimal(value, 7),
        Group::Humidity => decimal(value, 6),
        _ if group.is_speed() => decimal(value, 5),
        // [sxx,xx]: a sign only when negative.
        Group::Temperature if (value * 100.0).round() < 0.0 => format!("-{}", decimal(value.abs(), 5)),
        Group::Temperature => decimal(value, 5),
        // [xxx]: whole degrees, zero-padded.
        _ => format!("{:03}", value.round() as i64),
    })
}

/// `value` with two decimals and a decimal comma, zero-padded to `width` characters.
fn decimal(value: f64, width: usize) -> String {
    format!("{value:0width$.2}").replace('.', &DECIMAL_COMMA.to_string())
}

/// `steps` cut to a record of `len` steps.
fn clamped(steps: &Range<usize>, len: usize) -> Range<usize> {
    steps.start.min(len)..steps.end.min(len).max(steps.start.min(len))
}

/// The last step of `steps` (its start when empty).
fn last_step(steps: &Range<usize>) -> usize {
    steps.end.saturating_sub(1).max(steps.start)
}

/// Epoch second `seconds` in chrono `format`.
fn stamp(seconds: i64, format: &str) -> String {
    DateTime::from_timestamp(seconds, 0).unwrap_or_default().naive_utc().format(format).to_string()
}

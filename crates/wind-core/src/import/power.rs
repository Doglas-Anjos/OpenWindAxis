//! NASA POWER files (power.larc.nasa.gov: MERRA-2 meteorology at the
//! reanalysis grid point nearest a site, free and without credentials): a
//! header block between `-BEGIN HEADER-` and `-END HEADER-` (with the
//! location as "Latitude … Longitude …"), then a CSV whose time is split
//! over YEAR, MO, DY and HR columns (or YEAR and DOY), and parameters named
//! like WS50M. [`adapt`] rewrites such a file as the CSV the importer reads:
//! the header block kept as the preamble (the site comes from it), one
//! timestamp column, and each parameter named so its type and height are
//! recognised, the original name after it ("Vel 50m (WS50M)"). Values pass
//! unchanged; POWER marks missing data with -999, a default missing token.
use chrono::NaiveDate;
use super::{Adapted, HEAD_RECORDS};

/// The header line of the dates, ending with the time standard.
const DATES_LABEL: &str = "Dates";
/// How that line ends when the times are UTC.
const UTC_MARK: &str = "in UTC";

/// The line that closes the header block.
const HEADER_END: &str = "-END HEADER-";
/// POWER's field separator.
const SEPARATOR: char = ',';
/// Header of the timestamp column written in place of the time fields.
const TIME_HEADER: &str = "Data/Hora";
/// Written for a row whose time fields do not make a date: the importer
/// counts it as a bad timestamp.
const BAD_TIME: &str = "?";
/// Names of the time fields, as POWER writes them.
const YEAR: &str = "YEAR";
const MONTH: &str = "MO";
const DAY: &str = "DY";
const DAY_OF_YEAR: &str = "DOY";
const HOUR: &str = "HR";
const MINUTE: &str = "MN";
/// Parameter prefixes before a height and `M` ("WS50M"), and the word the
/// importer recognises for each.
const HEIGHT_PARAMETERS: [(&str, &str); 4] = [("WS", "Vel"), ("WD", "Dir"), ("T", "Temp"), ("RH", "Umid")];
/// Parameters without a height, and their recognised word.
const PLAIN_PARAMETERS: [(&str, &str); 1] = [("PS", "Pres")];

/// Where the time of a row is.
enum TimeFields {
    /// Calendar date, with optional hour and minute.
    Date { year: usize, month: usize, day: usize, hour: Option<usize>, minute: Option<usize> },
    /// Day of the year, with an optional hour.
    DayOfYear { year: usize, day: usize, hour: Option<usize> },
}

/// `text` rewritten as a plain CSV when it is a NASA POWER file; `None`
/// otherwise (the importer reads it as it is). O(text).
pub(super) fn adapt(text: &str) -> Option<Adapted> {
    let mut lines = text.lines();
    let preamble_lines = lines.by_ref().take(HEAD_RECORDS).position(|line| line.trim() == HEADER_END)? + 1;
    let header = lines.by_ref().find(|line| !line.trim().is_empty())?;
    let names: Vec<String> = header.split(SEPARATOR).map(|name| name.trim().to_ascii_uppercase()).collect();
    let field = |name: &str| names.iter().position(|candidate| candidate == name);
    let fields = match (field(YEAR)?, field(MONTH), field(DAY), field(DAY_OF_YEAR)) {
        (year, Some(month), Some(day), _) => TimeFields::Date { year, month, day, hour: field(HOUR), minute: field(MINUTE) },
        (year, _, _, Some(day)) => TimeFields::DayOfYear { year, day, hour: field(HOUR) },
        _ => return None,
    };
    let time_fields = [YEAR, MONTH, DAY, DAY_OF_YEAR, HOUR, MINUTE];
    let kept: Vec<usize> = (0..names.len()).filter(|&index| !time_fields.contains(&names[index].as_str())).collect();

    let mut adapted = String::with_capacity(text.len() + text.len() / 4);
    for line in text.lines().take(preamble_lines) {
        adapted.push_str(line);
        adapted.push('\n');
    }
    adapted.push_str(TIME_HEADER);
    for &index in &kept {
        adapted.push(SEPARATOR);
        adapted.push_str(&recognisable(&names[index]));
    }
    adapted.push('\n');
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let values: Vec<&str> = line.split(SEPARATOR).map(str::trim).collect();
        adapted.push_str(&stamp(&values, &fields).unwrap_or_else(|| BAD_TIME.to_string()));
        for &index in &kept {
            adapted.push(SEPARATOR);
            adapted.push_str(values.get(index).copied().unwrap_or_default());
        }
        adapted.push('\n');
    }
    // "Dates (month/day/year): … in UTC" (or "in LST", local solar time: no fixed offset).
    let in_utc = text.lines().take(preamble_lines).any(|line| line.contains(DATES_LABEL) && line.trim_end().ends_with(UTC_MARK));
    Some(Adapted { text: adapted, utc_offset_min: in_utc.then_some(0), ..Default::default() })
}

/// "YYYY-MM-DD HH:MM" of a row, or `None` when its fields make no date.
fn stamp(values: &[&str], fields: &TimeFields) -> Option<String> {
    let number = |index: usize| values.get(index)?.parse::<u32>().ok();
    let optional = |index: Option<usize>| index.map_or(Some(0), number);
    let (date, hour, minute) = match *fields {
        TimeFields::Date { year, month, day, hour, minute } => {
            (NaiveDate::from_ymd_opt(number(year)? as i32, number(month)?, number(day)?)?, optional(hour)?, optional(minute)?)
        }
        TimeFields::DayOfYear { year, day, hour } => (NaiveDate::from_yo_opt(number(year)? as i32, number(day)?)?, optional(hour)?, 0),
    };
    let time = date.and_hms_opt(hour, minute, 0)?;
    Some(time.format("%Y-%m-%d %H:%M").to_string())
}

/// A POWER parameter's name as the importer recognises it, the original
/// after it: WS50M -> "Vel 50m (WS50M)", PS -> "Pres (PS)"; unknown ones as
/// they are.
fn recognisable(name: &str) -> String {
    if let Some((_, word)) = PLAIN_PARAMETERS.iter().find(|(parameter, _)| *parameter == name) {
        return format!("{word} ({name})");
    }
    for (prefix, word) in HEIGHT_PARAMETERS {
        let height = name.strip_prefix(prefix).and_then(|rest| rest.strip_suffix('M'));
        if let Some(height) = height.filter(|height| !height.is_empty() && height.chars().all(|char| char.is_ascii_digit())) {
            return format!("{word} {height}m ({name})");
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_named_for_their_type_and_height() {
        assert_eq!(recognisable("WS50M"), "Vel 50m (WS50M)");
        assert_eq!(recognisable("WD10M"), "Dir 10m (WD10M)");
        assert_eq!(recognisable("T2M"), "Temp 2m (T2M)");
        assert_eq!(recognisable("PS"), "Pres (PS)");
        assert_eq!(recognisable("T2MDEW"), "T2MDEW", "not a height parameter");
        assert_eq!(recognisable("WSM"), "WSM", "no height");
    }

    #[test]
    fn rows_get_one_timestamp_and_bad_dates_are_marked() {
        let hourly = TimeFields::Date { year: 0, month: 1, day: 2, hour: Some(3), minute: None };
        assert_eq!(stamp(&["2023", "2", "28", "23"], &hourly).as_deref(), Some("2023-02-28 23:00"));
        assert_eq!(stamp(&["2023", "2", "29", "0"], &hourly), None, "no 29 February in 2023");
        let daily = TimeFields::DayOfYear { year: 0, day: 1, hour: None };
        assert_eq!(stamp(&["2024", "60"], &daily).as_deref(), Some("2024-02-29 00:00"), "day 60 of a leap year");
        assert!(adapt("t,ws\n2023-01-01 00:00,5\n").is_none(), "an ordinary CSV is left alone");
        let header = |standard: &str| format!("-BEGIN HEADER-\nDates (month/day/year): 01/01/2023 through 01/01/2023 in {standard}\n-END HEADER-\nYEAR,MO,DY,HR,WS50M\n2023,1,1,0,5\n");
        assert_eq!(adapt(&header("UTC")).unwrap().utc_offset_min, Some(0));
        assert_eq!(adapt(&header("LST")).unwrap().utc_offset_min, None, "local solar time has no fixed offset");
    }
}

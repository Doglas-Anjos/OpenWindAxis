//! Time-series CSV export and JSON metadata export.
use crate::error::{CoreError, CoreResult};
use crate::model::{DataColumn, Dataset, TimeStampMeaning};
use chrono::format::{Item, StrftimeItems};
use serde::Deserialize;
use std::fmt::Write;

pub mod epe;
pub mod openwind;
pub mod wasp_tab;

/// Header of the timestamp column.
const TIMESTAMP_HEADER: &str = "Timestamp";
/// Default timestamp layout: ISO date and time, to the second.
const DEFAULT_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// Bytes reserved per row for the timestamp, and per value.
const ROW_BYTES_TIMESTAMP: usize = 20;
const ROW_BYTES_PER_VALUE: usize = 8;

/// How to write the CSV.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CsvExportOptions {
    /// Field separator.
    pub delimiter: char,
    /// Write 5,25 instead of 5.25 (needs a delimiter other than ',').
    pub decimal_comma: bool,
    /// Written for missing values.
    pub missing: String,
    /// Which instant of each step the timestamp marks; `None` = the meaning
    /// the dataset was imported with, so stamps come out as in the source.
    #[serde(rename = "tsm")]
    pub timestamp_meaning: Option<TimeStampMeaning>,
    /// strftime layout of the timestamp.
    pub time_format: String,
    /// Also write the derived columns (TI, WPD, air density). Off by default:
    /// they are rebuilt on import, so a re-imported export would hold them
    /// twice.
    pub include_derived: bool,
}

impl Default for CsvExportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            decimal_comma: false,
            missing: String::new(),
            timestamp_meaning: None,
            time_format: DEFAULT_TIME_FORMAT.into(),
            include_derived: false,
        }
    }
}

/// The dataset as CSV: a timestamp column, then one column per data column
/// (derived ones only with `options.include_derived`), headed by its id.
/// Missing values are written as `options.missing`.
///
/// # Errors
/// [`CoreError::DecimalCommaWithCommaDelimiter`],
/// [`CoreError::InvalidTimeFormat`] (also for items a plain date-time cannot
/// render, such as `%z`) or [`CoreError::TimestampOutOfRange`].
pub fn to_csv(dataset: &Dataset, options: &CsvExportOptions) -> CoreResult<String> {
    if options.decimal_comma && options.delimiter == ',' {
        return Err(CoreError::DecimalCommaWithCommaDelimiter);
    }
    let invalid_format = || CoreError::InvalidTimeFormat { format: options.time_format.clone() };
    let format: Vec<Item> = StrftimeItems::new(&options.time_format).collect();
    if format.iter().any(|item| matches!(item, Item::Error)) {
        return Err(invalid_format());
    }
    // Step start -> file timestamp, the inverse of the import shift.
    let meaning = options.timestamp_meaning.unwrap_or(dataset.timestamp_meaning);
    let stamp_offset = -meaning.shift(dataset.step_s);
    let delimiter = options.delimiter;
    let columns: Vec<&DataColumn> =
        dataset.columns.iter().filter(|column| options.include_derived || column.derived.is_none()).collect();
    // A capacity hint only: saturating, so a huge record cannot wrap it on wasm32.
    let row_bytes = ROW_BYTES_PER_VALUE.saturating_mul(columns.len()).saturating_add(ROW_BYTES_TIMESTAMP);
    let mut csv = String::with_capacity(dataset.len.saturating_mul(row_bytes));
    csv.push_str(TIMESTAMP_HEADER);
    for column in &columns {
        csv.push(delimiter);
        csv.push_str(&quote(&column.id, delimiter));
    }
    csv.push('\n');
    for step in 0..dataset.len {
        let seconds = dataset.time_at(step) + stamp_offset;
        let stamp = chrono::DateTime::from_timestamp(seconds, 0).ok_or(CoreError::TimestampOutOfRange { seconds })?;
        // The String never fails, but chrono does for items a plain date-time
        // cannot render (an offset with %z): that error must reach the caller.
        write!(csv, "{}", stamp.naive_utc().format_with_items(format.iter())).map_err(|_| invalid_format())?;
        for column in &columns {
            csv.push(delimiter);
            let value = column.values[step];
            if value.is_nan() {
                csv.push_str(&options.missing);
            } else {
                write_number(&mut csv, value, options.decimal_comma);
            }
        }
        csv.push('\n');
    }
    Ok(csv)
}

/// Appends `value` in Rust's shortest round-trip form, with a comma instead
/// of the point when `decimal_comma`; no allocation.
fn write_number(csv: &mut String, value: f64, decimal_comma: bool) {
    let written_from = csv.len();
    // Formatting an f64 into a String cannot fail.
    let _ = write!(csv, "{value}");
    if decimal_comma {
        if let Some(point) = csv[written_from..].find('.') {
            let at = written_from + point;
            csv.replace_range(at..at + 1, ",");
        }
    }
}

/// Dataset and column metadata (no values), as pretty JSON.
pub fn to_json(dataset: &Dataset) -> String {
    serde_json::to_string_pretty(dataset).expect("a dataset always serializes: plain fields, string keys")
}

/// `text` as one CSV field: quoted when it holds the delimiter, a quote or a
/// newline, with inner quotes doubled.
fn quote(text: &str, delimiter: char) -> String {
    if text.contains(delimiter) || text.contains('"') || text.contains('\n') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

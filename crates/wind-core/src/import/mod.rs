//! Delimited-text import (CSV / TSV / semicolon) with auto-detection and
//! per-column overrides. Files are untrusted input: every guess can be
//! overridden through `ImportConfig`, and every anomaly lands in the report.
//!
//! Stages: [`detect_layout`] (delimiter, header, timestamp columns and
//! format) → [`build_columns`] (names, types, heights) → [`read_rows`] (one
//! streaming pass) → [`place_on_grid`] (regular time grid). A file of a
//! known format (NASA POWER, Campbell TOA5, Ammonit Meteo-40, NRG
//! SymphoniePRO) is first rewritten as a plain CSV ([`power`], [`toa5`],
//! [`ammonit`], [`sympro`]).
use crate::error::{CoreError, CoreResult};
use crate::displacement::Displacement;
use crate::model::{ColumnType, Dataset, Extrapolation, PrimaryChoice, Subtype, TimeStampMeaning};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::ops::RangeInclusive;

mod layout;
mod columns;
mod rows;
mod grid;
mod fast_time;
mod site;
mod power;
mod toa5;
mod ammonit;
mod sympro;
#[cfg(test)]
mod tests;

use layout::*;
use columns::*;
use rows::*;
use grid::*;
use fast_time::*;
use site::site_from_preamble;

/// Site elevations accepted, m: from below the lowest dry land (the Dead Sea
/// shore, about −430 m) to well inside the ISO 2533 troposphere (11 km),
/// where the standard-atmosphere pressure holds.
pub const ELEVATION_RANGE_M: RangeInclusive<f64> = -500.0..=9_000.0;

/// Clock offsets from UTC that exist, minutes: UTC−12 to UTC+14.
pub const UTC_OFFSET_RANGE_MIN: RangeInclusive<i32> = -720..=840;

/// Valid site coordinates, ± degrees.
const MAX_LATITUDE_DEG: f64 = 90.0;
const MAX_LONGITUDE_DEG: f64 = 180.0;

/// Tokens read as missing by default. Numeric ones also match "-999.0" etc.
pub const DEFAULT_MISSING_TOKENS: [&str; 10] = ["", "NaN", "nan", "NA", "N/A", "-", "null", "-999", "-9999", "-99999"];

/// How to read a file; every field left `None` is detected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportConfig {
    /// Field delimiter; `None` = auto.
    pub delimiter: Option<char>,
    /// Values like "5,2"; `None` = auto.
    pub decimal_comma: Option<bool>,
    /// Rows before the header row; `None` = auto (skips logger preambles).
    pub skip_rows: Option<usize>,
    /// Column(s) holding the timestamp; two = separate date and time.
    pub time_columns: Option<Vec<usize>>,
    /// chrono format string; `None` = auto from common formats.
    pub time_format: Option<String>,
    /// Which instant of each interval the file's timestamps mark.
    #[serde(rename = "tsm")]
    pub timestamp_meaning: TimeStampMeaning,
    /// Time step in seconds; `None` = most frequent interval.
    pub step_s: Option<i64>,
    /// Tokens read as missing. Numeric ones also match "-999.0" etc.
    pub missing: Vec<String>,
    /// Overrides by file column index.
    pub columns: Vec<ColumnOverride>,
    /// Site elevation above sea level, m, within [`ELEVATION_RANGE_M`]; the
    /// air density uses it ([`crate::derived`]).
    pub elevation_m: Option<f64>,
    /// Site latitude, degrees north; `None` = read from the logger preamble
    /// when it has one.
    pub latitude: Option<f64>,
    /// Site longitude, degrees east; `None` = from the preamble likewise.
    pub longitude: Option<f64>,
    /// Speeds to extrapolate to other heights, each a derived column.
    pub extrapolations: Vec<Extrapolation>,
    /// Calculated columns ([`crate::calculated`]), in order.
    pub calculated: Vec<crate::calculated::CalculatedColumn>,
    /// The mean speed below which TI is undefined, m/s; `None` = the original's rule.
    pub ti_cutoff_m_s: Option<f64>,
    /// How the primary column of each type is picked; types left out take
    /// the default ([`crate::primary`]).
    pub primary: Vec<(ColumnType, PrimaryChoice)>,
    /// The logger clock's offset from UTC, minutes, within
    /// [`UTC_OFFSET_RANGE_MIN`]; `None` = unknown (or as a recognised format states).
    pub utc_offset_min: Option<i32>,
    /// The displacement height; `None` = the heights as measured.
    pub displacement: Option<Displacement>,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            delimiter: None,
            decimal_comma: None,
            skip_rows: None,
            time_columns: None,
            time_format: None,
            timestamp_meaning: TimeStampMeaning::Start,
            step_s: None,
            missing: DEFAULT_MISSING_TOKENS.iter().map(|token| token.to_string()).collect(),
            columns: Vec::new(),
            elevation_m: None,
            latitude: None,
            longitude: None,
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            utc_offset_min: None,
            displacement: None,
        }
    }
}

/// The user's correction for one file column.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnOverride {
    /// File column index (0-based, counting the timestamp columns).
    pub index: usize,
    /// Leave the column out.
    pub skip: bool,
    /// Column type instead of the guess.
    #[serde(rename = "ctype")]
    pub kind: Option<ColumnType>,
    /// Subtype instead of the guess.
    pub subtype: Option<Subtype>,
    /// Height correction, m: absent = keep the guess, `null` = no height,
    /// a number = that height.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub height_m: Option<Option<f64>>,
    /// Units instead of the type's default.
    pub units: Option<String>,
    /// Column id instead of the header's name (made unique like a header's).
    pub name: Option<String>,
    /// File column index of the mean this SD / min / max / gust column
    /// belongs to, instead of the nearest mean of its type and height
    /// ("Colunas associadas" of Configurar dataset).
    pub parent: Option<usize>,
}

/// What the import detected and every anomaly it met.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// Field delimiter used.
    pub delimiter: char,
    /// Whether values were read with a decimal comma.
    pub decimal_comma: bool,
    /// Record index of the header row, if the file has one.
    pub header_row: Option<usize>,
    /// File columns holding the timestamp.
    pub time_columns: Vec<usize>,
    /// chrono format the timestamps were read with.
    pub time_format: String,
    /// Time step of the grid, seconds.
    pub step_s: i64,
    /// Non-blank data rows.
    pub rows_read: usize,
    /// Rows whose timestamp did not parse (dropped).
    pub rows_bad_time: usize,
    /// Rows that landed on an already filled step (dropped).
    pub duplicates: usize,
    /// Rows whose timestamp is not a whole number of steps from the start.
    pub off_grid: usize,
    /// Grid steps with no row at all.
    pub missing_steps: usize,
    /// Per dataset column: non-empty fields that are not numbers.
    pub unparsed: Vec<usize>,
    /// Per dataset column: the file column it came from (in the file as
    /// rewritten, for a recognised format).
    pub source_index: Vec<usize>,
    /// The format recognised and rewritten before reading ([`POWER_FORMAT`],
    /// [`TOA5_FORMAT`]); `None` for a plain delimited file.
    pub format: Option<String>,
}

/// A file of a known format, rewritten as the plain CSV the importer reads.
#[derive(Default)]
struct Adapted {
    /// The rewritten file.
    text: String,
    /// The clock's offset from UTC, minutes, when the file states it.
    utc_offset_min: Option<i32>,
    /// What the file declares about its columns (type, units, height), by
    /// column of the rewritten file: applied like the user's corrections,
    /// which win field by field.
    columns: Vec<ColumnOverride>,
}

/// `declared` (a format's own column facts) under `corrections` (the
/// user's): per column, each field the user set wins. O(columns²), a few
/// hundred at most.
fn with_declared(corrections: &[ColumnOverride], declared: &[ColumnOverride]) -> Vec<ColumnOverride> {
    let mut merged: Vec<ColumnOverride> = declared
        .iter()
        .map(|declared| match corrections.iter().find(|correction| correction.index == declared.index) {
            None => declared.clone(),
            Some(correction) => ColumnOverride {
                index: declared.index,
                skip: correction.skip || declared.skip,
                kind: correction.kind.or(declared.kind),
                subtype: correction.subtype.or(declared.subtype),
                height_m: correction.height_m.or(declared.height_m),
                units: correction.units.clone().or_else(|| declared.units.clone()),
                name: correction.name.clone().or_else(|| declared.name.clone()),
                parent: correction.parent.or(declared.parent),
            },
        })
        .collect();
    merged.extend(corrections.iter().filter(|correction| !declared.iter().any(|declared| declared.index == correction.index)).cloned());
    merged
}

/// [`ImportReport::format`] of an Ammonit Meteo-40 statistics file.
pub const AMMONIT_FORMAT: &str = "Ammonit Meteo-40";
/// [`ImportReport::format`] of an NRG SymphoniePRO text export.
pub const SYMPRO_FORMAT: &str = "NRG SymphoniePRO";
/// [`ImportReport::format`] of a NASA POWER file.
pub const POWER_FORMAT: &str = "NASA POWER";
/// [`ImportReport::format`] of a Campbell Scientific TOA5 file, whose time
/// stamps mark the end of each interval.
pub const TOA5_FORMAT: &str = "Campbell TOA5";

/// First byte of the range where Windows-1252 differs from Latin-1.
const WINDOWS_1252_HIGH_START: u8 = 0x80;
/// Last byte of that range.
const WINDOWS_1252_HIGH_END: u8 = 0x9F;
/// Windows-1252 characters for bytes 0x80..=0x9F; every other byte is Latin-1
/// (byte = code point). Undefined bytes map to the C1 control of the same
/// value, as the WHATWG encoding standard does.
const WINDOWS_1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// File bytes as text: UTF-8 when valid (borrowed, no copy), otherwise
/// Windows-1252, the code page of Brazilian and other Western loggers, so
/// accented headers ("Máx", "Direção") keep their letters.
pub fn decode_text(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(
            bytes
                .iter()
                .map(|&byte| match byte {
                    WINDOWS_1252_HIGH_START..=WINDOWS_1252_HIGH_END => {
                        WINDOWS_1252_HIGH[(byte - WINDOWS_1252_HIGH_START) as usize]
                    }
                    _ => byte as char,
                })
                .collect(),
        ),
    }
}

/// Deserializes a field that is present, even as `null`, into `Some`, so an
/// absent field stays `None`: tells "no correction" from "clear the value".
fn present<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Reads a delimited text file into a dataset on a regular time grid.
///
/// # Errors
/// Any import variant of [`CoreError`]: an empty file, no recognizable
/// timestamp, no data columns, too few rows, an implausible time span,
/// [`CoreError::ElevationOutOfRange`], [`CoreError::SiteLocationOutOfRange`]
/// or an invalid displacement height ([`Displacement::check`]).
pub fn import_text(name: &str, text: &str, config: &ImportConfig) -> CoreResult<(Dataset, ImportReport)> {
    if let Some(elevation_m) = config.elevation_m.filter(|elevation_m| !ELEVATION_RANGE_M.contains(elevation_m)) {
        let (min_m, max_m) = (*ELEVATION_RANGE_M.start(), *ELEVATION_RANGE_M.end());
        return Err(CoreError::ElevationOutOfRange { elevation_m, min_m, max_m });
    }
    let latitude_ok = config.latitude.map_or(true, |latitude| latitude.abs() <= MAX_LATITUDE_DEG);
    let longitude_ok = config.longitude.map_or(true, |longitude| longitude.abs() <= MAX_LONGITUDE_DEG);
    if !(latitude_ok && longitude_ok) {
        let (latitude, longitude) = (config.latitude.unwrap_or(f64::NAN), config.longitude.unwrap_or(f64::NAN));
        return Err(CoreError::SiteLocationOutOfRange { latitude, longitude });
    }
    if let Some(displacement) = &config.displacement {
        displacement.check()?;
    }
    if let Some(minutes) = config.utc_offset_min.filter(|minutes| !UTC_OFFSET_RANGE_MIN.contains(minutes)) {
        return Err(CoreError::UtcOffsetOutOfRange { minutes });
    }
    let text = text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(text);
    // A file of a known format is first rewritten as a plain CSV.
    let adapters: [(fn(&str) -> Option<Adapted>, &str); 4] =
        [(power::adapt, POWER_FORMAT), (toa5::adapt, TOA5_FORMAT), (ammonit::adapt, AMMONIT_FORMAT), (sympro::adapt, SYMPRO_FORMAT)];
    let adapted = adapters.into_iter().find_map(|(adapt, format)| adapt(text).map(|adapted| (adapted, format)));
    let format = adapted.as_ref().map(|(_, format)| format.to_string());
    let utc_offset_min = adapted.as_ref().and_then(|(adapted, _)| adapted.utc_offset_min);
    let text = adapted.as_ref().map_or(text, |(adapted, _)| adapted.text.as_str());
    let declared;
    let config = match adapted.as_ref().filter(|(adapted, _)| !adapted.columns.is_empty()) {
        Some((adapted, _)) => {
            declared = ImportConfig { columns: with_declared(&config.columns, &adapted.columns), ..config.clone() };
            &declared
        }
        None => config,
    };
    let layout = detect_layout(text, config)?;
    let (mut columns, source_fields) = build_columns(&layout, config)?;
    let rows = read_rows(text, &layout, &source_fields, config)?;
    let grid = place_on_grid(rows.times, rows.values, config.step_s)?;
    for (column, values) in columns.iter_mut().zip(grid.values) {
        column.values = values.into();
    }
    for (column, field) in columns.iter_mut().zip(&source_fields) {
        let units_corrected = config.columns.iter().any(|correction| correction.index == *field && correction.units.is_some());
        if let Some(units) = units_by_magnitude(column.kind, &column.values).filter(|_| !units_corrected) {
            column.units = units.to_string();
        }
    }
    // The preamble is the text above the header row (or above the data, without one).
    let header_row = layout.data_row.checked_sub(1);
    let preamble_end = header_row.and_then(|row| layout.head.get(row)).map_or(layout.data_offset, |(_, offset)| *offset);
    let (preamble_latitude, preamble_longitude) = site_from_preamble(&text[..preamble_end]);
    let report = ImportReport {
        delimiter: layout.delimiter,
        decimal_comma: layout.decimal_comma,
        header_row: layout.data_row.checked_sub(1),
        time_columns: layout.time_columns,
        time_format: layout.time_format,
        step_s: grid.step,
        rows_read: rows.read,
        rows_bad_time: rows.bad_time,
        duplicates: grid.duplicates,
        off_grid: grid.off_grid,
        missing_steps: grid.missing_steps,
        unparsed: rows.unparsed,
        source_index: source_fields,
        format,
    };
    let dataset = Dataset {
        name: name.to_string(),
        start: grid.start + config.timestamp_meaning.shift(grid.step),
        step_s: grid.step,
        len: grid.len,
        timestamp_meaning: config.timestamp_meaning,
        utc_offset_min: config.utc_offset_min.or(utc_offset_min),
        latitude: config.latitude.or(preamble_latitude),
        longitude: config.longitude.or(preamble_longitude),
        elevation_m: config.elevation_m,
        extrapolations: config.extrapolations.clone(),
        calculated: config.calculated.clone(),
        ti_cutoff_m_s: config.ti_cutoff_m_s,
        primary: config.primary.clone(),
        displacement: config.displacement.clone(),
        columns,
        flags: crate::flags::default_flags(),
        segments: Vec::new(),
        evidence: Default::default(),
        edits: Vec::new(),
    };
    Ok((dataset, report))
}

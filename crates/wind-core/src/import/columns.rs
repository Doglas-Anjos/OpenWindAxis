//! Stage 2: column names, types, subtypes, heights and units.
use crate::derived;
use crate::error::{CoreError, CoreResult};
use crate::model::{ColumnType, DataColumn, Subtype};
use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::sync::Arc;
use super::*;

// ---- Columns ----

/// Prefix of the name a column gets when the file has no header ("Col3").
pub(super) const UNNAMED_COLUMN_PREFIX: &str = "Col";
/// First suffix given to a repeated header name ("A", "A_2", ...).
pub(super) const FIRST_DUPLICATE_SUFFIX: usize = 2;
/// A bare number in a header ("WS80") is read as a height only in this range, m.
pub(super) const BARE_HEIGHT_RANGE_M: RangeInclusive<f64> = 5.0..=500.0;
/// The token that marks the number before it as a height ("60 m", "60m").
pub(super) const METRE_TOKEN: &str = "m";
/// Values sampled per temperature or pressure column to tell its units by
/// magnitude.
pub(super) const UNIT_SAMPLE_VALUES: usize = 1_000;
/// A temperature column whose median is above this is in kelvin: air
/// temperatures span about 184 K (−89 °C, the coldest on record) to 330 K
/// (57 °C, the hottest), so a median above 150 cannot be in °C.
pub(super) const KELVIN_MEDIAN_ABOVE: f64 = 150.0;
/// A pressure column whose median is below this is in kPa. Surface pressures
/// span about 50–110 kPa (5 km up to sea level), i.e. 500–1100 hPa or
/// 50 000–110 000 Pa, so the three scales never meet.
pub(super) const KILOPASCAL_MEDIAN_BELOW: f64 = 200.0;
/// A pressure column whose median is at or above this is in Pa (and below it,
/// down to [`KILOPASCAL_MEDIAN_BELOW`], in hPa).
pub(super) const PASCAL_MEDIAN_FROM: f64 = 20_000.0;

// ---- Stage 2: columns ----

/// The data columns (still empty) and, for each, the file column it reads.
pub(super) fn build_columns(layout: &Layout, config: &ImportConfig) -> CoreResult<(Vec<DataColumn>, Vec<usize>)> {
    let header = layout.header();
    let width = layout
        .data_records()
        .iter()
        .map(|(record, _)| record.len())
        .chain(header.map(|header| header.len()))
        .max()
        .unwrap_or(0);
    let name_of = |field: usize| {
        header
            .and_then(|header| header.get(field))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("{UNNAMED_COLUMN_PREFIX}{}", field + 1))
    };
    // Every header name, so a renamed duplicate never takes a later column's
    // real name: ids stay unique (A, A, A_2 -> A, A_3, A_2).
    let all_names: HashSet<String> = (0..width).map(name_of).collect();
    let mut used_ids: HashSet<String> = HashSet::new();
    let mut columns: Vec<DataColumn> = Vec::new();
    let mut source_fields: Vec<usize> = Vec::new();
    for field in (0..width).filter(|field| !layout.time_columns.contains(field)) {
        let correction = config.columns.iter().find(|correction| correction.index == field);
        if correction.is_some_and(|correction| correction.skip) {
            continue;
        }
        let name = correction.and_then(|correction| correction.name.clone()).filter(|name| !name.trim().is_empty()).unwrap_or_else(|| name_of(field));
        let id = if used_ids.contains(&name) {
            (FIRST_DUPLICATE_SUFFIX..)
                .map(|suffix| format!("{name}_{suffix}"))
                .find(|candidate| !all_names.contains(candidate) && !used_ids.contains(candidate))
                .expect("an unbounded range always has an unused name")
        } else {
            name.clone()
        };
        used_ids.insert(id.clone());
        let (mut kind, mut subtype, mut height_m) = infer_column(&name);
        if let Some(correction) = correction {
            kind = correction.kind.unwrap_or(kind);
            subtype = correction.subtype.unwrap_or(subtype);
            height_m = correction.height_m.unwrap_or(height_m);
        }
        let units = correction
            .and_then(|correction| correction.units.clone())
            .unwrap_or_else(|| kind.default_units().to_string());
        columns.push(DataColumn { id, kind, subtype, units, height_m, parent: None, values: Arc::default(), derived: None });
        source_fields.push(field);
    }
    if columns.is_empty() {
        return Err(CoreError::NoDataColumns);
    }
    assign_parents(&mut columns);
    // A parent the user named wins over the nearest one.
    for (child, field) in source_fields.iter().enumerate() {
        let Some(parent_field) = config.columns.iter().find(|correction| correction.index == *field).and_then(|correction| correction.parent) else { continue };
        let parent = source_fields.iter().position(|candidate| *candidate == parent_field).ok_or(CoreError::NoSuchColumn { column: parent_field })?;
        columns[child].parent = Some(parent);
    }
    Ok((columns, source_fields))
}

/// Header keywords for one category: a token matches when it starts with
/// one of `prefixes` or equals one of `words`.
pub(super) struct Keywords {
    pub(super) prefixes: &'static [&'static str],
    pub(super) words: &'static [&'static str],
}

impl Keywords {
    pub(super) fn matches(&self, token: &str) -> bool {
        self.prefixes.iter().any(|prefix| token.starts_with(prefix)) || self.words.contains(&token)
    }
}

/// Subtype keywords (English and Portuguese), tried in this order.
pub(super) const SUBTYPE_KEYWORDS: [(Subtype, Keywords); 4] = [
    (Subtype::Gust, Keywords { prefixes: &["gust", "rajad"], words: &[] }),
    (Subtype::StdDev, Keywords { prefixes: &["stdev", "desv"], words: &["sd", "std", "sigma", "dp"] }),
    (Subtype::Max, Keywords { prefixes: &["max", "máx"], words: &[] }),
    (Subtype::Min, Keywords { prefixes: &["minim", "mín"], words: &["min"] }),
];

/// Column-type keywords (English and Portuguese), tried in this order. The
/// derived types come before wind speed, so an exported "WS80 TI" or
/// "WS80 WPD" reads back as what it is, not as another speed.
pub(super) const TYPE_KEYWORDS: [(ColumnType, Keywords); 12] = [
    (ColumnType::WindDirection, Keywords { prefixes: &["dir"], words: &["wd", "wdir", "vane"] }),
    (ColumnType::VerticalWindSpeed, Keywords { prefixes: &["vert"], words: &["vws"] }),
    (ColumnType::TurbulenceIntensity, Keywords { prefixes: &["turb"], words: &["ti"] }),
    (ColumnType::PowerDensity, Keywords { prefixes: &[], words: &["wpd"] }),
    (ColumnType::AirDensity, Keywords { prefixes: &["dens"], words: &["rho"] }),
    (ColumnType::WindSpeed, Keywords { prefixes: &["spe", "spd", "vel", "anem", "vent"], words: &["ws", "wspd", "wind"] }),
    // "tc": Campbell's air temperature ("AirTC_Avg" splits into air, tc, avg).
    (ColumnType::Temperature, Keywords { prefixes: &["temp"], words: &["tmp", "tc"] }),
    (ColumnType::Pressure, Keywords { prefixes: &["pres", "baro"], words: &["hpa", "mbar", "kpa"] }),
    // "ur": umidade relativa, as Brazilian files write it.
    (ColumnType::RelativeHumidity, Keywords { prefixes: &["humid", "umid"], words: &["rh", "ur"] }),
    (ColumnType::SolarRadiation, Keywords { prefixes: &["solar", "irrad", "pyr"], words: &["ghi"] }),
    (ColumnType::BatteryVoltage, Keywords { prefixes: &["bat", "volt"], words: &[] }),
    (ColumnType::Quality, Keywords { prefixes: &["qual", "avail"], words: &["cnr"] }),
];

/// Types whose height may come from a bare number in the header ("WS80").
pub(super) const TYPES_WITH_BARE_HEIGHT: [ColumnType; 4] =
    [ColumnType::WindSpeed, ColumnType::WindDirection, ColumnType::VerticalWindSpeed, ColumnType::Temperature];

/// Guess type, subtype and height from a header such as "WS80_SD",
/// "Spd 60m Max", "Direção 98m" or "Temp 2 m". A guess only: overrides win.
pub(super) fn infer_column(name: &str) -> (ColumnType, Subtype, Option<f64>) {
    let tokens = tokens(name);
    let any_token = |keywords: &Keywords| tokens.iter().any(|token| keywords.matches(token));
    let subtype = SUBTYPE_KEYWORDS
        .iter()
        .find(|(_, keywords)| any_token(keywords))
        .map_or(Subtype::Mean, |(subtype, _)| *subtype);
    let mut kind = TYPE_KEYWORDS
        .iter()
        .find(|(_, keywords)| any_token(keywords))
        .map_or(ColumnType::Other, |(kind, _)| *kind);
    if kind == ColumnType::Other && subtype == Subtype::Gust {
        kind = ColumnType::WindSpeed; // a gust is always a wind speed
    }

    let number = |token: &str| token.parse::<f64>().ok().filter(|value| value.is_finite());
    let mut height_m = tokens
        .windows(2)
        .find_map(|pair| if pair[1] == METRE_TOKEN { number(&pair[0]) } else { None });
    if height_m.is_none() && TYPES_WITH_BARE_HEIGHT.contains(&kind) {
        let numbers: Vec<f64> = tokens.iter().filter_map(|token| number(token)).collect();
        if let [height] = numbers.as_slice() {
            if BARE_HEIGHT_RANGE_M.contains(height) {
                height_m = Some(*height);
            }
        }
    }
    (kind, subtype, height_m)
}

/// Header split into lowercase tokens at separators, letter/digit changes and
/// camelCase humps: "WS80_SD" -> ws, 80, sd; "SpdMax" -> spd, max.
pub(super) fn tokens(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let numeric = |char: char| char.is_ascii_digit() || char == '.';
    let mut tokens = Vec::new();
    let mut current = String::new();
    for (index, &char) in chars.iter().enumerate() {
        let decimal_point = char == '.'
            && index > 0
            && chars[index - 1].is_ascii_digit()
            && chars.get(index + 1).is_some_and(|next| next.is_ascii_digit());
        if !(char.is_alphanumeric() || decimal_point) {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            let previous = chars[index - 1];
            let camel_hump = previous.is_lowercase() && char.is_uppercase();
            if numeric(previous) != numeric(char) || camel_hump {
                tokens.push(std::mem::take(&mut current));
            }
        }
        current.extend(char.to_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Units of a temperature or pressure column, told by the magnitude of its
/// values: headers rarely state them, and the type's default units would put
/// a file in K or kPa on the wrong scale (a density off by a factor of ten).
/// `None` for other types or a column without data. Cost: a sample of
/// [`UNIT_SAMPLE_VALUES`] values.
pub(super) fn units_by_magnitude(kind: ColumnType, values: &[f64]) -> Option<&'static str> {
    if !matches!(kind, ColumnType::Temperature | ColumnType::Pressure) {
        return None;
    }
    let stride = (values.len() / UNIT_SAMPLE_VALUES).max(1);
    let mut sample: Vec<f64> = values.iter().step_by(stride).copied().filter(|value| value.is_finite()).collect();
    if sample.is_empty() {
        return None;
    }
    let middle = sample.len() / 2;
    let (_, &mut median, _) = sample.select_nth_unstable_by(middle, f64::total_cmp);
    Some(match kind {
        ColumnType::Temperature if median > KELVIN_MEDIAN_ABOVE => derived::KELVIN,
        ColumnType::Temperature => derived::CELSIUS,
        _ if median < KILOPASCAL_MEDIAN_BELOW => derived::KILOPASCAL,
        _ if median < PASCAL_MEDIAN_FROM => derived::HECTOPASCAL,
        _ => derived::PASCAL,
    })
}

/// Link SD / min / max / gust columns to the mean column of the same type and
/// height, preferring the nearest one to their left (loggers write channels
/// as mean, SD, max, min side by side).
pub(super) fn assign_parents(columns: &mut [DataColumn]) {
    for child in 0..columns.len() {
        if columns[child].subtype == Subtype::Mean {
            continue;
        }
        let same_sensor = |candidate: &DataColumn| {
            candidate.subtype == Subtype::Mean
                && candidate.kind == columns[child].kind
                && candidate.height_m == columns[child].height_m
        };
        let parent = (0..columns.len())
            .filter(|&candidate| same_sensor(&columns[candidate]))
            .min_by_key(|&candidate| (candidate > child, candidate.abs_diff(child)));
        columns[child].parent = parent;
    }
}

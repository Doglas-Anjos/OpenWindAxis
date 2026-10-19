//! Openwind's met mast text file ("MM", version 2.1), as the original's
//! Exportar dados › Openwind writes one height (the layout of the original's
//! export preview; ARQUITETURA 2026-10-18):
//! - `MM,2.1`;
//! - latitude and longitude (6 decimals) and the number of heights;
//! - the heights; then `2` per height;
//! - per height, the mean standard deviation of the speed over the steps of
//!   15 m/s and up (3 decimals), 1.400 without such steps or deviations;
//! - the site elevation, m; the source file's and the dataset's names;
//! - the header `Year,Month,Day,Hour,Minute,` then, per height, the speed
//!   `[m/s]`, the direction `[°]`, the temperature `[°C]`,
//!   `Density <h>m [kg/m³]` and the speed's `TI [%]`, and `L [m]`;
//! - one line per step where every value is valid ("Require valid data in
//!   all variables", the original's default): speed with 3 decimals,
//!   direction 1, temperature 2, density 3, TI 2, and L (the Obukhov
//!   length, not computed here: 0, as in the report's file).
//!
//! The density is the dataset's air density column when it has one (moist
//! air, [`crate::derived`]), else from the temperature and the standard
//! pressure at the site; without a thermometer, the standard atmosphere's
//! temperature at the speed's height.
//!
//! As the original's page, the speeds (and their deviations, so the TI
//! stays) can be scaled so their simple mean meets a target; the page shows
//! three means of the exported speeds ([`openwind_means`]): the simple one,
//! that of their 1 m/s distribution (Openwind's), and that of the
//! seasonally unbiased distribution (each calendar month weighed by its
//! mean days).
use crate::analysis::MEAN_DAYS_PER_MONTH;
use crate::derived::{self, air_density_of, kelvin_offset, standard_pressure_pa, CELSIUS_ZERO_K, DEFAULT_RELATIVE_HUMIDITY_PERCENT};
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::{Dataset, Subtype};
use chrono::{DateTime, Datelike, Timelike};
use std::fmt::Write;
use std::ops::Range;

/// First line: the format and its version.
const MAGIC: &str = "MM,2.1";
/// Written once per height on the fourth line, as the original does.
const PER_HEIGHT_CODE: &str = "2";
/// The speed from which the deviation of the fifth line is averaged, m/s.
const SIGMA_FROM_M_S: f64 = 15.0;
/// That deviation when no step reaches it, m/s.
const DEFAULT_SIGMA_M_S: f64 = 1.4;
/// Obukhov length, not computed: the original writes 0 without one.
const NO_OBUKHOV_LENGTH: &str = "0";
/// Percent in a share.
const PERCENT: f64 = 100.0;
/// Standard atmosphere: sea-level temperature, K, and lapse rate, K/m.
const SEA_LEVEL_TEMPERATURE_K: f64 = 288.15;
const LAPSE_RATE_K_M: f64 = 0.0065;
/// Decimals of each field, as the report's file shows them.
const SPEED_DECIMALS: usize = 3;
const DIRECTION_DECIMALS: usize = 1;
const TEMPERATURE_DECIMALS: usize = 2;
const DENSITY_DECIMALS: usize = 3;
const TI_DECIMALS: usize = 2;
const COORDINATE_DECIMALS: usize = 6;
const SIGMA_DECIMALS: usize = 3;
/// Width of the speed bins of the distribution means, m/s.
const DISTRIBUTION_BIN_M_S: f64 = 1.0;
const MONTHS_PER_YEAR: usize = 12;

/// What an Openwind file holds.
#[derive(Debug, Clone)]
pub struct OpenwindOptions {
    /// Mean speed column.
    pub speed: usize,
    /// Mean direction column.
    pub direction: usize,
    /// Temperature column; `None` = the standard atmosphere's.
    pub temperature: Option<usize>,
    /// The steps exported.
    pub steps: Range<usize>,
    /// Source file name, written beside the dataset's name.
    pub source_name: String,
    /// Factor on every speed and speed deviation (1: as measured).
    pub scale: f64,
}

/// The original's three means of the exported speeds, m/s, unscaled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenwindMeans {
    /// Steps exported.
    pub steps: usize,
    /// Their simple mean.
    pub simple: f64,
    /// The mean of their 1 m/s distribution, each bin at its centre.
    pub binned: f64,
    /// The mean of the distribution with each calendar month weighed by its mean days.
    pub unbiased: f64,
}

/// A number with `decimals`, `-0` written as `0`.
fn fixed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.trim_start_matches('-').chars().all(|char| char == '0' || char == '.') {
        text.trim_start_matches('-').to_string()
    } else {
        text
    }
}

/// A height as the shortest number ("30", "97.5").
fn height_text(height_m: f64) -> String {
    let text = format!("{height_m}");
    text.strip_suffix(".0").map(str::to_string).unwrap_or(text)
}

/// The speed's SD column, if it has one.
fn deviation_of(dataset: &Dataset, speed: usize) -> Option<usize> {
    dataset.columns.iter().position(|column| column.parent == Some(speed) && column.subtype == Subtype::StdDev)
}

/// The steps `options` covers where the filter keeps every column it reads.
fn kept_steps(dataset: &Dataset, view: &FilterView, options: &OpenwindOptions) -> CoreResult<Vec<usize>> {
    let columns = [Some(options.speed), Some(options.direction), options.temperature];
    if let Some(column) = columns.iter().flatten().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column: *column });
    }
    let window = options.steps.start.min(dataset.len)..options.steps.end.min(dataset.len);
    let mut needed = vec![options.speed, options.direction];
    needed.extend(options.temperature);
    needed.extend(deviation_of(dataset, options.speed));
    Ok(view.valid_runs(&needed, window).into_iter().flatten().collect())
}

/// The mean of a distribution given as counts per 1 m/s bin (bins at their centres).
fn binned_mean(counts: &[f64]) -> f64 {
    let total: f64 = counts.iter().sum();
    counts.iter().enumerate().map(|(bin, count)| (bin as f64 + 0.5) * DISTRIBUTION_BIN_M_S * count).sum::<f64>() / total
}

/// The three means of the speeds `options` exports (before any scaling),
/// over the steps with every value. O(steps).
///
/// # Errors
/// As [`to_openwind_mm`].
pub fn openwind_means(dataset: &Dataset, view: &FilterView, options: &OpenwindOptions) -> CoreResult<OpenwindMeans> {
    let unscaled = OpenwindOptions { scale: 1.0, ..options.clone() };
    let text = to_openwind_mm(dataset, view, &unscaled)?;
    // The steps written are those with every value: read back their times and speeds.
    let speeds: Vec<(usize, f64)> = text
        .lines()
        .skip(HEADER_LINES)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            Some((fields.get(1)?.parse::<usize>().ok()? - 1, fields.get(5)?.parse::<f64>().ok()?))
        })
        .collect();
    let bins = speeds.iter().map(|&(_, speed)| (speed.max(0.0) / DISTRIBUTION_BIN_M_S) as usize + 1).max().unwrap_or(1);
    let mut overall = vec![0.0; bins];
    let mut by_month = vec![vec![0.0; bins]; MONTHS_PER_YEAR];
    for &(month, speed) in &speeds {
        let bin = (speed.max(0.0) / DISTRIBUTION_BIN_M_S) as usize;
        overall[bin] += 1.0;
        by_month[month][bin] += 1.0;
    }
    // Each month's distribution as shares, weighed by its mean days.
    let mut unbiased = vec![0.0; bins];
    for (month, counts) in by_month.iter().enumerate() {
        let total: f64 = counts.iter().sum();
        if total > 0.0 {
            counts.iter().zip(unbiased.iter_mut()).for_each(|(count, share)| *share += MEAN_DAYS_PER_MONTH[month] * count / total);
        }
    }
    Ok(OpenwindMeans {
        steps: speeds.len(),
        simple: speeds.iter().map(|&(_, speed)| speed).sum::<f64>() / speeds.len() as f64,
        binned: binned_mean(&overall),
        unbiased: binned_mean(&unbiased),
    })
}

/// Lines before the first step: the seven of the file's head and the column header.
const HEADER_LINES: usize = 8;

/// The Openwind MM file of `options` under `view`. O(steps).
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for an index `dataset` lacks;
/// [`CoreError::NoDataAfterFilter`] when no step has every value.
pub fn to_openwind_mm(dataset: &Dataset, view: &FilterView, options: &OpenwindOptions) -> CoreResult<String> {
    let steps = kept_steps(dataset, view, options)?;
    let speed = &dataset.columns[options.speed];
    let height = speed.height_m.unwrap_or(0.0);
    let deviation = deviation_of(dataset, options.speed);
    let density = derived::air_density_column(dataset);

    let site = dataset.elevation_m.unwrap_or(0.0);
    let standard_k = SEA_LEVEL_TEMPERATURE_K - LAPSE_RATE_K_M * (site + height);
    let celsius_at = |step: usize| match options.temperature {
        Some(column) => {
            let offset = kelvin_offset(&dataset.columns[column].units).unwrap_or(CELSIUS_ZERO_K);
            dataset.columns[column].values[step] + offset - CELSIUS_ZERO_K
        }
        None => standard_k - CELSIUS_ZERO_K,
    };
    let density_at = |step: usize, celsius: f64| match density {
        Some(column) if dataset.columns[column].values[step].is_finite() => dataset.columns[column].values[step],
        _ => air_density_of(standard_pressure_pa(site + height), celsius + CELSIUS_ZERO_K, DEFAULT_RELATIVE_HUMIDITY_PERCENT),
    };
    let ti_at = |step: usize| deviation.map_or(0.0, |column| dataset.columns[column].values[step] / speed.values[step] * PERCENT);
    let scale = options.scale;

    let mut lines = Vec::new();
    let mut sigma = (0.0, 0usize);
    for &step in &steps {
        let (speed_value, direction_value) = (speed.values[step] * scale, dataset.columns[options.direction].values[step]);
        let celsius = celsius_at(step);
        let density_value = density_at(step, celsius);
        let ti = ti_at(step);
        if ![speed_value, direction_value, celsius, density_value, ti].iter().all(|value| value.is_finite()) {
            continue;
        }
        if let Some(column) = deviation.filter(|_| speed_value >= SIGMA_FROM_M_S) {
            sigma = (sigma.0 + dataset.columns[column].values[step] * scale, sigma.1 + 1);
        }
        let time = DateTime::from_timestamp(dataset.time_at(step), 0).unwrap_or_default().naive_utc();
        lines.push(format!(
            "{},{},{},{},{},{},{},{},{},{},{NO_OBUKHOV_LENGTH}",
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            fixed(speed_value, SPEED_DECIMALS),
            fixed(direction_value, DIRECTION_DECIMALS),
            fixed(celsius, TEMPERATURE_DECIMALS),
            fixed(density_value, DENSITY_DECIMALS),
            fixed(ti, TI_DECIMALS),
        ));
    }
    if lines.is_empty() {
        return Err(CoreError::NoDataAfterFilter);
    }
    let sigma_15 = if sigma.1 > 0 { sigma.0 / sigma.1 as f64 } else { DEFAULT_SIGMA_M_S };
    let temperature_name = options.temperature.map_or_else(|| "Temp".to_string(), |column| dataset.columns[column].id.clone());
    let mut text = String::with_capacity(lines.len() * lines[0].len() + 512);
    let (latitude, longitude) = (dataset.latitude.unwrap_or(0.0), dataset.longitude.unwrap_or(0.0));
    // Writing to a String cannot fail.
    let _ = writeln!(text, "{MAGIC}");
    let _ = writeln!(text, "{},{},1", fixed(latitude, COORDINATE_DECIMALS), fixed(longitude, COORDINATE_DECIMALS));
    let _ = writeln!(text, "{}", height_text(height));
    let _ = writeln!(text, "{PER_HEIGHT_CODE}");
    let _ = writeln!(text, "{}", fixed(sigma_15, SIGMA_DECIMALS));
    let _ = writeln!(text, "{}", height_text(site));
    let _ = writeln!(text, "{},{}", options.source_name, dataset.name);
    let _ = writeln!(
        text,
        "Year,Month,Day,Hour,Minute,{} [m/s],{} [°],{temperature_name} [°C],Density {}m [kg/m³],{} TI [%],L [m]",
        speed.id,
        dataset.columns[options.direction].id,
        height_text(height),
        speed.id,
    );
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_text, ImportConfig};

    #[test]
    fn the_file_has_the_original_layout() {
        let csv = "t,WS 30m Avg,WS 30m SD,WD 30m,Temp 30m\n\
                   2019-09-01 03:00,2.321,1.31,36.6,15.53\n\
                   2019-09-01 03:10,16,1.5,40,15.5\n\
                   2019-09-01 03:20,,1,40,15\n";
        let (mut dataset, _) = import_text("csv_TORRE03.windog", csv, &ImportConfig::default()).unwrap();
        derived::append(&mut dataset);
        let options = OpenwindOptions { speed: 0, direction: 2, temperature: Some(3), steps: 0..dataset.len, source_name: "csv_TORRE03.windog".into(), scale: 1.0 };
        let text = to_openwind_mm(&dataset, &FilterView::new(&dataset), &options).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(&lines[..7], ["MM,2.1", "0.000000,0.000000,1", "30", "2", "1.500", "0", "csv_TORRE03.windog,csv_TORRE03.windog"]);
        assert_eq!(lines[7], "Year,Month,Day,Hour,Minute,WS 30m Avg [m/s],WD 30m [°],Temp 30m [°C],Density 30m [kg/m³],WS 30m Avg TI [%],L [m]");
        assert!(lines[8].starts_with("2019,9,1,3,0,2.321,36.6,15.53,1."), "{}", lines[8]);
        assert!(lines[8].ends_with(",56.44,0"), "TI = 1.31/2.321 in percent: {}", lines[8]);
        assert_eq!(lines.len(), 10, "the step without a speed is left out");
        // The original's means: simple 9.1605; in 1 m/s bins, 2.5 and 16.5; one month only, so the unbiased is the binned.
        let means = openwind_means(&dataset, &FilterView::new(&dataset), &options).unwrap();
        assert_eq!(means.steps, 2);
        assert!((means.simple - (2.321 + 16.0) / 2.0).abs() < 1e-9 && (means.binned - 9.5).abs() < 1e-9 && (means.unbiased - 9.5).abs() < 1e-9);
        // Scaled to a mean of 18.321 (× 2): speeds double, TI stays.
        let doubled = to_openwind_mm(&dataset, &FilterView::new(&dataset), &OpenwindOptions { scale: 2.0, ..options }).unwrap();
        assert!(doubled.lines().nth(8).unwrap().starts_with("2019,9,1,3,0,4.642,36.6,15.53,1."));
        assert!(doubled.lines().nth(8).unwrap().ends_with(",56.44,0"));
    }
}

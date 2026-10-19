//! Analisar › Distorção da torre (manual dlg-analyze-tower-distortion.htm):
//! the ratio or difference of two colocated anemometers by direction sector,
//! and the two factors the original reports for the pair, as its code
//! computes them (ARQUITETURA 2026-10-18):
//! - **Sectors:** 72 of 5°, the first centred on north; a step counts where
//!   both speeds and the direction are valid (and B ≠ 0 for a ratio).
//! - **Percentiles** are nearest-rank, as the original's: the value at
//!   ⌊p·n/100⌋ of the sorted sector (the upper middle for the median).
//! - **Tower distortion factor:** Σ mᵢ·|hᵢ − 1| / Σ mᵢ, hᵢ the median ratio
//!   of sector i and mᵢ its steps (def-tower-distortion-factor.htm).
//! - **Scatter factor:** Σ mᵢ·sᵢ / Σ mᵢ, sᵢ the sample SD of the ratios of
//!   sector i (def-scatter-factor.htm).
//!
//! Both factors are of ratios, whichever measure the window shows.
//!
//! For one pair, [`distortion_detail`] gives the original's other displays:
//! the median by sector per calendar year and per calendar month, and every
//! step's value against the direction for the scatter plots.
use super::sector_of;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use chrono::{DateTime, Datelike};

/// Sectors of the analysis, as the original fixes them.
pub const DISTORTION_SECTORS: usize = 72;
/// Percent in a share.
const PERCENT: f64 = 100.0;
/// The quartiles drawn around the median.
const LOWER_QUARTILE: f64 = 25.0;
const MEDIAN: f64 = 50.0;
const UPPER_QUARTILE: f64 = 75.0;

/// What is compared between the two anemometers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistortionMeasure {
    /// A / B.
    #[default]
    Ratio,
    /// A − B, m/s.
    Difference,
}

/// The values of one direction sector.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorStats {
    /// Steps in it.
    pub count: usize,
    /// Mean; NaN when empty.
    pub mean: f64,
    /// Smallest value; NaN when empty.
    pub min: f64,
    /// Largest value; NaN when empty.
    pub max: f64,
    /// Sample standard deviation; 0 under two steps.
    pub deviation: f64,
    /// Nearest-rank 25th percentile; NaN when empty.
    pub lower_quartile: f64,
    /// Nearest-rank median; NaN when empty.
    pub median: f64,
    /// Nearest-rank 75th percentile; NaN when empty.
    pub upper_quartile: f64,
}

/// One pair's distortion by sector, with its factors.
#[derive(Debug, Clone, PartialEq)]
pub struct TowerDistortion {
    /// The measure by sector, sector `i` centred on `i · 5°`.
    pub sectors: Vec<SectorStats>,
    /// Steps with both speeds and the direction ("Total occurrences").
    pub occurrences: usize,
    /// Weighted departure of the sectors' median ratio from 1.
    pub distortion_factor: f64,
    /// Weighted standard deviation of the ratios within sectors.
    pub scatter_factor: f64,
}

/// The nearest-rank percentile `percent` of `sorted`: its value at
/// ⌊percent·n/100⌋, the last one at most. NaN when empty.
pub fn nearest_rank(sorted: &[f64], percent: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((percent * sorted.len() as f64 / PERCENT) as usize).min(sorted.len() - 1)]
}

/// The statistics of `values` (any order; sorted in place).
fn stats(values: &mut [f64]) -> SectorStats {
    values.sort_unstable_by(f64::total_cmp);
    let count = values.len();
    let mean = values.iter().sum::<f64>() / count as f64;
    let deviation = if count < 2 {
        0.0
    } else {
        (values.iter().map(|value| (value - mean) * (value - mean)).sum::<f64>() / (count - 1) as f64).sqrt()
    };
    SectorStats {
        count,
        mean,
        min: values.first().copied().unwrap_or(f64::NAN),
        max: values.last().copied().unwrap_or(f64::NAN),
        deviation,
        lower_quartile: nearest_rank(values, LOWER_QUARTILE),
        median: nearest_rank(values, MEDIAN),
        upper_quartile: nearest_rank(values, UPPER_QUARTILE),
    }
}

/// Speeds `a` and `b` against `direction` under `view`. O(steps + steps·log).
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for an index `dataset` lacks.
pub fn tower_distortion(dataset: &Dataset, view: &FilterView, a: usize, b: usize, direction: usize, measure: DistortionMeasure) -> CoreResult<TowerDistortion> {
    if let Some(&column) = [a, b, direction].iter().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column });
    }
    let (speeds_a, speeds_b, directions) = (&dataset.columns[a].values, &dataset.columns[b].values, &dataset.columns[direction].values);
    let mut ratios = vec![Vec::new(); DISTORTION_SECTORS];
    let mut differences = vec![Vec::new(); if measure == DistortionMeasure::Difference { DISTORTION_SECTORS } else { 0 }];
    let mut occurrences = 0;
    for step in view.valid_runs(&[a, b, direction], 0..dataset.len).into_iter().flatten() {
        let (speed_a, speed_b, degrees) = (speeds_a[step], speeds_b[step], directions[step]);
        if !(speed_a.is_finite() && speed_b.is_finite() && degrees.is_finite()) {
            continue;
        }
        occurrences += 1;
        let sector = sector_of(degrees, DISTORTION_SECTORS);
        if speed_b != 0.0 {
            ratios[sector].push(speed_a / speed_b);
        }
        if measure == DistortionMeasure::Difference {
            differences[sector].push(speed_a - speed_b);
        }
    }
    let ratio_stats: Vec<SectorStats> = ratios.iter_mut().map(|values| stats(values)).collect();
    let weighted = |value: &dyn Fn(&SectorStats) -> f64| {
        let total: usize = ratio_stats.iter().map(|sector| sector.count).sum();
        let sum: f64 = ratio_stats.iter().filter(|sector| sector.count > 0).map(|sector| sector.count as f64 * value(sector)).sum();
        if total > 0 {
            sum / total as f64
        } else {
            f64::NAN
        }
    };
    let distortion_factor = weighted(&|sector| (sector.median - 1.0).abs());
    let scatter_factor = weighted(&|sector| sector.deviation);
    let sectors = match measure {
        DistortionMeasure::Ratio => ratio_stats,
        DistortionMeasure::Difference => differences.iter_mut().map(|values| stats(values)).collect(),
    };
    Ok(TowerDistortion { sectors, occurrences, distortion_factor, scatter_factor })
}

/// Months in a year.
const MONTHS_PER_YEAR: usize = 12;

/// One pair's medians by sector per year and per month, and its steps.
#[derive(Debug, Clone, PartialEq)]
pub struct DistortionDetail {
    /// Per calendar year with data, ascending: the year and the median of
    /// each sector (NaN where empty).
    pub by_year: Vec<(i32, Vec<f64>)>,
    /// Per calendar month (0 = January), the median of each sector.
    pub by_month: Vec<Vec<f64>>,
    /// Every `stride`-th step: direction, value, year and month (0 = January).
    pub points: Vec<(f64, f64, i32, u8)>,
}

/// The medians of `values` by group (any order; sorted in place).
fn medians(values: &mut [Vec<f64>]) -> Vec<f64> {
    values
        .iter_mut()
        .map(|values| {
            values.sort_unstable_by(f64::total_cmp);
            nearest_rank(values, MEDIAN)
        })
        .collect()
}

/// The measure of pair `a`, `b` per year and month of `direction`'s
/// sectors, and the steps for the scatter: at most `max_points`, every
/// k-th one. Steps on the dataset's clock. O(steps · log).
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for an index `dataset` lacks.
pub fn distortion_detail(dataset: &Dataset, view: &FilterView, a: usize, b: usize, direction: usize, measure: DistortionMeasure, max_points: usize) -> CoreResult<DistortionDetail> {
    if let Some(&column) = [a, b, direction].iter().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column });
    }
    let (speeds_a, speeds_b, directions) = (&dataset.columns[a].values, &dataset.columns[b].values, &dataset.columns[direction].values);
    let mut values: Vec<(f64, f64, i32, u8)> = Vec::new();
    for step in view.valid_runs(&[a, b, direction], 0..dataset.len).into_iter().flatten() {
        let (speed_a, speed_b, degrees) = (speeds_a[step], speeds_b[step], directions[step]);
        if !(speed_a.is_finite() && speed_b.is_finite() && degrees.is_finite()) || (measure == DistortionMeasure::Ratio && speed_b == 0.0) {
            continue;
        }
        let value = match measure {
            DistortionMeasure::Ratio => speed_a / speed_b,
            DistortionMeasure::Difference => speed_a - speed_b,
        };
        let time = DateTime::from_timestamp(dataset.time_at(step), 0).unwrap_or_default();
        values.push((degrees, value, time.year(), time.month0() as u8));
    }
    let years: Vec<i32> = {
        let mut years: Vec<i32> = values.iter().map(|value| value.2).collect();
        years.sort_unstable();
        years.dedup();
        years
    };
    let mut per_year = vec![vec![Vec::new(); DISTORTION_SECTORS]; years.len()];
    let mut per_month = vec![vec![Vec::new(); DISTORTION_SECTORS]; MONTHS_PER_YEAR];
    for &(degrees, value, year, month) in &values {
        let sector = sector_of(degrees, DISTORTION_SECTORS);
        let year_index = years.binary_search(&year).expect("every year was collected");
        per_year[year_index][sector].push(value);
        per_month[month as usize][sector].push(value);
    }
    let stride = values.len().div_ceil(max_points.max(1)).max(1);
    Ok(DistortionDetail {
        by_year: years.iter().zip(per_year.iter_mut()).map(|(&year, sectors)| (year, medians(sectors))).collect(),
        by_month: per_month.iter_mut().map(|sectors| medians(sectors)).collect(),
        points: values.into_iter().step_by(stride).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_detail_splits_years_and_months() {
        use crate::import::{import_text, ImportConfig};
        // Two years of daily steps: A/B is 1.1 in 2020 and 1.2 in 2021, all from 90°.
        let mut csv = String::from("t,a,b,dir\n");
        let first = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        for day in 0..731 {
            let date = first + chrono::Days::new(day);
            let ratio = if date.year() == 2020 { 1.1 } else { 1.2 };
            csv.push_str(&format!("{date} 00:00,{},10,90\n", 10.0 * ratio));
        }
        let config = ImportConfig { timestamp_meaning: crate::model::TimeStampMeaning::Start, ..ImportConfig::default() };
        let (dataset, _) = import_text("par.csv", &csv, &config).unwrap();
        let view = FilterView::new(&dataset);
        let detail = distortion_detail(&dataset, &view, 0, 1, 2, DistortionMeasure::Ratio, 100).unwrap();
        let east = sector_of(90.0, DISTORTION_SECTORS);
        assert_eq!(detail.by_year.iter().map(|(year, medians)| (*year, medians[east])).collect::<Vec<_>>(), [(2020, 1.1), (2021, 1.2)]);
        assert_eq!(detail.by_month[0][east], 1.2, "January of both years: the upper middle");
        assert!(detail.by_month[0][0].is_nan());
        assert_eq!(detail.points.len(), 92, "731 steps, every eighth");
    }

    #[test]
    fn percentiles_are_nearest_rank_as_the_original_takes_them() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(nearest_rank(&sorted, MEDIAN), 3.0, "the upper middle, not 2.5");
        assert_eq!(nearest_rank(&sorted, LOWER_QUARTILE), 2.0);
        assert_eq!(nearest_rank(&sorted, PERCENT), 4.0, "clamped to the last");
        assert!(nearest_rank(&[], MEDIAN).is_nan());
        let mut values = vec![3.0, 1.0, 2.0];
        let summary = stats(&mut values);
        assert_eq!((summary.median, summary.deviation, summary.min, summary.max), (2.0, 1.0, 1.0, 3.0));
    }
}

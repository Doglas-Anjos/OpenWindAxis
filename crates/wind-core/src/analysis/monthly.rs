//! Mean of a column by calendar month, January to December, over every year
//! of the record: the "monthly mean" of the summary panel
//! (ESPEC-INTERFACE §1.1). Directions average as vectors.
use super::MEAN_DAYS_PER_MONTH;
use crate::acc::{each_step, MeanAcc};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::series::{DayStats, MeanPeriod};
use crate::time::{days_in_month, Month, MONTHS_PER_YEAR, SECONDS_PER_DAY};
use serde::Serialize;

/// Mean and valid steps per calendar month.
#[derive(Debug, Clone, Serialize)]
pub struct MonthlyMeans {
    /// January first; NaN for a calendar month without valid data.
    pub means: Vec<f64>,
    /// Valid steps per calendar month.
    pub samples: Vec<usize>,
}

/// Monthly means of `column`; `months` is the record's calendar
/// (`DayStats::months`). One pass over the valid steps.
pub fn monthly_means(dataset: &Dataset, view: &FilterView, months: &[Month], column: usize) -> MonthlyMeans {
    let values = &dataset.columns[column].values;
    let is_angle = dataset.columns[column].is_angle();
    let runs = view.valid_runs(&[column], 0..dataset.len);
    let month_steps: Vec<_> = months.iter().map(Month::steps).collect();
    let mut by_month = [MeanAcc::EMPTY; MONTHS_PER_YEAR];
    each_step(&month_steps, &runs, |month, step| {
        if !values[step].is_nan() {
            by_month[months[month].index_in_year()].add(values[step], is_angle);
        }
    });
    MonthlyMeans {
        means: by_month.iter().map(|month| month.mean(is_angle)).collect(),
        samples: by_month.iter().map(|month| month.count).collect(),
    }
}

/// The monthly means of every year and the figures built on them (Analisar ›
/// Tabelas, monthly means): the table year × month, the mean of each
/// calendar month over the years, the mean of monthly means (MoMM) and the
/// interannual variability.
#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyTable {
    /// Calendar years of the record, ascending.
    pub years: Vec<i32>,
    /// `[year][month]`: the month's mean, January first; NaN without data.
    pub means: Vec<[f64; MONTHS_PER_YEAR]>,
    /// `[year][month]`: share of the whole calendar month with data (0 for
    /// a month outside the record).
    pub recovery: Vec<[f64; MONTHS_PER_YEAR]>,
    /// Per calendar month: the mean of the years' means that have data.
    pub mean_of_years: [f64; MONTHS_PER_YEAR],
    /// MoMM: `mean_of_years` weighted by each month's mean length in days
    /// ([`MEAN_DAYS_PER_MONTH`]); NaN unless all twelve months have data.
    pub momm: f64,
    /// Per year: its twelve monthly means weighted by their days; NaN unless
    /// the year has all twelve.
    pub annual: Vec<f64>,
    /// Sample standard deviation over mean of the complete years' `annual`;
    /// NaN below two complete years.
    pub interannual_variability: f64,
}

/// The monthly table of `column`, from the per-day summaries `stats`
/// (built under the view the table is for). O(days). Directions get the
/// table of vector means only: their other figures are NaN, as averaging
/// angles arithmetically means nothing.
pub fn monthly_table(dataset: &Dataset, stats: &DayStats, column: usize) -> MonthlyTable {
    let is_angle = dataset.columns[column].is_angle();
    let months = stats.months();
    let month_means = stats.period_means(column, MeanPeriod::Month);
    let mut years: Vec<i32> = months.iter().map(|month| month.year).collect();
    years.dedup();
    let empty = [f64::NAN; MONTHS_PER_YEAR];
    let mut means = vec![empty; years.len()];
    let mut recovery = vec![[0.0; MONTHS_PER_YEAR]; years.len()];
    let steps_in_days = |days: u32| f64::from(days) * SECONDS_PER_DAY as f64 / dataset.step_s as f64;
    for (month, mean) in months.iter().zip(&month_means) {
        let row = years.binary_search(&month.year).expect("every month's year is listed");
        let index = month.index_in_year();
        means[row][index] = mean.mean;
        recovery[row][index] = mean.valid as f64 / steps_in_days(days_in_month(month.year, month.month));
    }
    let weighted = |values: &[f64; MONTHS_PER_YEAR], weights: [f64; MONTHS_PER_YEAR]| {
        if is_angle || values.iter().any(|value| value.is_nan()) {
            return f64::NAN;
        }
        values.iter().zip(weights).map(|(value, days)| value * days).sum::<f64>() / weights.iter().sum::<f64>()
    };
    let mut mean_of_years = empty;
    if !is_angle {
        for (month, cell) in mean_of_years.iter_mut().enumerate() {
            let with_data: Vec<f64> = means.iter().map(|row| row[month]).filter(|mean| !mean.is_nan()).collect();
            if !with_data.is_empty() {
                *cell = with_data.iter().sum::<f64>() / with_data.len() as f64;
            }
        }
    }
    let annual: Vec<f64> = years
        .iter()
        .zip(&means)
        .map(|(&year, row)| weighted(row, std::array::from_fn(|month| f64::from(days_in_month(year, month as u32 + 1)))))
        .collect();
    let complete: Vec<f64> = annual.iter().copied().filter(|mean| !mean.is_nan()).collect();
    let interannual_variability = if complete.len() >= 2 {
        let average = complete.iter().sum::<f64>() / complete.len() as f64;
        let variance = complete.iter().map(|mean| (mean - average).powi(2)).sum::<f64>() / (complete.len() - 1) as f64;
        variance.sqrt() / average
    } else {
        f64::NAN
    };
    MonthlyTable { momm: weighted(&mean_of_years, MEAN_DAYS_PER_MONTH), years, means, recovery, mean_of_years, annual, interannual_variability }
}

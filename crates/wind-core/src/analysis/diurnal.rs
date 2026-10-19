//! Diurnal profile: mean by hour of day, overall and per calendar month.
use crate::acc::{each_step, Acc, MeanAcc};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time::{hour_of, Month, HOURS_PER_DAY, MONTHS_PER_YEAR};
use serde::Serialize;

/// Mean by hour of day.
#[derive(Debug, Clone, Serialize)]
pub struct Diurnal {
    /// Mean per hour of day over the whole record.
    pub all: Vec<f64>,
    /// `[month 0..12][hour]`: mean per calendar month (January first).
    pub by_month: Vec<Vec<f64>>,
}

/// Diurnal profile of `column`, overall and per calendar month; directions
/// average as vectors. `months` is the record's calendar (`DayStats::months`).
/// One pass: each sample lands in its month × hour cell, and the overall
/// profile is the in-order merge of the twelve months.
pub fn diurnal(dataset: &Dataset, view: &FilterView, months: &[Month], column: usize) -> Diurnal {
    let values = &dataset.columns[column].values;
    let is_angle = dataset.columns[column].is_angle();
    let runs = view.valid_runs(&[column], 0..dataset.len);
    let month_steps: Vec<_> = months.iter().map(Month::steps).collect();
    let mut by_month = [[MeanAcc::EMPTY; HOURS_PER_DAY]; MONTHS_PER_YEAR];
    each_step(&month_steps, &runs, |month, step| {
        if !values[step].is_nan() {
            by_month[months[month].index_in_year()][hour_of(dataset, step)].add(values[step], is_angle);
        }
    });
    let all: Vec<f64> = (0..HOURS_PER_DAY)
        .map(|hour| {
            let mut whole_year = by_month[0][hour];
            by_month[1..].iter().for_each(|month| whole_year.merge(&month[hour]));
            whole_year.mean(is_angle)
        })
        .collect();
    let by_month = by_month.iter().map(|hours| hours.iter().map(|mean| mean.mean(is_angle)).collect()).collect();
    Diurnal { all, by_month }
}

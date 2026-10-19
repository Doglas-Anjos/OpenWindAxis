//! Per-column summaries, kept per day so edits stay cheap, and chart
//! decimation.
use crate::acc::{self, Acc, MeanAcc};
use crate::filter::{self, FilterView};
use crate::model::Dataset;
use crate::ops::Op;
use crate::time::{self, Month};
use serde::Serialize;
use std::ops::Range;

/// One column over the whole record, after default filtering.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnSummary {
    /// Steps with a value that survives filtering.
    pub valid: usize,
    /// `valid` over the steps in the record (0..1; 0 for an empty record).
    pub recovery: f64,
    /// Vector (circular) mean for direction angles, arithmetic otherwise.
    pub mean: f64,
    /// NaN when `valid` is 0.
    pub min: f64,
    /// NaN when `valid` is 0.
    pub max: f64,
}

/// Count, mean, min and max of a span of samples.
#[derive(Debug, Clone, Copy)]
struct SummaryAcc {
    mean: MeanAcc,
    min: f64,
    max: f64,
}

impl SummaryAcc {
    const EMPTY: SummaryAcc = SummaryAcc {
        mean: MeanAcc::EMPTY,
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
    };
}

impl Acc for SummaryAcc {
    fn merge(&mut self, later: &Self) {
        self.mean.merge(&later.mean);
        self.min = self.min.min(later.min);
        self.max = self.max.max(later.max);
    }
}

/// Per-day summary of every column, kept in step with the dataset: a flag
/// edit re-summarises only the days it touches (ARQUITETURA A2), and the
/// record-wide numbers are an in-order merge of the days.
///
/// Memory: one 72-byte summary per column per day (30 columns × 20 years
/// ≈ 16 MB).
#[derive(Debug, Clone)]
pub struct DayStats {
    /// Steps of each calendar day ([`time::days`]).
    days: Vec<Range<usize>>,
    months: Vec<Month>,
    /// Steps in the record.
    step_count: usize,
    /// Per column: whether it averages as an angle.
    is_angle: Vec<bool>,
    /// `[column][day]`.
    per_day: Vec<Vec<SummaryAcc>>,
}

impl DayStats {
    /// Summarises every column of `dataset` under `view`. O(columns × steps), the
    /// only full pass; afterwards use [`DayStats::refresh`].
    pub fn new(dataset: &Dataset, view: &FilterView) -> Self {
        let days = time::days(dataset);
        let months = time::months(dataset, &days);
        let is_angle = dataset.columns.iter().map(|column| column.is_angle()).collect();
        let per_day = vec![vec![SummaryAcc::EMPTY; days.len()]; dataset.columns.len()];
        let mut stats = DayStats { days, months, step_count: dataset.len, is_angle, per_day };
        let all_days = 0..stats.days.len();
        for column in 0..dataset.columns.len() {
            stats.resummarise(dataset, view, column, all_days.clone());
        }
        stats
    }

    /// Brings the summaries up to date after a flag edit on `steps` of
    /// `columns`; their SD / min / max / gust children and the derived columns
    /// computed from them follow ([`filter::affected`]). `view` must already
    /// reflect the edit. O(steps touched + columns × days).
    ///
    /// Any other change (new data, a flag's include setting) needs a fresh
    /// [`DayStats::new`].
    pub fn refresh(&mut self, dataset: &Dataset, view: &FilterView, columns: &[usize], steps: Range<usize>) {
        let first_day = self.days.partition_point(|day| day.end <= steps.start);
        let end_day = self.days.partition_point(|day| day.start < steps.end).max(first_day);
        let affected = filter::affected(dataset, columns);
        for column in (0..dataset.columns.len()).filter(|&column| affected[column]) {
            self.resummarise(dataset, view, column, first_day..end_day);
        }
    }

    /// Brings the summaries up to date after `op` was applied, undone or
    /// redone; `view` must already reflect it. A flag edit refreshes only the
    /// days and columns it reached ([`DayStats::refresh`]). A value edit
    /// builds them again: a derived column can change far from the edited
    /// steps (an extrapolation falls back on the record's month × hour table
    /// of exponents), and a time shift moves every day boundary. A value
    /// edit already copies whole columns, so the rebuild costs the same order.
    pub fn follow(&mut self, dataset: &Dataset, view: &FilterView, op: &Op) {
        if op.changes_values() || op.moves_time() {
            *self = DayStats::new(dataset, view);
        } else {
            let (columns, steps) = op.reach();
            self.refresh(dataset, view, columns, steps);
            // A calculated column reads its sources filtered, and a moving
            // average or an accumulation moves beyond the flagged steps.
            let calculated: Vec<usize> =
                (0..dataset.columns.len()).filter(|&index| matches!(dataset.columns[index].derived, Some(crate::derived::Derived::Calculated(_)))).collect();
            if !calculated.is_empty() {
                self.refresh(dataset, view, &calculated, 0..dataset.len);
            }
        }
    }

    /// Recomputes the summaries of `column` on days `days`.
    fn resummarise(&mut self, dataset: &Dataset, view: &FilterView, column: usize, days: Range<usize>) {
        let day_steps = &self.days[days.clone()];
        let (Some(first), Some(last)) = (day_steps.first(), day_steps.last()) else { return };
        let runs = view.valid_runs(&[column], first.start..last.end);
        let (values, is_angle) = (&dataset.columns[column].values, self.is_angle[column]);
        acc::fill(&mut self.per_day[column][days], day_steps, &runs, || SummaryAcc::EMPTY, |summary, step| {
            let value = values[step];
            if !value.is_nan() {
                summary.mean.add(value, is_angle);
                summary.min = summary.min.min(value);
                summary.max = summary.max.max(value);
            }
        });
    }

    /// Summary of every column over the whole record. O(columns × days).
    pub fn summaries(&self) -> Vec<ColumnSummary> {
        (0..self.per_day.len()).map(|column| self.summary(column)).collect()
    }

    /// Summary of `column` over the whole record. O(days).
    pub fn summary(&self, column: usize) -> ColumnSummary {
        let total = acc::fold(&self.per_day[column]).unwrap_or(SummaryAcc::EMPTY);
        let valid = total.mean.count;
        ColumnSummary {
            valid,
            recovery: if self.step_count == 0 { 0.0 } else { valid as f64 / self.step_count as f64 },
            mean: total.mean.mean(self.is_angle[column]),
            min: if valid == 0 { f64::NAN } else { total.min },
            max: if valid == 0 { f64::NAN } else { total.max },
        }
    }

    /// Calendar months of the record.
    pub fn months(&self) -> &[Month] {
        &self.months
    }

    /// Columns summarised.
    pub fn columns(&self) -> usize {
        self.per_day.len()
    }

    /// Steps in the record.
    pub fn len(&self) -> usize {
        self.step_count
    }

    /// Whether the record has no steps.
    pub fn is_empty(&self) -> bool {
        self.step_count == 0
    }

    /// Valid samples of `column` on days `days`. O(days).
    pub fn valid(&self, column: usize, days: Range<usize>) -> usize {
        self.per_day[column][days].iter().map(|summary| summary.mean.count).sum()
    }

    /// Mean of `column` over each day, calendar month or calendar year of
    /// the record, from the per-day summaries: O(days), no pass over the
    /// steps. Directions average as vectors; a period without data has a
    /// NaN mean.
    pub fn period_means(&self, column: usize, period: MeanPeriod) -> Vec<PeriodMean> {
        let mean_of = |days: Range<usize>, steps: Range<usize>| {
            let total = acc::fold(&self.per_day[column][days]).unwrap_or(SummaryAcc::EMPTY);
            PeriodMean { steps, mean: total.mean.mean(self.is_angle[column]), valid: total.mean.count }
        };
        match period {
            MeanPeriod::Day => (0..self.days.len()).map(|day| mean_of(day..day + 1, self.days[day].clone())).collect(),
            MeanPeriod::Month => self.months.iter().map(|month| mean_of(month.days(), month.steps())).collect(),
            MeanPeriod::Year => {
                let mut means = Vec::new();
                let mut first = 0;
                while first < self.months.len() {
                    let year = self.months[first].year;
                    let last = first + self.months[first..].iter().take_while(|month| month.year == year).count() - 1;
                    let (from, to) = (&self.months[first], &self.months[last]);
                    means.push(mean_of(from.day_start..to.day_end, from.step_start..to.step_end));
                    first = last + 1;
                }
                means
            }
        }
    }
}

/// The period of [`DayStats::period_means`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeanPeriod {
    /// Calendar day.
    Day,
    /// Calendar month.
    Month,
    /// Calendar year.
    Year,
}

/// The mean of a column over one period.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodMean {
    /// The period's steps in the record.
    pub steps: Range<usize>,
    /// Mean of the valid samples; NaN without any.
    pub mean: f64,
    /// Valid samples.
    pub valid: usize,
}

/// Points a bucket can emit: its minimum and its maximum.
const POINTS_PER_BUCKET: usize = 2;

/// Min/max decimation of `values[steps]` into at most `2 * buckets`
/// `(step, value)` points, keeping each bucket's extremes in time order.
/// A bucket without data emits one NaN point so charts break the line.
/// Ranges small enough are returned raw, gaps included.
pub fn decimate(values: &[f64], steps: Range<usize>, buckets: usize) -> Vec<(usize, f64)> {
    let steps = steps.start..steps.end.min(values.len());
    if steps.is_empty() {
        return Vec::new();
    }
    let step_count = steps.len();
    if buckets == 0 || step_count <= POINTS_PER_BUCKET * buckets {
        return steps.map(|step| (step, values[step])).collect();
    }
    // In u64: step_count × bucket overflows a 32-bit usize (WASM) past ~2 M steps.
    let bucket_start = |bucket: usize| steps.start + (step_count as u64 * bucket as u64 / buckets as u64) as usize;
    let mut points = Vec::with_capacity(POINTS_PER_BUCKET * buckets);
    for bucket in 0..buckets {
        let (first, end) = (bucket_start(bucket), bucket_start(bucket + 1));
        let (mut lowest, mut highest): (Option<usize>, Option<usize>) = (None, None);
        for step in first..end {
            let value = values[step];
            if value.is_nan() {
                continue;
            }
            if lowest.map_or(true, |best| value < values[best]) {
                lowest = Some(step);
            }
            if highest.map_or(true, |best| value > values[best]) {
                highest = Some(step);
            }
        }
        match (lowest, highest) {
            (Some(low), Some(high)) if low == high => points.push((low, values[low])),
            (Some(low), Some(high)) => {
                let (earlier, later) = if low < high { (low, high) } else { (high, low) };
                points.push((earlier, values[earlier]));
                points.push((later, values[later]));
            }
            _ => points.push((first, f64::NAN)),
        }
    }
    points
}

//! The record's calendar as step ranges. Days and months are worked out once
//! per record, so analyses group samples by index arithmetic and never call
//! `chrono` per sample (ARQUITETURA P4). Times are on the dataset's own clock.
use crate::model::Dataset;
use chrono::{DateTime, Datelike};
use serde::Serialize;
use std::ops::Range;

/// Seconds in a calendar day.
pub const SECONDS_PER_DAY: i64 = 86_400;
/// Seconds in an hour.
pub const SECONDS_PER_HOUR: i64 = 3_600;
/// Seconds in a minute.
pub const SECONDS_PER_MINUTE: i64 = 60;
/// Hours in a day: the bins of every diurnal analysis.
pub const HOURS_PER_DAY: usize = 24;
/// Calendar months in a year.
pub const MONTHS_PER_YEAR: usize = 12;

/// Epoch seconds `seconds` as text in UTC by the chrono `format`; empty
/// off the calendar.
pub fn format_utc(seconds: i64, format: &str) -> String {
    DateTime::from_timestamp(seconds, 0).map_or_else(String::new, |time| time.format(format).to_string())
}

/// Whether `year` is a Gregorian leap year.
pub fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days in calendar month `month` (1–12) of `year`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days in calendar year `year`.
pub fn days_in_year(year: i32) -> u32 {
    if is_leap_year(year) {
        366
    } else {
        365
    }
}

/// Steps of each calendar day the record touches, in order: range `day` holds
/// the steps that start on that day, counting from the day of the first step.
///
/// Days without any step (time step longer than a day) stay in the list as
/// empty ranges, so index `day` is always `day` days after the first day. The
/// ranges tile `0..len` with no gap. O(days); empty without a positive step.
pub fn days(dataset: &Dataset) -> Vec<Range<usize>> {
    let mut days = Vec::new();
    if dataset.step_s <= 0 {
        return days;
    }
    let first_midnight = dataset.start.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let mut day_start = 0;
    while day_start < dataset.len {
        let next_midnight = first_midnight + (days.len() as i64 + 1) * SECONDS_PER_DAY;
        let day_end = dataset.first_step_from(next_midnight);
        days.push(day_start..day_end);
        day_start = day_end;
    }
    days
}

/// A calendar month of the record: steps `step_start..step_end` start in it,
/// and they are exactly the steps of days `day_start..day_end` (indices into
/// [`days`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Month {
    /// Calendar year.
    pub year: i32,
    /// 1 = January.
    pub month: u32,
    /// First step of the month.
    #[serde(rename = "i0")]
    pub step_start: usize,
    /// One past its last step.
    #[serde(rename = "i1")]
    pub step_end: usize,
    /// First day of the month, as an index into [`days`].
    #[serde(skip)]
    pub day_start: usize,
    /// One past its last day.
    #[serde(skip)]
    pub day_end: usize,
}

impl Month {
    /// The month's steps.
    pub fn steps(&self) -> Range<usize> {
        self.step_start..self.step_end
    }

    /// The month's days, as indices into [`days`].
    pub fn days(&self) -> Range<usize> {
        self.day_start..self.day_end
    }

    /// 0 = January, for indexing per-month tables.
    pub fn index_in_year(&self) -> usize {
        self.month as usize - 1
    }
}

/// Calendar months of the record, grouped from its [`days`]. Partial first and
/// last months hold only the steps inside the record. O(days).
pub fn months(dataset: &Dataset, days: &[Range<usize>]) -> Vec<Month> {
    let first_day = dataset.start.div_euclid(SECONDS_PER_DAY);
    let mut months: Vec<Month> = Vec::new();
    for (day, steps) in days.iter().enumerate() {
        let Some(midnight) = DateTime::from_timestamp((first_day + day as i64) * SECONDS_PER_DAY, 0) else { break };
        let (year, month) = (midnight.year(), midnight.month());
        match months.last_mut() {
            Some(current) if (current.year, current.month) == (year, month) => {
                current.step_end = steps.end;
                current.day_end = day + 1;
            }
            _ => months.push(Month {
                year,
                month,
                step_start: steps.start,
                step_end: steps.end,
                day_start: day,
                day_end: day + 1,
            }),
        }
    }
    months
}

/// Hour of day (0..24) in which step `step` starts.
pub fn hour_of(dataset: &Dataset, step: usize) -> usize {
    (dataset.time_at(step).rem_euclid(SECONDS_PER_DAY) / SECONDS_PER_HOUR) as usize
}

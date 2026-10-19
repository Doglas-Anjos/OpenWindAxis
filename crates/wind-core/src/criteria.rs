//! "Filtrar por" (ESPEC-INTERFACE §3.2; manual articles-filtering-data.htm and
//! def-filter-criteria.htm): criteria that narrow what an analysis reads. A
//! step is read only when it meets **every** active criterion.
//!
//! Two kinds, kept apart so filtering never copies a column or allocates per
//! column what the whole record shares (ARQUITETURA §4.2):
//! - **Flags** ([`FlagSelection`]) change which flagged data each column
//!   keeps, as default flag filtering does: per column, from the segments.
//! - **Row criteria** (date range, month and hour, direction sector, value of
//!   a column) test a step regardless of the column read, so they become one
//!   list of excluded steps shared by every column ([`excluded_steps`]).
//!
//! "Require concurrency" needs no criterion here: analyses of several columns
//! (rose, shear) already read only steps where all of them are valid.
use crate::analysis::{check_sectors, sector_of};
use crate::error::{CoreError, CoreResult};
use crate::model::Dataset;
use crate::time::{self, HOURS_PER_DAY, MONTHS_PER_YEAR, SECONDS_PER_DAY, SECONDS_PER_HOUR};
use std::ops::Range;

/// First calendar month, January.
const FIRST_MONTH: u32 = 1;
/// Hours of the day as bounds: a start in `0..24`, an end in `1..=24`.
const LAST_HOUR_BOUND: u32 = HOURS_PER_DAY as u32;

/// Which flagged data analyses keep (§3.2 "Flag / Incluir").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSelection {
    /// Per flag of the dataset, in order: keep data under it. Exclusion wins:
    /// a step under any flag not kept is out, whatever other flags it has.
    pub keep: Vec<bool>,
    /// Keep steps that no flag covers ("Dados sem flag"). Off, only flagged
    /// data is read.
    pub keep_unflagged: bool,
}

/// Steps whose direction falls in one sector (§3.2 "Setor de direção").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectorCriterion {
    /// Direction column index.
    pub direction: usize,
    /// Sectors of the full circle, `1..=MAX_SECTORS`; sector 0 is centred
    /// on north, counting clockwise, as in the wind rose.
    pub sectors: usize,
    /// The sector kept, `0..sectors`.
    pub sector: usize,
}

/// Steps where one column lies in `[min, max)` (§3.2 "Coluna de dados").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueCriterion {
    /// Column index.
    pub column: usize,
    /// Lower bound, kept; `-∞` for none.
    pub min: f64,
    /// Upper bound, excluded; `+∞` for none.
    pub max: f64,
}

/// The active criteria of one analysis; `None` (or default) is inactive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Criteria {
    /// Flags kept; `None` = default flag filtering.
    pub flags: Option<FlagSelection>,
    /// Only these steps (§3.2 "Intervalo de datas"), from the caller's dates.
    pub steps: Option<Range<usize>>,
    /// Only months `first..=last` (1 = January), wrapping past December when
    /// `first > last` (Dec–Feb).
    pub months: Option<(u32, u32)>,
    /// Only hours of the day `[start, end)`, `start` in `0..24` and `end` in
    /// `1..=24`, wrapping past midnight when `start ≥ end` (20–5). The hour is
    /// that of the step's start, on the logger's clock.
    pub hours: Option<(u32, u32)>,
    /// Only steps in one direction sector; steps without a valid direction
    /// are out.
    pub sector: Option<SectorCriterion>,
    /// Only steps where a column lies in `[min, max)`; steps without a value
    /// are out.
    pub value: Option<ValueCriterion>,
}

impl Criteria {
    /// Whether any row criterion is active, i.e. [`excluded_steps`] has work.
    fn tests_rows(&self) -> bool {
        self.steps.is_some() || self.months.is_some() || self.hours.is_some() || self.sector.is_some() || self.value.is_some()
    }

    /// Fails for a criterion that does not fit `dataset` or makes no sense.
    ///
    /// # Errors
    /// [`CoreError::NoSuchColumn`], [`CoreError::SectorsOutOfRange`], or
    /// [`CoreError::FilterCriterionInvalid`] naming the criterion.
    pub fn check(&self, dataset: &Dataset) -> CoreResult<()> {
        let invalid = |criterion: &'static str, value: f64| Err(CoreError::FilterCriterionInvalid { criterion, value });
        let check_column = |column: usize| {
            if column < dataset.columns.len() {
                Ok(())
            } else {
                Err(CoreError::NoSuchColumn { column })
            }
        };
        if let Some(selection) = &self.flags {
            if selection.keep.len() != dataset.flags.len() {
                return invalid("flags", selection.keep.len() as f64);
            }
        }
        if let Some((first, last)) = self.months {
            let months = FIRST_MONTH..=MONTHS_PER_YEAR as u32;
            if let Some(month) = [first, last].into_iter().find(|month| !months.contains(month)) {
                return invalid("months", f64::from(month));
            }
        }
        if let Some((start, end)) = self.hours {
            if start >= LAST_HOUR_BOUND {
                return invalid("hours", f64::from(start));
            }
            if !(1..=LAST_HOUR_BOUND).contains(&end) {
                return invalid("hours", f64::from(end));
            }
        }
        if let Some(sector) = self.sector {
            check_column(sector.direction)?;
            check_sectors(sector.sectors)?;
            if sector.sector >= sector.sectors {
                return invalid("sector", sector.sector as f64);
            }
        }
        if let Some(value) = self.value {
            check_column(value.column)?;
            if !(value.min < value.max) {
                return invalid("value", value.min);
            }
        }
        Ok(())
    }
}

/// The steps that the row criteria leave out, for every column: sorted,
/// disjoint, non-touching ranges. Empty when no row criterion is active.
///
/// Cost: one pass over the record when month, hour, sector or value is
/// active (arithmetic per step, no calendar call: months come from
/// [`time::months`]); O(1) for a date range alone.
pub fn excluded_steps(dataset: &Dataset, criteria: &Criteria) -> Vec<Range<usize>> {
    if !criteria.tests_rows() {
        return Vec::new();
    }
    let within = criteria.steps.clone().map_or(0..dataset.len, |steps| steps.start.min(dataset.len)..steps.end.min(dataset.len));
    let mut excluded = Vec::new();
    if within.start > 0 {
        excluded.push(0..within.start);
    }
    let per_step = criteria.months.is_some() || criteria.hours.is_some() || criteria.sector.is_some() || criteria.value.is_some();
    if per_step && !within.is_empty() {
        let months = criteria.months.map(|_| time::months(dataset, &time::days(dataset)));
        let mut month_index = 0;
        let mut open: Option<usize> = None;
        for step in within.clone() {
            let month = months.as_ref().map(|months| {
                while month_index + 1 < months.len() && months[month_index].step_end <= step {
                    month_index += 1;
                }
                months[month_index].month
            });
            let kept = month.map_or(true, |month| month_kept(month, criteria.months))
                && criteria.hours.map_or(true, |hours| hour_kept(dataset, step, hours))
                && criteria.sector.map_or(true, |sector| sector_kept(dataset, step, sector))
                && criteria.value.map_or(true, |value| value_kept(dataset, step, value));
            match (kept, open) {
                (false, None) => open = Some(step),
                (true, Some(start)) => {
                    excluded.push(start..step);
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(start) = open {
            excluded.push(start..within.end);
        }
    }
    if within.end < dataset.len {
        excluded.push(within.end..dataset.len);
    }
    // Ranges were pushed in order; only the date-range edges can touch them.
    excluded.dedup_by(|later, kept| {
        let touches = later.start <= kept.end;
        if touches {
            kept.end = kept.end.max(later.end);
        }
        touches
    });
    excluded
}

/// Whether `month` (1–12) lies in `first..=last`, wrapping past December.
fn month_kept(month: u32, months: Option<(u32, u32)>) -> bool {
    match months {
        None => true,
        Some((first, last)) if first <= last => (first..=last).contains(&month),
        Some((first, last)) => month >= first || month <= last,
    }
}

/// Whether step `step` starts inside the hours `[start, end)`, wrapping past
/// midnight when `start ≥ end`.
#[inline]
fn hour_kept(dataset: &Dataset, step: usize, (start, end): (u32, u32)) -> bool {
    let second = dataset.time_at(step).rem_euclid(SECONDS_PER_DAY);
    let (from, to) = (i64::from(start) * SECONDS_PER_HOUR, i64::from(end) * SECONDS_PER_HOUR);
    if start < end {
        from <= second && second < to
    } else {
        second >= from || second < to
    }
}

/// Whether the direction at `step` is valid and in the sector.
#[inline]
fn sector_kept(dataset: &Dataset, step: usize, criterion: SectorCriterion) -> bool {
    let direction = dataset.columns[criterion.direction].values[step];
    !direction.is_nan() && sector_of(direction, criterion.sectors) == criterion.sector
}

/// Whether the column's value at `step` is in `[min, max)` (NaN never is).
#[inline]
fn value_kept(dataset: &Dataset, step: usize, criterion: ValueCriterion) -> bool {
    let value = dataset.columns[criterion.column].values[step];
    criterion.min <= value && value < criterion.max
}

//! Flag rules: automatic QC that flags the stretches of a column where a test
//! holds, and keeps with each stretch its evidence (the test, its thresholds
//! and the statistic that fired), so every automatic flag can be audited and
//! undone (ARQUITETURA B4). Sources: manual dlg-flag-flag-rule-properties.htm
//! and articles-flag-rule-examples.htm; ESPEC-INTERFACE §5.4–5.5.
//!
//! Rules for every test:
//! - **What the search sees:** steps that default filtering excludes, and
//!   missing values, never fire and break every stretch (manual: data already
//!   flagged to exclude is not considered). All rules of one run see the
//!   filtering as it was before the run, so their order does not matter.
//! - **Range is half-open,** like bins: outside means `v < min` or `v ≥ max`
//!   (manual, flag rule example 9). An infinite bound is no bound.
//! - **Changes are per step,** between adjacent steps that both have valid
//!   data, so the first step after a gap has no change (ESPEC §8, gap 12). A
//!   change above the limit flags the later step.
//! - **Stuck** is the complement: adjacent changes within the tolerance. The
//!   stretch starts at the first step of the flat, so `min_steps` counts the
//!   steps that hold the value.
//! - **Duration:** a stretch is flagged only when the test holds on at least
//!   `min_steps` consecutive steps (manual: time span, default one step).
//! - **What gets flagged:** a rule flags the column it searched; SD / min /
//!   max / gust children follow through filtering, as with manual flags.
//! - **Window tests** ([`Criterion::TwinDrift`], [`Criterion::ClockShift`])
//!   judge whole days or weeks instead of single steps (ARQUITETURA B4): a
//!   window that fires is flagged whole, adjacent firing windows form one
//!   stretch, and `min_steps` applies to that stretch. A window without
//!   enough data never fires.
//! - **Sector test** ([`Criterion::DirectionInSector`]) reads another
//!   column, the direction, step by step: it fires where both columns are
//!   valid and the direction lies in the sector, half-open like bins. It is
//!   the tower-shading rule ([`crate::shading`]).
//!
//! **Evidence and canonical segments.** Evidence is kept apart from the
//! segments, which stay in their one canonical form ([`flags::normalize`]):
//! equal flagging still means equal segment lists. Each evidence record is a
//! stretch inside a segment of the same column and flag, and the evidence
//! list has its own canonical form ([`normalize_evidence`]: sorted, exact
//! duplicates dropped, never merged, since each stretch has its own
//! statistic). Unflagging trims evidence like segments; flagging the same
//! steps again keeps it, because they are still flagged.
use crate::acc::DEGREES_PER_TURN;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::flags;
use crate::model::{Dataset, Segment};
use crate::time::{self, hour_of, HOURS_PER_DAY};
use std::cmp::Ordering;
use std::ops::Range;

/// Rules one run may hold: each costs a test per step of its columns (P9).
pub const MAX_RULES: usize = 64;
/// Stretches one run may flag. Each costs a segment and an evidence record,
/// and the undo history keeps a copy of the segment list per revision (P10);
/// a rule that fires this often is mis-set, and the error says so.
pub const MAX_RULE_HITS: usize = 50_000;
/// Steps scanned per call of [`scan_block`]: a function called many times
/// lets V8 switch the WASM loop to optimized code (P13).
const SCAN_BLOCK_STEPS: usize = 4_096;
/// Shortest stretch a value or change test can flag, in steps.
const LEAST_MIN_STEPS: usize = 1;
/// A flat needs two steps: one change within the tolerance.
const LEAST_STUCK_STEPS: usize = 2;
/// Twin anemometers are compared only where the reference reads at least
/// this, m/s: below it cup anemometers leave their calibrated range (IEC
/// 61400-12-1, annex F, from 4 m/s) and the ratio is noise.
pub const TWIN_MIN_SPEED_M_S: f64 = 4.0;
/// Share of a day's steps that must pair both anemometers for the day to count.
pub const MIN_DAY_SHARE: f64 = 0.5;
/// Days in a clock-shift window: a week averages out the weather, so its
/// diurnal cycle is the sun's.
pub const CLOCK_WINDOW_DAYS: usize = 7;
/// Hours of the day a window's cycle must have data in.
pub const MIN_PROFILE_HOURS: usize = 20;
/// A window's cycle must swing at least this share of the record's, or its
/// phase is weather, not clock.
pub const MIN_AMPLITUDE_SHARE: f64 = 0.5;
/// A clock shift is measured in whole hours, at most half a day either way.
const MAX_SHIFT_HOURS: f64 = HOURS_PER_DAY as f64 / 2.0;

/// What a rule looks for in one column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Criterion {
    /// The value lies outside `[min, max)`: `v < min` or `v ≥ max`, in the
    /// column's units; ±∞ means no bound.
    OutsideRange {
        /// Lower bound, inside the range.
        min: f64,
        /// Upper bound, outside the range.
        max: f64,
    },
    /// The change from the previous step, `|v_i − v_{i−1}|`, exceeds `limit`
    /// (column units per step).
    ChangeAbove {
        /// Largest change allowed, column units per step.
        limit: f64,
    },
    /// The change from the previous step stays within `tolerance` (column
    /// units per step): the sensor holds its value.
    Stuck {
        /// Largest change that still counts as no change.
        tolerance: f64,
    },
    /// A day whose mean ratio to `reference`, a twin anemometer, departs from
    /// the record's ratio by more than `tolerance`, a share (0.03 = 3 %): the
    /// level shift of a drifting or damaged sensor. Only steps where both
    /// speeds are valid and the reference reads at least
    /// [`TWIN_MIN_SPEED_M_S`] count, and a day needs [`MIN_DAY_SHARE`] of its
    /// steps paired.
    TwinDrift {
        /// The twin anemometer's column.
        reference: usize,
        /// Largest relative departure allowed.
        tolerance: f64,
    },
    /// A week whose diurnal cycle is shifted from the record's by at least
    /// `min_hours` whole hours: a logger clock that jumped, or a
    /// daylight-saving change. The shift is the lag that best lines up the
    /// week's mean hourly cycle with the record's (circular
    /// cross-correlation, one-hour resolution).
    ClockShift {
        /// Smallest shift flagged, hours (1 to 12).
        min_hours: f64,
    },
    /// The `direction` column reads inside the sector `[center − width/2,
    /// center + width/2)`, degrees, wrapping through north: the searched
    /// speed sensor is downwind of the tower (tower shading).
    DirectionInSector {
        /// The wind direction column.
        direction: usize,
        /// Centre of the sector, degrees.
        center: f64,
        /// Width of the sector, degrees (0 to 360].
        width: f64,
    },
}

impl Criterion {
    /// Whether `direction` (degrees) lies in this test's sector; false for
    /// other tests and for NaN.
    pub fn in_sector(&self, direction: f64) -> bool {
        let Criterion::DirectionInSector { center, width, .. } = *self else { return false };
        (direction - (center - width / 2.0)).rem_euclid(DEGREES_PER_TURN) < width
    }
}

/// Thresholds are never NaN once checked ([`RuleRun::new`],
/// [`restore_evidence`]), so equality is reflexive.
impl Eq for Criterion {}

impl Criterion {
    /// Fails unless the thresholds make sense and `min_steps` is long enough
    /// for this test.
    ///
    /// # Errors
    /// [`CoreError::RuleRangeInvalid`], [`CoreError::RuleThresholdInvalid`]
    /// or [`CoreError::RuleMinStepsTooSmall`].
    pub fn check(&self, min_steps: usize) -> CoreResult<()> {
        match *self {
            Criterion::OutsideRange { min, max } if !(min < max) => return Err(CoreError::RuleRangeInvalid { min, max }),
            Criterion::ChangeAbove { limit: threshold } | Criterion::Stuck { tolerance: threshold }
                if !(threshold >= 0.0 && threshold.is_finite()) =>
            {
                return Err(CoreError::RuleThresholdInvalid { threshold })
            }
            Criterion::TwinDrift { tolerance: threshold, .. } if !(threshold > 0.0 && threshold.is_finite()) => {
                return Err(CoreError::RuleThresholdInvalid { threshold })
            }
            Criterion::ClockShift { min_hours: threshold } if !((1.0..=MAX_SHIFT_HOURS).contains(&threshold)) => {
                return Err(CoreError::RuleThresholdInvalid { threshold })
            }
            Criterion::DirectionInSector { center, width, .. } if !(center.is_finite() && width > 0.0 && width <= DEGREES_PER_TURN) => {
                return Err(CoreError::RuleThresholdInvalid { threshold: if center.is_finite() { width } else { center } })
            }
            _ => {}
        }
        let least = if matches!(self, Criterion::Stuck { .. }) { LEAST_STUCK_STEPS } else { LEAST_MIN_STEPS };
        if min_steps < least {
            return Err(CoreError::RuleMinStepsTooSmall { min_steps, least });
        }
        Ok(())
    }

    /// Whether the test judges single steps (as opposed to whole windows).
    fn per_step(&self) -> bool {
        matches!(self, Criterion::OutsideRange { .. } | Criterion::ChangeAbove { .. } | Criterion::Stuck { .. })
    }

    /// Total order for canonical lists: by test, then thresholds.
    fn order(&self, other: &Self) -> Ordering {
        let key = |criterion: &Criterion| match *criterion {
            Criterion::OutsideRange { min, max } => (0, min, max, 0.0),
            Criterion::ChangeAbove { limit } => (1, limit, 0.0, 0.0),
            Criterion::Stuck { tolerance } => (2, tolerance, 0.0, 0.0),
            Criterion::TwinDrift { reference, tolerance } => (3, tolerance, reference as f64, 0.0),
            Criterion::ClockShift { min_hours } => (4, min_hours, 0.0, 0.0),
            Criterion::DirectionInSector { direction, center, width } => (5, center, width, direction as f64),
        };
        let (left, right) = (key(self), key(other));
        left.0
            .cmp(&right.0)
            .then(left.1.total_cmp(&right.1))
            .then(left.2.total_cmp(&right.2))
            .then(left.3.total_cmp(&right.3))
    }
}

/// Flag `flag` on the stretches of each of `columns` where `criterion` holds
/// for at least `min_steps` consecutive steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagRule {
    /// What the rule tests.
    pub criterion: Criterion,
    /// Shortest stretch flagged, in steps (≥ 1; ≥ 2 for [`Criterion::Stuck`]).
    pub min_steps: usize,
    /// Columns searched; each is flagged where it fires.
    pub columns: Vec<usize>,
    /// Index into `Dataset::flags`.
    pub flag: usize,
}

/// The checked rules of one run over `steps`. Built by [`RuleRun::new`], so
/// [`RuleRun::columns`], the columns the run can change, always matches them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRun {
    rules: Vec<FlagRule>,
    steps: Range<usize>,
    /// Every column some rule searches, sorted, without repeats.
    columns: Vec<usize>,
}

impl RuleRun {
    /// Checks `rules` (thresholds and durations; indices are checked against
    /// the dataset when the run is applied) and drops repeated columns.
    ///
    /// # Errors
    /// [`CoreError::TooManyRules`], or a rule's [`Criterion::check`] error.
    pub fn new(mut rules: Vec<FlagRule>, steps: Range<usize>) -> CoreResult<Self> {
        if rules.len() > MAX_RULES {
            return Err(CoreError::TooManyRules { rules: rules.len(), max: MAX_RULES });
        }
        for rule in &mut rules {
            rule.criterion.check(rule.min_steps)?;
            rule.columns.sort_unstable();
            rule.columns.dedup();
        }
        let mut columns: Vec<usize> = rules.iter().flat_map(|rule| rule.columns.iter().copied()).collect();
        columns.sort_unstable();
        columns.dedup();
        Ok(RuleRun { rules, steps, columns })
    }

    /// The rules, in the order given; a [`Finding::rule`] indexes them.
    pub fn rules(&self) -> &[FlagRule] {
        &self.rules
    }

    /// The steps searched.
    pub fn steps(&self) -> Range<usize> {
        self.steps.clone()
    }

    /// Every column some rule searches (and may flag), sorted.
    pub fn columns(&self) -> &[usize] {
        &self.columns
    }

    /// Fails for a column or flag index `dataset` lacks, or a derived column:
    /// those inherit their sources' flags, as with manual flags.
    fn check_indices(&self, dataset: &Dataset) -> CoreResult<()> {
        flags::check_columns(dataset, &self.columns)?;
        for rule in &self.rules {
            if let Criterion::TwinDrift { reference: other, .. } | Criterion::DirectionInSector { direction: other, .. } = rule.criterion {
                if other >= dataset.columns.len() {
                    return Err(CoreError::NoSuchColumn { column: other });
                }
            }
        }
        match self.rules.iter().find(|rule| rule.flag >= dataset.flags.len()) {
            Some(rule) => Err(CoreError::NoSuchFlag { flag: rule.flag }),
            None => Ok(()),
        }
    }
}

/// Why a stretch was flagged automatically: the test that fired, with its
/// thresholds, and the statistic that made it fire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evidence {
    /// Index into `Dataset::columns`.
    pub column: usize,
    /// Index into `Dataset::flags`.
    pub flag: usize,
    /// First flagged step.
    pub start: usize,
    /// One past the last flagged step.
    pub end: usize,
    /// The test that fired.
    pub criterion: Criterion,
    /// The rule's minimum duration, in steps.
    pub min_steps: usize,
    /// What fired: for [`Criterion::OutsideRange`] the value furthest outside
    /// the range; for [`Criterion::ChangeAbove`] the largest change between
    /// steps, signed (later minus earlier); both in the column's units. For
    /// [`Criterion::Stuck`] the length of the flat, in steps. For
    /// [`Criterion::TwinDrift`] the day's departure from the record's ratio,
    /// a signed share; for [`Criterion::ClockShift`] the week's shift in
    /// hours, positive when the logger's clock runs ahead; for
    /// [`Criterion::DirectionInSector`] the direction nearest the sector's
    /// centre, degrees. Window tests keep the strongest window of the stretch. NaN only when a saved project
    /// could not write it (an infinite value).
    pub statistic: f64,
    /// Step where the statistic was measured (the first step of a flat). It
    /// describes the stretch as the rule found it, so after a partial unflag
    /// it may lie outside `start..end`.
    pub statistic_step: usize,
}

impl Evidence {
    /// The flagged steps.
    pub fn steps(&self) -> Range<usize> {
        self.start..self.end
    }

    /// The segment this evidence explains.
    pub fn segment(&self) -> Segment {
        Segment { column: self.column, flag: self.flag, start: self.start, end: self.end }
    }

    /// Total order for the canonical list: by column, flag and steps first,
    /// like segments.
    fn order(&self, other: &Self) -> Ordering {
        let key = |evidence: &Evidence| {
            (evidence.column, evidence.flag, evidence.start, evidence.end, evidence.min_steps, evidence.statistic_step)
        };
        key(self)
            .cmp(&key(other))
            .then_with(|| self.criterion.order(&other.criterion))
            .then_with(|| self.statistic.total_cmp(&other.statistic))
    }
}

/// One stretch a run found, with the index of the rule that found it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finding {
    /// Index into [`RuleRun::rules`].
    pub rule: usize,
    /// The stretch and why it was found.
    pub evidence: Evidence,
}

/// What one rule found in one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleTally {
    /// Index into [`RuleRun::rules`].
    pub rule: usize,
    /// Index into `Dataset::columns`.
    pub column: usize,
    /// Stretches found.
    pub stretches: usize,
    /// Steps in those stretches (a rule's stretches never overlap).
    pub steps: usize,
}

/// Finds what `run` would flag in `dataset` under `view` (default filtering),
/// without changing anything: the flag rules' "test" (ESPEC §5.4).
///
/// Cost: one pass over each searched column, block by block; every rule on
/// the column tests a block while it is in cache. O(steps × rules on the
/// column), no allocation per step; findings are allocated as found. Natively,
/// three rules on 30 columns of a million steps take ~150 ms.
///
/// # Errors
/// [`CoreError::NoSuchColumn`] or [`CoreError::NoSuchFlag`] for an index
/// `dataset` lacks; [`CoreError::TooManyRuleHits`] past [`MAX_RULE_HITS`].
pub fn search(dataset: &Dataset, view: &FilterView, run: &RuleRun) -> CoreResult<Vec<Finding>> {
    run.check_indices(dataset)?;
    let window = run.steps.start.min(dataset.len)..run.steps.end.min(dataset.len);
    let mut findings = Vec::new();
    for &column in &run.columns {
        let mut scans: Vec<Scan> = run
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.criterion.per_step() && rule.columns.binary_search(&column).is_ok())
            .map(|(index, rule)| Scan::new(index, rule, column))
            .collect();
        let values = &dataset.columns[column].values;
        for valid in view.valid_runs(&[column], window.clone()) {
            // A valid run starts after an excluded stretch: no previous value.
            let mut previous = f64::NAN;
            let mut block_start = valid.start;
            while block_start < valid.end {
                let block_end = (block_start + SCAN_BLOCK_STEPS).min(valid.end);
                previous = scan_block(&mut scans, values, block_start..block_end, previous, &mut findings);
                if findings.len() > MAX_RULE_HITS {
                    return Err(CoreError::TooManyRuleHits { max: MAX_RULE_HITS });
                }
                block_start = block_end;
            }
            for scan in &mut scans {
                scan.close(valid.end, &mut findings);
            }
        }
    }
    let window_rules: Vec<(usize, &FlagRule)> = run.rules.iter().enumerate().filter(|(_, rule)| !rule.criterion.per_step()).collect();
    let by_days = |rule: &FlagRule| matches!(rule.criterion, Criterion::TwinDrift { .. } | Criterion::ClockShift { .. });
    let days = if window_rules.iter().any(|(_, rule)| by_days(rule)) { time::days(dataset) } else { Vec::new() };
    for (index, rule) in window_rules {
        // Without a positive step there are no days, and no day or week can fire.
        if by_days(rule) && days.is_empty() {
            continue;
        }
        for &column in &rule.columns {
            let windows = match rule.criterion {
                Criterion::TwinDrift { reference, tolerance } => twin_drift(dataset, view, &days, column, reference, tolerance, window.clone()),
                Criterion::ClockShift { min_hours } => clock_shift(dataset, view, &days, column, min_hours, window.clone()),
                Criterion::DirectionInSector { direction, .. } => in_sector(dataset, view, &rule.criterion, column, direction, window.clone()),
                _ => unreachable!("only window and sector tests reach here"),
            };
            stretches_of_windows(index, rule, column, &windows, &mut findings);
            if findings.len() > MAX_RULE_HITS {
                return Err(CoreError::TooManyRuleHits { max: MAX_RULE_HITS });
            }
        }
    }
    if findings.len() > MAX_RULE_HITS {
        return Err(CoreError::TooManyRuleHits { max: MAX_RULE_HITS });
    }
    Ok(findings)
}

/// One window a window test judged: its steps, and when it fired, how
/// strongly, with what statistic and where.
struct Window {
    steps: Range<usize>,
    fired: Option<(f64, f64, usize)>,
}

/// Days of `days` whose mean ratio of `column` to `reference` departs from
/// the record's by more than `tolerance` ([`Criterion::TwinDrift`]).
fn twin_drift(dataset: &Dataset, view: &FilterView, days: &[Range<usize>], column: usize, reference: usize, tolerance: f64, within: Range<usize>) -> Vec<Window> {
    let (speeds, references) = (&dataset.columns[column].values, &dataset.columns[reference].values);
    let mut sums = vec![(0usize, 0.0, 0.0); days.len()];
    let mut day = 0;
    for step in view.valid_runs(&[column, reference], within).into_iter().flatten() {
        while days[day].end <= step {
            day += 1;
        }
        let (speed, reference) = (speeds[step], references[step]);
        if !speed.is_nan() && reference >= TWIN_MIN_SPEED_M_S {
            let sum = &mut sums[day];
            *sum = (sum.0 + 1, sum.1 + speed, sum.2 + reference);
        }
    }
    let counted = |(day, sum): (usize, &(usize, f64, f64))| (sum.0 as f64 >= MIN_DAY_SHARE * days[day].len() as f64).then_some(*sum);
    let (total_speed, total_reference) =
        sums.iter().enumerate().filter_map(counted).fold((0.0, 0.0), |(speed, reference), sum| (speed + sum.1, reference + sum.2));
    let record_ratio = total_speed / total_reference;
    days.iter()
        .zip(sums.iter().enumerate())
        .map(|(steps, entry)| {
            let fired = counted(entry).and_then(|(_, speed, reference)| {
                let departure = speed / reference / record_ratio - 1.0;
                (departure.abs() > tolerance).then_some((departure.abs(), departure, steps.start))
            });
            Window { steps: steps.clone(), fired }
        })
        .collect()
}

/// Weeks whose diurnal cycle of `column` is shifted from the record's by at
/// least `min_hours` ([`Criterion::ClockShift`]).
fn clock_shift(dataset: &Dataset, view: &FilterView, days: &[Range<usize>], column: usize, min_hours: f64, within: Range<usize>) -> Vec<Window> {
    let values = &dataset.columns[column].values;
    let weeks: Vec<Range<usize>> = days.chunks(CLOCK_WINDOW_DAYS).map(|week| week[0].start..week[week.len() - 1].end).collect();
    let mut cycles = vec![[(0usize, 0.0); HOURS_PER_DAY]; weeks.len()];
    let mut record = [(0usize, 0.0); HOURS_PER_DAY];
    let mut week = 0;
    for step in view.valid_runs(&[column], within).into_iter().flatten() {
        while weeks[week].end <= step {
            week += 1;
        }
        let value = values[step];
        if !value.is_nan() {
            let hour = hour_of(dataset, step);
            for cell in [&mut cycles[week][hour], &mut record[hour]] {
                *cell = (cell.0 + 1, cell.1 + value);
            }
        }
    }
    let means = |cycle: &[(usize, f64); HOURS_PER_DAY]| cycle.map(|(count, sum)| if count > 0 { sum / count as f64 } else { f64::NAN });
    let record = means(&record);
    let amplitude = |cycle: &[f64]| {
        let present = cycle.iter().copied().filter(|value| !value.is_nan());
        present.clone().fold(f64::NEG_INFINITY, f64::max) - present.fold(f64::INFINITY, f64::min)
    };
    let record_amplitude = amplitude(&record);
    weeks
        .iter()
        .zip(&cycles)
        .map(|(steps, cycle)| {
            let cycle = means(cycle);
            let hours = cycle.iter().filter(|value| !value.is_nan()).count();
            let usable = hours >= MIN_PROFILE_HOURS && amplitude(&cycle) >= MIN_AMPLITUDE_SHARE * record_amplitude;
            let fired = usable.then(|| best_lag(&cycle, &record)).flatten().and_then(|lag| {
                let hours = lag as f64;
                (hours.abs() >= min_hours).then_some((hours.abs(), hours, steps.start))
            });
            Window { steps: steps.clone(), fired }
        })
        .collect()
}

/// Runs of steps where `column` and `direction` are both valid and the
/// direction lies in `criterion`'s sector ([`Criterion::DirectionInSector`]),
/// each a firing window followed by an empty silent one, so runs never
/// merge. Each keeps the direction nearest the sector's centre. O(steps).
fn in_sector(dataset: &Dataset, view: &FilterView, criterion: &Criterion, column: usize, direction: usize, within: Range<usize>) -> Vec<Window> {
    let Criterion::DirectionInSector { center, .. } = *criterion else { return Vec::new() };
    let (values, directions) = (&dataset.columns[column].values, &dataset.columns[direction].values);
    let off_center = |degrees: f64| {
        let turn = (degrees - center).rem_euclid(DEGREES_PER_TURN);
        turn.min(DEGREES_PER_TURN - turn)
    };
    let mut windows = Vec::new();
    // The open run: its first step and its best (strength, direction, step) so far.
    let mut open: Option<(usize, (f64, f64, usize))> = None;
    let close = |open: &mut Option<(usize, (f64, f64, usize))>, end: usize, windows: &mut Vec<Window>| {
        if let Some((start, fired)) = open.take() {
            windows.push(Window { steps: start..end, fired: Some(fired) });
            windows.push(Window { steps: end..end, fired: None });
        }
    };
    for valid in view.valid_runs(&[column, direction], within) {
        for step in valid.clone() {
            let degrees = directions[step];
            if !criterion.in_sector(degrees) || values[step].is_nan() {
                close(&mut open, step, &mut windows);
                continue;
            }
            // Nearer the centre is stronger.
            let strength = -off_center(degrees);
            match open.as_mut() {
                Some((_, best)) if strength > best.0 => *best = (strength, degrees, step),
                Some(_) => {}
                None => open = Some((step, (strength, degrees, step))),
            }
        }
        close(&mut open, valid.end, &mut windows);
    }
    windows
}

/// The lag, in hours (−11 to 12), that best lines up `cycle` with `record`:
/// `cycle[h]` ≈ `record[h − lag]`. Hours missing in either are skipped.
fn best_lag(cycle: &[f64; HOURS_PER_DAY], record: &[f64; HOURS_PER_DAY]) -> Option<i64> {
    let centred = |values: &[f64; HOURS_PER_DAY]| {
        let present: Vec<f64> = values.iter().copied().filter(|value| !value.is_nan()).collect();
        let mean = present.iter().sum::<f64>() / present.len() as f64;
        values.map(|value| value - mean)
    };
    let (cycle, record) = (centred(cycle), centred(record));
    let correlation = |lag: usize| -> f64 {
        (0..HOURS_PER_DAY)
            .map(|hour| cycle[hour] * record[(hour + HOURS_PER_DAY - lag) % HOURS_PER_DAY])
            .filter(|product| !product.is_nan())
            .sum()
    };
    let best = (0..HOURS_PER_DAY).max_by(|&left, &right| correlation(left).total_cmp(&correlation(right)))?;
    let lag = best as i64;
    Some(if lag as f64 > MAX_SHIFT_HOURS { lag - HOURS_PER_DAY as i64 } else { lag })
}

/// Runs of adjacent firing windows as findings, each with its strongest
/// window's statistic, when at least `min_steps` long.
fn stretches_of_windows(rule_index: usize, rule: &FlagRule, column: usize, windows: &[Window], findings: &mut Vec<Finding>) {
    let mut open: Option<(usize, usize, f64, f64, usize)> = None;
    let close = |open: &mut Option<(usize, usize, f64, f64, usize)>, findings: &mut Vec<Finding>| {
        if let Some((start, end, _, statistic, statistic_step)) = open.take() {
            if end - start >= rule.min_steps {
                let evidence = Evidence { column, flag: rule.flag, start, end, criterion: rule.criterion, min_steps: rule.min_steps, statistic, statistic_step };
                findings.push(Finding { rule: rule_index, evidence });
            }
        }
    };
    for window in windows {
        match (window.fired, open.as_mut()) {
            (Some((strength, statistic, step)), Some(stretch)) => {
                stretch.1 = window.steps.end;
                if strength > stretch.2 {
                    (stretch.2, stretch.3, stretch.4) = (strength, statistic, step);
                }
            }
            (Some((strength, statistic, step)), None) => open = Some((window.steps.start, window.steps.end, strength, statistic, step)),
            (None, _) => close(&mut open, findings),
        }
    }
    close(&mut open, findings);
}

/// Runs `run` on `dataset`: flags every stretch found and keeps its evidence.
/// Returns the findings, including stretches that were already flagged and
/// explained (then nothing changes). Leaves `dataset` untouched on error.
///
/// Cost: [`search`] plus O((segments + evidence) · log) to merge.
///
/// # Errors
/// As [`search`].
pub fn apply(dataset: &mut Dataset, run: &RuleRun) -> CoreResult<Vec<Finding>> {
    let findings = search(dataset, &FilterView::new(dataset), run)?;
    if findings.is_empty() {
        return Ok(findings);
    }
    dataset.segments.extend(findings.iter().map(|finding| finding.evidence.segment()));
    flags::normalize(&mut dataset.segments);
    let mut evidence = Vec::with_capacity(dataset.evidence.len() + findings.len());
    evidence.extend_from_slice(&dataset.evidence);
    evidence.extend(findings.iter().map(|finding| finding.evidence));
    normalize_evidence(&mut evidence);
    // Unchanged evidence keeps its allocation, still shared with the history.
    if !same_evidence(&evidence, &dataset.evidence) {
        dataset.evidence = evidence.into();
    }
    Ok(findings)
}

/// Stretches and steps per rule and searched column of `run`, zeros
/// included, ordered by rule and column. O(findings · log).
pub fn tally(run: &RuleRun, findings: &[Finding]) -> Vec<RuleTally> {
    // Rules in order, each with its columns sorted: already ordered by (rule, column).
    let mut tallies: Vec<RuleTally> = run
        .rules
        .iter()
        .enumerate()
        .flat_map(|(rule, flag_rule)| flag_rule.columns.iter().map(move |&column| RuleTally { rule, column, stretches: 0, steps: 0 }))
        .collect();
    for finding in findings {
        let key = (finding.rule, finding.evidence.column);
        if let Ok(index) = tallies.binary_search_by_key(&key, |tally| (tally.rule, tally.column)) {
            tallies[index].stretches += 1;
            tallies[index].steps += finding.evidence.end - finding.evidence.start;
        }
    }
    tallies
}

/// Puts evidence in its canonical form: sorted, with exact duplicates (a rule
/// re-run on the same data) dropped. Records are never merged: each keeps
/// its own statistic.
pub fn normalize_evidence(evidence: &mut Vec<Evidence>) {
    evidence.sort_unstable_by(Evidence::order);
    evidence.dedup_by(|later, kept| later.order(kept) == Ordering::Equal);
}

/// Whether two canonical evidence lists are the same, NaN statistics
/// included. O(1) when they share an allocation.
pub fn same_evidence(left: &[Evidence], right: &[Evidence]) -> bool {
    std::ptr::eq(left, right)
        || (left.len() == right.len() && left.iter().zip(right).all(|(left, right)| left.order(right) == Ordering::Equal))
}

/// Trims the evidence of `columns` (all flags, or only `flag`) out of
/// `steps`, as [`flags::unflag`] trims segments. Untouched evidence keeps its
/// allocation.
pub(crate) fn trim_evidence(dataset: &mut Dataset, columns: &[usize], flag: Option<usize>, steps: &Range<usize>) {
    let targeted = |evidence: &Evidence| {
        evidence.start < steps.end
            && steps.start < evidence.end
            && columns.contains(&evidence.column)
            && flag.map_or(true, |flag| flag == evidence.flag)
    };
    if !dataset.evidence.iter().any(targeted) {
        return;
    }
    let mut kept = Vec::with_capacity(dataset.evidence.len() + 1);
    for &evidence in dataset.evidence.iter() {
        if !targeted(&evidence) {
            kept.push(evidence);
            continue;
        }
        if evidence.start < steps.start {
            kept.push(Evidence { end: steps.start, ..evidence });
        }
        if steps.end < evidence.end {
            kept.push(Evidence { start: steps.end, ..evidence });
        }
    }
    normalize_evidence(&mut kept);
    dataset.evidence = kept.into();
}

/// Puts back evidence saved with a project, after [`flags::restore`] put back
/// the segments.
///
/// # Errors
/// [`CoreError::NoSuchColumn`], [`CoreError::NoSuchFlag`],
/// [`CoreError::SegmentOutOfRange`], a [`Criterion::check`] error, or
/// [`CoreError::EvidenceNotFlagged`] for a record that does not fit the
/// dataset; nothing is changed then. A project file comes from disk, so
/// nothing in it is trusted.
pub fn restore_evidence(dataset: &mut Dataset, mut evidence: Vec<Evidence>) -> CoreResult<()> {
    for record in &evidence {
        if record.column >= dataset.columns.len() {
            return Err(CoreError::NoSuchColumn { column: record.column });
        }
        if record.flag >= dataset.flags.len() {
            return Err(CoreError::NoSuchFlag { flag: record.flag });
        }
        if record.start >= record.end || record.end > dataset.len {
            return Err(CoreError::SegmentOutOfRange { start: record.start, end: record.end, len: dataset.len });
        }
        if record.statistic_step >= dataset.len {
            let step = record.statistic_step;
            return Err(CoreError::SegmentOutOfRange { start: step, end: step + 1, len: dataset.len });
        }
        record.criterion.check(record.min_steps)?;
        if !is_flagged(&dataset.segments, record) {
            return Err(CoreError::EvidenceNotFlagged { column: record.column, start: record.start, end: record.end });
        }
    }
    normalize_evidence(&mut evidence);
    dataset.evidence = evidence.into();
    Ok(())
}

/// Whether one segment of `segments` (canonical) covers `evidence`.
/// O(log segments).
fn is_flagged(segments: &[Segment], evidence: &Evidence) -> bool {
    let key = (evidence.column, evidence.flag, evidence.start);
    let after = segments.partition_point(|segment| (segment.column, segment.flag, segment.start) <= key);
    after.checked_sub(1).map(|index| &segments[index]).is_some_and(|segment| {
        segment.column == evidence.column && segment.flag == evidence.flag && segment.end >= evidence.end
    })
}

/// One rule's search state over one column: the stretch open so far and its
/// strongest statistic.
struct Scan {
    rule: usize,
    criterion: Criterion,
    min_steps: usize,
    column: usize,
    flag: usize,
    /// First step of the stretch being followed, if any.
    open: Option<usize>,
    /// How strongly the stretch fired so far (distance outside the range,
    /// size of the change); the statistic is taken where it peaks.
    strength: f64,
    statistic: f64,
    statistic_step: usize,
}

impl Scan {
    fn new(rule: usize, flag_rule: &FlagRule, column: usize) -> Self {
        Scan {
            rule,
            criterion: flag_rule.criterion,
            min_steps: flag_rule.min_steps,
            column,
            flag: flag_rule.flag,
            open: None,
            strength: 0.0,
            statistic: 0.0,
            statistic_step: 0,
        }
    }

    /// Follows `values[block]` with `test`, which gives (strength,
    /// statistic) for a value and the one before it when the rule fires.
    /// `previous` is the value before the block, NaN for none. A NaN value
    /// or change fails every comparison, so missing data never fires and
    /// breaks the stretch.
    #[inline(always)]
    fn sweep(
        &mut self,
        values: &[f64],
        block: Range<usize>,
        mut previous: f64,
        findings: &mut Vec<Finding>,
        test: impl Fn(f64, f64) -> Option<(f64, f64)>,
    ) {
        for step in block {
            let value = values[step];
            match test(value, previous) {
                Some((strength, statistic)) => self.extend(step, strength, statistic),
                None => self.close(step, findings),
            }
            previous = value;
        }
    }

    /// Adds firing step `step` to the stretch, opening one if needed.
    #[inline(always)]
    fn extend(&mut self, step: usize, strength: f64, statistic: f64) {
        match self.open {
            None => {
                // A flat includes the step it started from.
                let is_flat = matches!(self.criterion, Criterion::Stuck { .. });
                let start = if is_flat { step - 1 } else { step };
                self.open = Some(start);
                self.strength = strength;
                self.statistic = statistic;
                self.statistic_step = start;
            }
            Some(_) if strength > self.strength => {
                self.strength = strength;
                self.statistic = statistic;
                self.statistic_step = step;
            }
            Some(_) => {}
        }
    }

    /// Ends the open stretch before step `end`, keeping it when long enough.
    #[inline]
    fn close(&mut self, end: usize, findings: &mut Vec<Finding>) {
        let Some(start) = self.open.take() else { return };
        if end - start < self.min_steps {
            return;
        }
        let statistic = match self.criterion {
            Criterion::Stuck { .. } => (end - start) as f64,
            _ => self.statistic,
        };
        let evidence = Evidence {
            column: self.column,
            flag: self.flag,
            start,
            end,
            criterion: self.criterion,
            min_steps: self.min_steps,
            statistic,
            statistic_step: self.statistic_step,
        };
        findings.push(Finding { rule: self.rule, evidence });
    }
}

/// Runs every scan over `values[block]`, steps that default filtering keeps;
/// `previous` is the value before the block (NaN for none). Returns the last
/// value, for the next block. Rule by rule: the block stays in cache, and
/// each test gets its own tight loop. A separate, often-called function so
/// the WASM build gets optimized code (P13).
#[inline(never)]
fn scan_block(scans: &mut [Scan], values: &[f64], block: Range<usize>, previous: f64, findings: &mut Vec<Finding>) -> f64 {
    let last = values[block.end - 1];
    for scan in scans.iter_mut() {
        match scan.criterion {
            Criterion::OutsideRange { min, max } => scan.sweep(values, block.clone(), previous, findings, |value, _| {
                let below = value < min;
                (below || value >= max).then_some((if below { min - value } else { value - max }, value))
            }),
            Criterion::ChangeAbove { limit } => scan.sweep(values, block.clone(), previous, findings, |value, previous| {
                let change = value - previous;
                (change.abs() > limit).then_some((change.abs(), change))
            }),
            Criterion::Stuck { tolerance } => scan.sweep(values, block.clone(), previous, findings, |value, previous| {
                ((value - previous).abs() <= tolerance).then_some((0.0, 0.0))
            }),
            Criterion::TwinDrift { .. } | Criterion::ClockShift { .. } | Criterion::DirectionInSector { .. } => {
                unreachable!("window and sector tests are not scanned step by step")
            }
        }
    }
    last
}

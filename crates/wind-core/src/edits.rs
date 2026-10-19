//! Edits that change values (ESPEC-INTERFACE §1, menu Revisar): scale and
//! offset, delete data and the two time shifts of the reference product
//! [manual: dlg-revise-apply-scale-and-offset.htm, dlg-revise-delete-data.htm,
//! dlg-revise-apply-time-shift.htm]. Each runs as [`crate::ops::Op::Edit`],
//! so it is undone like any edit.
//!
//! Rules for the whole module:
//! - **Only the columns chosen.** As in the reference product, a mean's SD /
//!   min / max / gust children are not edited with it: an offset must not
//!   reach a standard deviation, so the user picks the columns.
//! - **Derived columns follow.** They take no edit of their own; after an
//!   edit, the ones computed from an edited column are computed again
//!   ([`crate::derived::recompute`]).
//! - **Whole columns replaced.** An edit writes new `Arc<[f64]>` columns and
//!   leaves the old ones to the undo history, so revisions share every
//!   column they did not change.
//! - **Flags stay in time.** Shifting data does not move flags: a flag marks
//!   a stretch of time.
//! - **The edits in effect are kept** in [`Dataset::edits`]: replayed on a
//!   re-import ([`carry_over`]) and saved in project files.
//! - **Reconstruction** ([`Edit::Reconstruct`]) fills only missing values:
//!   flagged data keeps its flag (tower shadow is never reconstructed), and
//!   every filled step gets the [`RECONSTRUCTED_FLAG`], included by default,
//!   so what was filled can be seen and filtered out. By another height,
//!   the source speed × (z/z_source)^α with α the month × hour median
//!   exponent between the two ([`derived::ShearTable`]); by Markov, a chain
//!   over [`MARKOV_BIN`]-wide bins of the column's own values, each gap
//!   walked from the value before it, each value drawn among the column's
//!   values in its bin, with a fixed seed (so a replay gives the same fill).
use crate::bootstrap::SplitMix64;
use crate::derived::{self, kelvin_offset, ShearTable};
use crate::error::{CoreError, CoreResult};
use crate::flags;
use crate::model::{ColumnType, DataColumn, Dataset, Flag, Segment, Subtype};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

/// Degrees in a full turn: edited directions are brought back into `0..360`.
pub const DEGREES_PER_TURN: f64 = 360.0;
/// The flag of reconstructed steps, as (name, CSS colour); data under it is
/// kept by default filtering.
pub const RECONSTRUCTED_FLAG: (&str, &str) = ("Reconstruído", "#3a9d5d");
/// Width of a Markov reconstruction's bins, in the column's units.
pub const MARKOV_BIN: f64 = 1.0;
/// Seed of every Markov reconstruction: fixed, so a replay fills the same.
const MARKOV_SEED: u64 = 0x0057_1D0B_0D1A_2026;

/// How a reconstruction fills a column's gaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reconstruction {
    /// From another speed column at another height, by the power law.
    FromHeight {
        /// The source column.
        source: usize,
    },
    /// A Markov chain over the column's own values.
    Markov,
}

/// One change of values.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// `value · scale + offset` on `steps` of `columns`. Directions are
    /// brought back into `0..360`; temperatures in known units are scaled in
    /// kelvin and brought back, since °C has its zero away from absolute zero.
    ScaleOffset {
        /// Column indices.
        columns: Vec<usize>,
        /// The steps edited.
        steps: Range<usize>,
        /// Factor applied first.
        scale: f64,
        /// Added after scaling, in the column's units.
        offset: f64,
    },
    /// Deletes `steps` of `columns`: their values become missing (NaN).
    Delete {
        /// Column indices.
        columns: Vec<usize>,
        /// The steps deleted.
        steps: Range<usize>,
    },
    /// Moves the values of `steps` of `columns` by `by` steps (later when
    /// positive), over whatever is there; the steps left behind become
    /// missing, and values moved past either end of the record are lost.
    ShiftSteps {
        /// Column indices.
        columns: Vec<usize>,
        /// The steps moved.
        steps: Range<usize>,
        /// Steps to move by.
        by: i64,
    },
    /// Fills the missing values of `steps` of `columns` ([`Reconstruction`]),
    /// flagging what it filled with [`RECONSTRUCTED_FLAG`].
    Reconstruct {
        /// Column indices.
        columns: Vec<usize>,
        /// The steps searched for gaps.
        steps: Range<usize>,
        /// How.
        method: Reconstruction,
    },
    /// Moves the whole record by `by_s` seconds: only the time stamps change
    /// (a daylight-saving or clock fix).
    ShiftStart {
        /// Seconds to add to every time stamp.
        by_s: i64,
    },
}

impl Edit {
    /// Applies the edit to `dataset`, computes the derived columns of the
    /// edited ones again and logs it in [`Dataset::edits`]. Steps past the
    /// record are ignored. Leaves `dataset` untouched on error.
    ///
    /// # Errors
    /// [`CoreError::NoSuchColumn`], [`CoreError::DerivedColumnNotEditable`],
    /// [`CoreError::ScaleOffsetNotFinite`], or
    /// [`CoreError::TimestampOutOfRange`] for a shift off the calendar.
    pub fn apply(&self, dataset: &mut Dataset) -> CoreResult<()> {
        match self {
            Edit::ScaleOffset { columns, steps, scale, offset } => {
                if !(scale.is_finite() && offset.is_finite()) {
                    return Err(CoreError::ScaleOffsetNotFinite { scale: *scale, offset: *offset });
                }
                edit_columns(dataset, columns, |column, values| {
                    let transform = scale_offset(column, *scale, *offset);
                    for value in &mut values[clamped(steps, column.values.len())] {
                        *value = transform(*value);
                    }
                })?;
            }
            Edit::Delete { columns, steps } => edit_columns(dataset, columns, |column, values| {
                values[clamped(steps, column.values.len())].fill(f64::NAN);
            })?,
            Edit::ShiftSteps { columns, steps, by } => edit_columns(dataset, columns, |column, values| {
                let source = clamped(steps, values.len());
                values[source.clone()].fill(f64::NAN);
                for step in source {
                    if let Some(target) = moved(step, *by, values.len()) {
                        values[target] = column.values[step];
                    }
                }
            })?,
            Edit::Reconstruct { columns, steps, method } => reconstruct(dataset, columns, steps, *method)?,
            Edit::ShiftStart { by_s } => {
                let start = dataset.start.checked_add(*by_s).ok_or(CoreError::TimestampOutOfRange { seconds: dataset.start })?;
                let end = start.checked_add(dataset.len as i64 * dataset.step_s).ok_or(CoreError::TimestampOutOfRange { seconds: start })?;
                for seconds in [start, end] {
                    chrono::DateTime::from_timestamp(seconds, 0).ok_or(CoreError::TimestampOutOfRange { seconds })?;
                }
                dataset.start = start;
            }
        }
        dataset.edits.push(self.clone());
        Ok(())
    }

    /// The columns and steps whose values the edit can change: what
    /// `DayStats::refresh` needs after applying, undoing or redoing it.
    /// Empty for [`Edit::ShiftStart`], which moves every day boundary
    /// instead: see [`Edit::moves_time`].
    pub fn reach(&self) -> (&[usize], Range<usize>) {
        match self {
            Edit::ScaleOffset { columns, steps, .. } | Edit::Delete { columns, steps } | Edit::Reconstruct { columns, steps, .. } => {
                (columns, steps.clone())
            }
            Edit::ShiftSteps { columns, steps, by } => {
                let start = steps.start as i64 + (*by).min(0);
                let end = (steps.end as i64).saturating_add((*by).max(0));
                (columns, to_step(start)..to_step(end))
            }
            Edit::ShiftStart { .. } => (&[], 0..0),
        }
    }

    /// Whether the edit moves the record in time, so per-day summaries must
    /// be built again rather than refreshed.
    pub fn moves_time(&self) -> bool {
        matches!(self, Edit::ShiftStart { .. })
    }

    /// The same edit with columns and steps renumbered by `column` and
    /// `steps` (`None` = not in the new dataset); `None` when no edited
    /// column or step is left.
    fn renumbered(&self, column: impl Fn(usize) -> Option<usize>, steps: impl Fn(Range<usize>) -> Option<Range<usize>>) -> Option<Edit> {
        let columns_in = |columns: &[usize]| {
            let found: Vec<usize> = columns.iter().filter_map(|&old| column(old)).collect();
            (!found.is_empty()).then_some(found)
        };
        Some(match self {
            Edit::ScaleOffset { columns, steps: old, scale, offset } => {
                Edit::ScaleOffset { columns: columns_in(columns)?, steps: steps(old.clone())?, scale: *scale, offset: *offset }
            }
            Edit::Delete { columns, steps: old } => Edit::Delete { columns: columns_in(columns)?, steps: steps(old.clone())? },
            Edit::ShiftSteps { columns, steps: old, by } => Edit::ShiftSteps { columns: columns_in(columns)?, steps: steps(old.clone())?, by: *by },
            Edit::Reconstruct { columns, steps: old, method } => Edit::Reconstruct {
                columns: columns_in(columns)?,
                steps: steps(old.clone())?,
                method: match method {
                    Reconstruction::FromHeight { source } => Reconstruction::FromHeight { source: column(*source)? },
                    Reconstruction::Markov => Reconstruction::Markov,
                },
            },
            Edit::ShiftStart { by_s } => Edit::ShiftStart { by_s: *by_s },
        })
    }
}

/// Replays the edits of `old` on `new`, the same file just read again
/// (perhaps with another configuration), before its flags are carried over.
/// Columns are matched by id and steps by file time stamp, as flags are
/// ([`crate::flags::carry_over`]); an edit whose columns or steps are all
/// gone is dropped. Time shifts replay too, so `new` ends up on `old`'s clock.
///
/// # Errors
/// An edit's own check; unlikely, since each passed it on `old`.
pub fn carry_over(old: &Dataset, new: &mut Dataset) -> CoreResult<()> {
    // Old steps are mapped through the file's own clock: the stamps without
    // the time shifts, which the replay adds back in order.
    let shifted_s: i64 = old.edits.iter().map(|edit| if let Edit::ShiftStart { by_s } = edit { *by_s } else { 0 }).sum();
    let start_in_new = |old_step: usize| old.stamp_at(old_step) - shifted_s + new.timestamp_meaning.shift(new.step_s);
    let column_in_new = |old_column: usize| {
        let old_id = &old.columns[old_column].id;
        new.columns.iter().position(|candidate| &candidate.id == old_id)
    };
    let steps_in_new = |old_steps: Range<usize>| {
        let start = new.index_of(start_in_new(old_steps.start));
        let end = new.first_step_from(start_in_new(old_steps.end));
        (start < end).then_some(start..end)
    };
    // Renumbered on `new` as read, before any replayed shift moves its clock.
    let edits: Vec<Edit> = old.edits.iter().filter_map(|edit| edit.renumbered(&column_in_new, &steps_in_new)).collect();
    edits.iter().try_for_each(|edit| edit.apply(new))
}

/// Replaces each of `columns` with a copy that `change` edited (it gets the
/// old column and the copy), then computes the derived columns again. A
/// column listed twice is edited once.
fn edit_columns(dataset: &mut Dataset, columns: &[usize], change: impl Fn(&DataColumn, &mut [f64])) -> CoreResult<()> {
    for &index in columns {
        let column = dataset.columns.get(index).ok_or(CoreError::NoSuchColumn { column: index })?;
        if column.derived.is_some() {
            return Err(CoreError::DerivedColumnNotEditable { column: column.id.clone() });
        }
    }
    let mut columns = columns.to_vec();
    columns.sort_unstable();
    columns.dedup();
    for &index in &columns {
        let column = &dataset.columns[index];
        let mut values = column.values.to_vec();
        change(column, &mut values);
        dataset.columns[index].values = Arc::from(values);
    }
    derived::recompute(dataset, &columns);
    Ok(())
}

/// [`Edit::Reconstruct`]: checks everything first, then fills each
/// column's gaps, computes the derived columns again and flags the filled
/// steps.
fn reconstruct(dataset: &mut Dataset, columns: &[usize], steps: &Range<usize>, method: Reconstruction) -> CoreResult<()> {
    for &index in columns {
        let column = dataset.columns.get(index).ok_or(CoreError::NoSuchColumn { column: index })?;
        if column.derived.is_some() {
            return Err(CoreError::DerivedColumnNotEditable { column: column.id.clone() });
        }
        match method {
            Reconstruction::FromHeight { source } => {
                let from = dataset.columns.get(source).ok_or(CoreError::NoSuchColumn { column: source })?;
                for checked in [column, from] {
                    if checked.height_m.is_none() {
                        return Err(CoreError::ColumnWithoutHeight { column: checked.id.clone() });
                    }
                }
                if source == index || from.height_m == column.height_m {
                    return Err(CoreError::ShearNeedsTwoHeights);
                }
            }
            Reconstruction::Markov if column.is_angle() => return Err(CoreError::MarkovNeedsLinearColumn { column: column.id.clone() }),
            Reconstruction::Markov => {}
        }
    }
    let mut filled: Vec<Segment> = Vec::new();
    for &index in columns {
        let window = clamped(steps, dataset.len);
        let mut values = dataset.columns[index].values.to_vec();
        let gaps: Vec<usize> = window.filter(|&step| values[step].is_nan()).collect();
        match method {
            Reconstruction::FromHeight { source } => {
                let table = ShearTable::new(dataset, index, source);
                for &step in &gaps {
                    // Effective heights: both less the step's displacement height.
                    let ratio = derived::height_ratio(dataset, index, source, table.displacement(step));
                    values[step] = dataset.columns[source].values[step] * ratio.powf(table.alpha(dataset, step));
                }
            }
            Reconstruction::Markov => markov_fill(&mut values, &gaps, MARKOV_BIN, MARKOV_SEED ^ (index as u64).rotate_left(32)),
        }
        for &step in gaps.iter().filter(|&&step| !values[step].is_nan()) {
            match filled.last_mut() {
                Some(segment) if segment.column == index && segment.end == step => segment.end = step + 1,
                _ => filled.push(Segment { column: index, flag: 0, start: step, end: step + 1 }),
            }
        }
        dataset.columns[index].values = Arc::from(values);
    }
    derived::recompute(dataset, columns);
    if !filled.is_empty() {
        let flag = reconstructed_flag(dataset);
        dataset.segments.extend(filled.into_iter().map(|segment| Segment { flag, ..segment }));
        flags::normalize(&mut dataset.segments);
    }
    Ok(())
}

/// Index of the [`RECONSTRUCTED_FLAG`], added to the dataset's flags when missing.
fn reconstructed_flag(dataset: &mut Dataset) -> usize {
    let (name, color) = RECONSTRUCTED_FLAG;
    dataset.flags.iter().position(|flag| flag.name == name).unwrap_or_else(|| {
        dataset.flags.push(Flag { name: name.to_string(), color: color.to_string(), include: true, notes: String::new() });
        dataset.flags.len() - 1
    })
}

/// Fills `values` at `gaps` (ascending) by a Markov chain over `width`-wide
/// bins of the valid values: each gap step's bin is drawn from the
/// transitions out of the previous value's bin (the record's spread of
/// values when that bin has none), its value among the values in that bin;
/// `seed` fixes the draws. Nothing is filled with fewer than two valid values.
pub(crate) fn markov_fill(values: &mut [f64], gaps: &[usize], width: f64, seed: u64) {
    let bin = |value: f64| (value / width).floor() as i64;
    let valid: Vec<f64> = values.iter().copied().filter(|value| !value.is_nan()).collect();
    if valid.len() < 2 {
        return;
    }
    let mut members: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    for &value in &valid {
        members.entry(bin(value)).or_default().push(value);
    }
    let mut transitions: BTreeMap<i64, BTreeMap<i64, u64>> = BTreeMap::new();
    for pair in values.windows(2) {
        if !pair[0].is_nan() && !pair[1].is_nan() {
            *transitions.entry(bin(pair[0])).or_default().entry(bin(pair[1])).or_default() += 1;
        }
    }
    let mut random = SplitMix64::new(seed);
    let mut previous = f64::NAN;
    let mut last_gap: Option<usize> = None;
    for &step in gaps {
        // A new gap starts from the value just before it.
        if last_gap != step.checked_sub(1) {
            previous = step.checked_sub(1).map_or(f64::NAN, |before| values[before]);
        }
        let next_bin = match transitions.get(&bin(previous)).filter(|_| !previous.is_nan()) {
            Some(row) => {
                let total: u64 = row.values().sum();
                let mut draw = random.below(total as usize) as u64;
                *row.iter().find(|(_, &count)| if draw < count { true } else { draw -= count; false }).expect("draws stay below the total").0
            }
            None => bin(valid[random.below(valid.len())]),
        };
        let candidates = &members[&next_bin];
        previous = candidates[random.below(candidates.len())];
        values[step] = previous;
        last_gap = Some(step);
    }
}

/// What scale and offset do to one value of `column`.
fn scale_offset(column: &DataColumn, scale: f64, offset: f64) -> impl Fn(f64) -> f64 {
    let is_angle = column.is_angle();
    let kelvin = match (column.kind, column.subtype) {
        // A deviation is a difference: it has no zero to move.
        (ColumnType::Temperature, Subtype::StdDev) => 0.0,
        (ColumnType::Temperature, _) => kelvin_offset(&column.units).unwrap_or(0.0),
        _ => 0.0,
    };
    move |value| {
        let edited = (value + kelvin) * scale + offset - kelvin;
        if is_angle {
            edited.rem_euclid(DEGREES_PER_TURN)
        } else {
            edited
        }
    }
}

/// `steps` cut to a record of `len` steps.
fn clamped(steps: &Range<usize>, len: usize) -> Range<usize> {
    steps.start.min(len)..steps.end.min(len).max(steps.start.min(len))
}

/// Where step `step` lands moved by `by` steps, if still in a record of `len`.
fn moved(step: usize, by: i64, len: usize) -> Option<usize> {
    let target = (step as i64).checked_add(by)?;
    usize::try_from(target).ok().filter(|&target| target < len)
}

/// A step index from a signed one, clamped at 0 and at `usize::MAX`.
fn to_step(index: i64) -> usize {
    usize::try_from(index.max(0)).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_steps_stay_in_the_record() {
        assert_eq!(moved(3, 2, 10), Some(5));
        assert_eq!(moved(3, -4, 10), None, "before the start");
        assert_eq!(moved(8, 2, 10), None, "past the end");
        assert_eq!(clamped(&(8..20), 10), 8..10);
        assert_eq!(clamped(&(12..20), 10), 10..10);
    }
}

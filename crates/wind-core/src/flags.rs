//! Flagging: which steps of which columns carry which flag. How flags filter
//! the data lives in [`crate::filter`]. After an edit here, rebuild the
//! `FilterView` and call `DayStats::refresh` with the same columns and steps.
use crate::error::{CoreError, CoreResult};
use crate::model::{Dataset, Flag, Segment};
use crate::rules::{self, Evidence};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

/// The flag a new dataset reserves for tower shading: the "reconstruct and
/// average" calculated columns read its values as predictors.
pub const TOWER_SHADOW_FLAG: &str = "Sombra da torre";

/// Flags every new dataset starts with, as (name, CSS colour). All of them
/// exclude data from analyses by default.
pub const DEFAULT_FLAGS: [(&str, &str); 3] = [
    ("Inválido", "#d64545"),
    (TOWER_SHADOW_FLAG, "#8a63d2"),
    ("Gelo", "#2a9fc0"),
];

/// The [`DEFAULT_FLAGS`] as dataset flags.
pub fn default_flags() -> Vec<Flag> {
    DEFAULT_FLAGS
        .iter()
        .map(|(name, color)| Flag { name: name.to_string(), color: color.to_string(), include: false, notes: String::new() })
        .collect()
}

/// Replaces the dataset's flags with `flags`, in order, each with the index
/// it had (`None`: a new flag) ("Definir flags", ESPEC-INTERFACE §5.2).
/// Segments and rule evidence follow their flag to its new index; those of a
/// flag left out go with it. Returns whether a kept flag moved or one was
/// left out: the undo history then speaks of other flags and must restart.
/// O(segments + evidence).
///
/// # Errors
/// [`CoreError::FlagNameInvalid`] for an empty or repeated name;
/// [`CoreError::NoSuchFlag`] for a former index past the flags or given
/// twice. The dataset is untouched then.
pub fn redefine(dataset: &mut Dataset, flags: Vec<(Flag, Option<usize>)>) -> CoreResult<bool> {
    let mut names = HashSet::new();
    let mut new_index: Vec<Option<usize>> = vec![None; dataset.flags.len()];
    for (index, (flag, was)) in flags.iter().enumerate() {
        let name = flag.name.trim();
        if name.is_empty() || !names.insert(name.to_lowercase()) {
            return Err(CoreError::FlagNameInvalid { name: flag.name.clone() });
        }
        if let Some(was) = *was {
            match new_index.get_mut(was) {
                Some(slot @ None) => *slot = Some(index),
                _ => return Err(CoreError::NoSuchFlag { flag: was }),
            }
        }
    }
    let renumbered = new_index.iter().enumerate().any(|(old, new)| *new != Some(old));
    dataset.segments.retain_mut(|segment| new_index[segment.flag].map(|flag| segment.flag = flag).is_some());
    let mut evidence: Vec<Evidence> =
        dataset.evidence.iter().filter_map(|evidence| new_index[evidence.flag].map(|flag| Evidence { flag, ..*evidence })).collect();
    // Both lists are ordered by flag within a column: put them in order again.
    normalize(&mut dataset.segments);
    rules::normalize_evidence(&mut evidence);
    dataset.evidence = evidence.into();
    dataset.flags = flags.into_iter().map(|(flag, _)| Flag { name: flag.name.trim().to_string(), ..flag }).collect();
    Ok(renumbered)
}

/// Flags `steps` of each of `columns` with `flag`; re-flagging the same steps
/// is a no-op, and steps past the record are ignored.
///
/// # Errors
/// [`CoreError::NoSuchFlag`] or [`CoreError::NoSuchColumn`] for a bad index,
/// [`CoreError::DerivedColumnNotFlaggable`] for a derived column.
pub fn flag(dataset: &mut Dataset, columns: &[usize], flag: usize, steps: Range<usize>) -> CoreResult<()> {
    if flag >= dataset.flags.len() {
        return Err(CoreError::NoSuchFlag { flag });
    }
    check_columns(dataset, columns)?;
    let steps = steps.start..steps.end.min(dataset.len);
    if steps.is_empty() {
        return Ok(());
    }
    // Segments only: the steps stay flagged, so their rule evidence stays true.
    remove_segments(dataset, columns, Some(flag), &steps);
    dataset.segments.extend(columns.iter().map(|&column| Segment { column, flag, start: steps.start, end: steps.end }));
    normalize(&mut dataset.segments);
    Ok(())
}

/// Flags `runs` (sorted, disjoint step ranges) of each of `columns` with
/// `flag`, as [`flag`] does each run, in one pass: a scatter selection is
/// thousands of runs. O((segments + runs · columns) · log).
///
/// # Errors
/// As [`flag`].
pub fn flag_runs(dataset: &mut Dataset, columns: &[usize], flag: usize, runs: &[Range<usize>]) -> CoreResult<()> {
    if flag >= dataset.flags.len() {
        return Err(CoreError::NoSuchFlag { flag });
    }
    check_columns(dataset, columns)?;
    let len = dataset.len;
    let clamped = runs.iter().map(|run| run.start..run.end.min(len)).filter(|run| !run.is_empty());
    let added: Vec<Segment> =
        clamped.flat_map(|run| columns.iter().map(move |&column| Segment { column, flag, start: run.start, end: run.end })).collect();
    dataset.segments.extend(added);
    normalize(&mut dataset.segments);
    Ok(())
}

/// Removes flags (all, or only `flag`) from `runs` of each of `columns`, as
/// [`unflag`] does each run.
/// ponytail: O(runs · segments), one pass per run; a merge pass if selections of tens of thousands of runs get slow.
///
/// # Errors
/// As [`unflag`].
pub fn unflag_runs(dataset: &mut Dataset, columns: &[usize], flag: Option<usize>, runs: &[Range<usize>]) -> CoreResult<()> {
    check_columns(dataset, columns)?;
    for run in runs {
        let steps = run.start..run.end.min(dataset.len);
        if !steps.is_empty() {
            remove_segments(dataset, columns, flag, &steps);
            rules::trim_evidence(dataset, columns, flag, &steps);
        }
    }
    Ok(())
}

/// Removes flags (all of them, or only `flag`) from `steps` of each of
/// `columns`, splitting segments that straddle the range. An empty or
/// reversed range does nothing. Splitting keeps segments normalized. Rule
/// evidence is trimmed the same way ([`crate::rules`]).
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for a bad index, so a stale index fails
/// instead of silently removing nothing.
pub fn unflag(dataset: &mut Dataset, columns: &[usize], flag: Option<usize>, steps: Range<usize>) -> CoreResult<()> {
    check_columns(dataset, columns)?;
    let steps = steps.start..steps.end.min(dataset.len);
    if steps.is_empty() {
        return Ok(());
    }
    remove_segments(dataset, columns, flag, &steps);
    rules::trim_evidence(dataset, columns, flag, &steps);
    Ok(())
}

/// The segment part of [`unflag`], on checked columns and a clamped range.
fn remove_segments(dataset: &mut Dataset, columns: &[usize], flag: Option<usize>, steps: &Range<usize>) {
    let mut kept = Vec::with_capacity(dataset.segments.len());
    for segment in std::mem::take(&mut dataset.segments) {
        let overlaps = segment.start < steps.end && steps.start < segment.end;
        let targeted = columns.contains(&segment.column) && flag.map_or(true, |flag| flag == segment.flag);
        if !(overlaps && targeted) {
            kept.push(segment);
            continue;
        }
        if segment.start < steps.start {
            kept.push(Segment { end: steps.start, ..segment });
        }
        if steps.end < segment.end {
            kept.push(Segment { start: steps.end, ..segment });
        }
    }
    dataset.segments = kept;
}

/// Puts segments in their one canonical form: sorted by column, flag and
/// start, with overlapping or touching segments of the same column and flag
/// merged. Equal flagging then means equal segment lists, so re-flagging the
/// same steps changes nothing (and records no undo step), and repeated edits
/// never fragment the list.
pub fn normalize(segments: &mut Vec<Segment>) {
    segments.sort_unstable_by_key(|segment| (segment.column, segment.flag, segment.start));
    segments.dedup_by(|later, kept| {
        let joins = later.column == kept.column && later.flag == kept.flag && later.start <= kept.end;
        if joins {
            kept.end = kept.end.max(later.end);
        }
        joins
    });
}

/// Puts back flags and segments saved with a project, after re-reading the
/// same file with the same configuration (so the grid is the same).
///
/// # Errors
/// [`CoreError::NoSuchColumn`], [`CoreError::NoSuchFlag`] or
/// [`CoreError::SegmentOutOfRange`] for a segment that does not fit the
/// dataset; nothing is changed then. A project file comes from disk, so
/// nothing in it is trusted.
pub fn restore(dataset: &mut Dataset, flags: Vec<Flag>, mut segments: Vec<Segment>) -> CoreResult<()> {
    for segment in &segments {
        check_columns(dataset, &[segment.column])?;
        if segment.flag >= flags.len() {
            return Err(CoreError::NoSuchFlag { flag: segment.flag });
        }
        if segment.start >= segment.end || segment.end > dataset.len {
            return Err(CoreError::SegmentOutOfRange { start: segment.start, end: segment.end, len: dataset.len });
        }
    }
    normalize(&mut segments);
    dataset.flags = flags;
    dataset.segments = segments;
    // Evidence belongs to the replaced segments; `rules::restore_evidence` puts saved evidence back.
    dataset.evidence = Arc::default();
    Ok(())
}

/// Fails with [`CoreError::NoSuchColumn`] for the first index past the
/// columns, or [`CoreError::DerivedColumnNotFlaggable`] for a derived column
/// other than a synthesized one: those inherit their sources' flags instead.
pub(crate) fn check_columns(dataset: &Dataset, columns: &[usize]) -> CoreResult<()> {
    for &column in columns {
        match dataset.columns.get(column) {
            None => return Err(CoreError::NoSuchColumn { column }),
            Some(data) if data.derived.as_ref().is_some_and(|derived| !derived.is_flaggable()) => {
                return Err(CoreError::DerivedColumnNotFlaggable { column: data.id.clone() })
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// After a re-import, keeps the flags and the segments of columns that still
/// exist (matched by id), so remapping a column does not lose QC work.
///
/// Segments follow the file's own timestamps ([`Dataset::stamp_at`]): they
/// stay on the same rows when only the timestamp meaning changed, and on the
/// same times when rows were added or removed or the time step changed. A new
/// step that only partly overlaps a segment is flagged too, so coarsening the
/// step never drops a flag. Rule evidence moves the same way, so it stays
/// inside its segment.
pub fn carry_over(old: &Dataset, new: &mut Dataset) {
    new.flags = old.flags.clone();
    let start_in_new = |old_step: usize| old.stamp_at(old_step) + new.timestamp_meaning.shift(new.step_s);
    let column_in_new = |old_column: usize| {
        let old_id = &old.columns[old_column].id;
        new.columns.iter().position(|candidate| &candidate.id == old_id)
    };
    let steps_in_new = |old_steps: Range<usize>| {
        let start = new.index_of(start_in_new(old_steps.start));
        let end = new.first_step_from(start_in_new(old_steps.end));
        (start < end).then_some(start..end)
    };
    let mut segments: Vec<Segment> = old
        .segments
        .iter()
        .filter_map(|segment| {
            let (column, steps) = (column_in_new(segment.column)?, steps_in_new(segment.steps())?);
            Some(Segment { column, start: steps.start, end: steps.end, ..*segment })
        })
        .collect();
    let mut evidence: Vec<Evidence> = old
        .evidence
        .iter()
        .filter_map(|evidence| {
            let (column, steps) = (column_in_new(evidence.column)?, steps_in_new(evidence.steps())?);
            let statistic_step = new.index_of(start_in_new(evidence.statistic_step)).min(new.len.saturating_sub(1));
            Some(Evidence { column, start: steps.start, end: steps.end, statistic_step, ..*evidence })
        })
        .collect();
    normalize(&mut segments);
    rules::normalize_evidence(&mut evidence);
    new.segments = segments;
    new.evidence = evidence.into();
}

/// How much of each column each flag covers, and which rule evidence
/// explains each segment: the "Inspect and remove flags" view (ESPEC §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagStatistics {
    /// `[flag][column]`: steps of the column's own segments with the flag.
    /// SD / min / max / gust children have no segments of their own: they
    /// follow their mean column.
    pub steps: Vec<Vec<usize>>,
    /// Per segment of `Dataset::segments`, in order: the records of
    /// `Dataset::evidence` inside it (empty for a manual flag).
    pub evidence: Vec<Range<usize>>,
    /// Steps in the record.
    pub step_count: usize,
}

impl FlagStatistics {
    /// Share of the record that `flag` covers on `column`, 0..1 (0 for an
    /// empty record).
    pub fn share(&self, flag: usize, column: usize) -> f64 {
        if self.step_count == 0 {
            0.0
        } else {
            self.steps[flag][column] as f64 / self.step_count as f64
        }
    }
}

/// Counts flagged steps per flag and column and matches each segment with
/// its evidence. O(segments + evidence), one walk over both canonical lists:
/// evidence of a column and flag lies inside its segments, in the same order.
pub fn statistics(dataset: &Dataset) -> FlagStatistics {
    let mut steps = vec![vec![0; dataset.columns.len()]; dataset.flags.len()];
    let mut evidence = Vec::with_capacity(dataset.segments.len());
    let records = &dataset.evidence;
    let group_of = |record: &Evidence| (record.column, record.flag);
    let mut next = 0;
    for segment in &dataset.segments {
        if let Some(count) = steps.get_mut(segment.flag).and_then(|columns| columns.get_mut(segment.column)) {
            *count += segment.end - segment.start;
        }
        // Skip the evidence of earlier segments, then take what starts inside this one.
        let group = (segment.column, segment.flag);
        while records.get(next).is_some_and(|record| (group_of(record), record.end) <= (group, segment.start)) {
            next += 1;
        }
        let first = next;
        while records.get(next).is_some_and(|record| (group_of(record), record.start) < (group, segment.end)) {
            next += 1;
        }
        evidence.push(first..next);
    }
    FlagStatistics { steps, evidence, step_count: dataset.len }
}

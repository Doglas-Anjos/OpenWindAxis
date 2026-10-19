//! Default flag filtering as step ranges. Data under a flag that is not
//! included by default reads as missing in every analysis (exclusion always
//! wins), and a flag on a mean column also covers its SD / min / max / gust
//! children. Analyses walk `values[run]` for each valid run, so filtering
//! never copies a column (ARQUITETURA §4.2). A derived column inherits the
//! exclusions of every column it is computed from ([`crate::derived`]).
//! "Filtrar por" criteria ([`crate::criteria`]) narrow a view further.
use crate::criteria::{self, Criteria};
use crate::error::CoreResult;
use crate::model::Dataset;
use std::ops::Range;

/// Steps each column loses to flag filtering, plus the steps that row
/// criteria take from every column.
///
/// Rebuild it after any change to flags or segments: O(s log s) for `s`
/// segments, independent of the series length; criteria add one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterView {
    /// Per column: sorted, disjoint, non-touching ranges.
    excluded: Vec<Vec<Range<usize>>>,
    /// Steps excluded for every column by row criteria: sorted, disjoint,
    /// non-touching; kept once, not copied into each column.
    common: Vec<Range<usize>>,
}

impl FilterView {
    /// Default flag filtering of every column of `dataset`. A segment that
    /// names an unknown flag excludes its steps: hiding data beats counting
    /// bad data.
    pub fn new(dataset: &Dataset) -> Self {
        let keep_by_default = |flag: usize| dataset.flags.get(flag).is_some_and(|flag| flag.include);
        Self::by_flags(dataset, keep_by_default, true)
    }

    /// Default filtering that also keeps the data under `flag`: tower-shading
    /// detection must still see the shaded sectors after they were flagged
    /// (manual dlg-flag-flag-tower-shading.htm, additional considerations).
    pub fn keeping(dataset: &Dataset, flag: usize) -> Self {
        let keep = |other: usize| other == flag || dataset.flags.get(other).is_some_and(|known| known.include);
        Self::by_flags(dataset, keep, true)
    }

    /// The view of an analysis with "Filtrar por" criteria: their flag
    /// selection instead of default filtering, and their row criteria.
    ///
    /// # Errors
    /// [`Criteria::check`]'s.
    pub fn with_criteria(dataset: &Dataset, criteria: &Criteria) -> CoreResult<Self> {
        criteria.check(dataset)?;
        let mut view = match &criteria.flags {
            None => Self::new(dataset),
            Some(selection) => {
                Self::by_flags(dataset, |flag| selection.keep.get(flag).copied().unwrap_or(false), selection.keep_unflagged)
            }
        };
        view.common = criteria::excluded_steps(dataset, criteria);
        Ok(view)
    }

    /// Flag filtering where data under flag `f` stays when `keep(f)`, and
    /// steps no flag covers stay when `keep_unflagged`; a flag on a mean
    /// column covers its children, and derived columns inherit.
    fn by_flags(dataset: &Dataset, keep: impl Fn(usize) -> bool, keep_unflagged: bool) -> Self {
        let column_count = dataset.columns.len();
        let mut children = vec![Vec::new(); column_count];
        for (child, column) in dataset.columns.iter().enumerate() {
            if let Some(parent) = column.parent.filter(|&parent| parent < column_count) {
                children[parent].push(child);
            }
        }
        let mut excluded = vec![Vec::new(); column_count];
        // Only needed to drop unflagged data: every flagged stretch, kept or not.
        let mut flagged = vec![Vec::new(); if keep_unflagged { 0 } else { column_count }];
        for segment in &dataset.segments {
            let steps = segment.start.min(dataset.len)..segment.end.min(dataset.len);
            if segment.column >= column_count || steps.is_empty() {
                continue;
            }
            if !keep_unflagged {
                for &column in children[segment.column].iter().chain([&segment.column]) {
                    flagged[column].push(steps.clone());
                }
            }
            if keep(segment.flag) {
                continue;
            }
            for &child in &children[segment.column] {
                excluded[child].push(steps.clone());
            }
            excluded[segment.column].push(steps);
        }
        for (column, mut flagged) in flagged.into_iter().enumerate() {
            normalize(&mut flagged);
            excluded[column].extend(complement(&flagged, dataset.len));
        }
        excluded.iter_mut().for_each(normalize);
        // Sources come before the derived column, so in index order each
        // source's list is already complete when it is inherited.
        for (index, column) in dataset.columns.iter().enumerate() {
            let Some(derived) = column.derived.as_ref().filter(|derived| derived.inherits_flags()) else { continue };
            let mut inherited: Vec<Range<usize>> =
                derived.sources().into_iter().filter(|&source| source < index).flat_map(|source| excluded[source].clone()).collect();
            inherited.append(&mut excluded[index]);
            normalize(&mut inherited);
            excluded[index] = inherited;
        }
        FilterView { excluded, common: Vec::new() }
    }

    /// Ranges `column` loses to flag filtering: sorted, disjoint and
    /// non-touching. Row criteria are apart, in [`FilterView::common`].
    pub fn excluded(&self, column: usize) -> &[Range<usize>] {
        &self.excluded[column]
    }

    /// Ranges every column loses to row criteria: sorted, disjoint and
    /// non-touching; empty without criteria.
    pub fn common(&self) -> &[Range<usize>] {
        &self.common
    }

    /// Runs of steps inside `within` where none of `columns` is excluded, in
    /// order. Values in a run can still be NaN (missing in the file).
    ///
    /// O(r log r) for the `r` excluded ranges of `columns`; allocates only the
    /// runs, never anything the size of the series.
    pub fn valid_runs(&self, columns: &[usize], within: Range<usize>) -> Vec<Range<usize>> {
        let mut excluded: Vec<Range<usize>> =
            columns.iter().flat_map(|&column| self.excluded[column].iter().cloned()).chain(self.common.iter().cloned()).collect();
        if columns.len() > 1 || !self.common.is_empty() {
            normalize(&mut excluded);
        }
        let mut runs = Vec::with_capacity(excluded.len() + 1);
        let mut cursor = within.start;
        for gap in excluded {
            if gap.start >= within.end {
                break;
            }
            if gap.start > cursor {
                runs.push(cursor..gap.start);
            }
            cursor = cursor.max(gap.end);
        }
        if cursor < within.end {
            runs.push(cursor..within.end);
        }
        runs
    }

    /// Exclusion test for one column, for steps visited in increasing order.
    pub fn cursor(&self, column: usize) -> Cursor<'_> {
        Cursor { remaining: &self.excluded[column], common: &self.common }
    }
}

/// Answers "is step `step` excluded?" in O(1) amortized; see [`FilterView::cursor`].
pub struct Cursor<'a> {
    /// The column's excluded ranges not yet passed.
    remaining: &'a [Range<usize>],
    /// The row criteria's excluded ranges not yet passed.
    common: &'a [Range<usize>],
}

impl Cursor<'_> {
    /// Whether `step` is excluded. `step` must not decrease between calls.
    pub fn excludes(&mut self, step: usize) -> bool {
        // Both lists advance on every call, so neither falls behind.
        let by_column = passes_into(&mut self.remaining, step);
        let by_criteria = passes_into(&mut self.common, step);
        by_column || by_criteria
    }
}

/// Drops the ranges of `ranges` that end at or before `step` and tells
/// whether the next one holds it.
#[inline]
fn passes_into(ranges: &mut &[Range<usize>], step: usize) -> bool {
    while let Some(gap) = ranges.first() {
        if step < gap.end {
            return gap.start <= step;
        }
        *ranges = &ranges[1..];
    }
    false
}

/// The steps of `0..len` outside `ranges` (sorted, disjoint).
fn complement(ranges: &[Range<usize>], len: usize) -> Vec<Range<usize>> {
    let mut gaps = Vec::with_capacity(ranges.len() + 1);
    let mut cursor = 0;
    for range in ranges {
        if range.start > cursor {
            gaps.push(cursor..range.start);
        }
        cursor = cursor.max(range.end);
    }
    if cursor < len {
        gaps.push(cursor..len);
    }
    gaps
}

/// Which columns' filtered data an edit of the columns `edited` can change:
/// those columns, their SD / min / max / gust children, and every derived
/// column computed from any of them. One pass in index order.
pub fn affected(dataset: &Dataset, edited: &[usize]) -> Vec<bool> {
    let mut affected: Vec<bool> = dataset
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| edited.contains(&index) || column.parent.is_some_and(|parent| edited.contains(&parent)))
        .collect();
    for (index, column) in dataset.columns.iter().enumerate() {
        if let Some(derived) = &column.derived {
            affected[index] |= derived.sources().into_iter().any(|source| source < index && affected[source]);
        }
    }
    affected
}

/// The non-NaN values of `values` inside `runs`, in order.
pub fn valid_values<'a>(values: &'a [f64], runs: &'a [Range<usize>]) -> impl Iterator<Item = f64> + 'a {
    runs.iter()
        .flat_map(move |run| values[run.clone()].iter().copied())
        .filter(|value| !value.is_nan())
}

/// Sorts and merges overlapping or touching ranges, in place.
fn normalize(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    *ranges = merged;
}

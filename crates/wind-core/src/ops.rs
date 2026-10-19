//! Edits of a dataset as values (ARQUITETURA §2.1). Every change goes through
//! an [`Op`], plain data, so undo and redo ([`crate::history`]) and, later,
//! workflows (a saved list of ops) share one mechanism.
use crate::edits::Edit;
use crate::error::{CoreError, CoreResult};
use crate::flags;
use crate::model::Dataset;
use crate::rules::{self, Finding, RuleRun};
use std::ops::Range;

/// One edit of a dataset.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Flag `steps` of `columns` with `flag`; their SD / min / max / gust
    /// children follow.
    Flag {
        /// Column indices.
        columns: Vec<usize>,
        /// Index into `Dataset::flags`.
        flag: usize,
        /// The steps flagged.
        steps: Range<usize>,
    },
    /// Remove flags (all of them, or only `flag`) from `steps` of `columns`.
    Unflag {
        /// Column indices.
        columns: Vec<usize>,
        /// Only this flag; `None` = every flag.
        flag: Option<usize>,
        /// The steps cleared.
        steps: Range<usize>,
    },
    /// Flag `runs` (sorted, disjoint) of `columns` with `flag`: the steps a
    /// scatter selection holds ([`crate::region`]); children follow.
    FlagSteps {
        /// Column indices.
        columns: Vec<usize>,
        /// Index into `Dataset::flags`.
        flag: usize,
        /// The steps flagged, as runs.
        runs: Vec<Range<usize>>,
    },
    /// Remove flags (all, or only `flag`) from `runs` of `columns`.
    UnflagSteps {
        /// Column indices.
        columns: Vec<usize>,
        /// Only this flag; `None` = every flag.
        flag: Option<usize>,
        /// The steps cleared, as runs.
        runs: Vec<Range<usize>>,
    },
    /// Flag what the run's rules find, keeping each stretch's evidence
    /// ([`crate::rules`]); SD / min / max / gust children follow.
    FlagByRules(RuleRun),
    /// Change values: scale and offset, delete, shift in time
    /// ([`crate::edits`]).
    Edit(Edit),
}

/// What applying an op found besides the edit itself ("Op com relatório",
/// ARQUITETURA B4).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpReport {
    /// For [`Op::FlagByRules`], every stretch the rules found, already
    /// flagged ones included; empty for other ops.
    pub findings: Vec<Finding>,
}

impl Op {
    /// Applies the op to `dataset` and reports what it found. Leaves
    /// `dataset` untouched on error.
    ///
    /// # Errors
    /// The op's own validation, e.g. [`crate::error::CoreError::NoSuchColumn`].
    pub fn apply(&self, dataset: &mut Dataset) -> CoreResult<OpReport> {
        let report = self.apply_alone(dataset)?;
        // Calculated columns read their sources filtered, so flags change them too.
        crate::calculated::refresh(dataset);
        Ok(report)
    }

    /// [`Op::apply`] without refreshing the calculated columns.
    fn apply_alone(&self, dataset: &mut Dataset) -> CoreResult<OpReport> {
        match self {
            Op::Flag { columns, flag, steps } => {
                flags::flag(dataset, columns, *flag, steps.clone()).map(|()| OpReport::default())
            }
            Op::Unflag { columns, flag, steps } => {
                flags::unflag(dataset, columns, *flag, steps.clone()).map(|()| OpReport::default())
            }
            Op::FlagSteps { columns, flag, runs } => flags::flag_runs(dataset, columns, *flag, runs).map(|()| OpReport::default()),
            Op::UnflagSteps { columns, flag, runs } => {
                flags::unflag_runs(dataset, columns, *flag, runs).map(|()| OpReport::default())
            }
            Op::FlagByRules(run) => rules::apply(dataset, run).map(|findings| OpReport { findings }),
            Op::Edit(edit) => edit.apply(dataset).map(|()| OpReport::default()),
        }
    }

    /// The [`Op::Unflag`] that removes `flag` from `columns` (every column
    /// when `None`) over the whole record ("Remover" in ESPEC §5.3). It
    /// reaches only the columns that carry the flag, between their first and
    /// last flagged steps, so refreshing summaries costs only what changes.
    /// O(segments).
    ///
    /// # Errors
    /// [`CoreError::NoSuchFlag`] for a flag the dataset lacks.
    pub fn remove_flag(dataset: &Dataset, flag: usize, columns: Option<&[usize]>) -> CoreResult<Op> {
        if flag >= dataset.flags.len() {
            return Err(CoreError::NoSuchFlag { flag });
        }
        let targeted = dataset
            .segments
            .iter()
            .filter(|segment| segment.flag == flag && columns.map_or(true, |columns| columns.contains(&segment.column)));
        let mut reached: Vec<usize> = targeted.clone().map(|segment| segment.column).collect();
        reached.sort_unstable();
        reached.dedup();
        let first = targeted.clone().map(|segment| segment.start).min().unwrap_or(0);
        let end = targeted.map(|segment| segment.end).max().unwrap_or(0);
        Ok(Op::Unflag { columns: reached, flag: Some(flag), steps: first..end })
    }

    /// The columns and steps whose filtered data the op can change: what
    /// `DayStats::refresh` needs after applying, undoing or redoing it,
    /// unless [`Op::moves_time`].
    pub fn reach(&self) -> (&[usize], Range<usize>) {
        match self {
            Op::Flag { columns, steps, .. } | Op::Unflag { columns, steps, .. } => (columns, steps.clone()),
            // The runs are sorted: the first starts and the last ends what can change.
            Op::FlagSteps { columns, runs, .. } | Op::UnflagSteps { columns, runs, .. } => {
                (columns, runs.first().map_or(0, |run| run.start)..runs.last().map_or(0, |run| run.end))
            }
            Op::FlagByRules(run) => (run.columns(), run.steps()),
            Op::Edit(edit) => edit.reach(),
        }
    }

    /// Whether the op changes values, not only flags.
    pub fn changes_values(&self) -> bool {
        matches!(self, Op::Edit(_))
    }

    /// Whether the op moves the record in time, so per-day summaries must be
    /// built again (`DayStats::new`) rather than refreshed.
    pub fn moves_time(&self) -> bool {
        matches!(self, Op::Edit(edit) if edit.moves_time())
    }
}

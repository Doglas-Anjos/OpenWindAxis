//! Undo and redo of [`Op`]s. Each revision keeps the state its op replaced,
//! so undo and redo are exact swaps that cost only that state, never a replay
//! of the record.
//!
//! A revision keeps the segment list, the rule evidence, the start of the
//! record, the log of value edits and every column's values. Evidence and
//! columns are `Arc`s: a revision shares, not copies, whatever its op left
//! alone, so a flag edit holds only segments, and a value edit only the
//! columns it replaced (ARQUITETURA §2.3). Memory stays bounded by a count
//! of revisions and by a budget for replaced columns (ARQUITETURA P10).
use crate::edits::Edit;
use crate::error::CoreResult;
use crate::model::{Dataset, Segment};
use crate::ops::{Op, OpReport};
use crate::rules::{self, Evidence};
use std::collections::VecDeque;
use std::mem::{size_of, swap};
use std::sync::Arc;

/// Revisions kept for undo; older ones are dropped.
pub const MAX_REVISIONS: usize = 100;
/// Bytes of replaced column values the undo history may hold: 256 MiB, a
/// tenth of a 32-bit WASM memory, or about 32 edits of one column of a
/// million steps. Older revisions are dropped past it; the newest is always
/// kept, so the last edit can be undone.
pub const MAX_HISTORY_BYTES: usize = 256 << 20;

/// An applied (or undone) op and the state on the other side of it.
#[derive(Debug, Clone)]
struct Revision {
    op: Op,
    segments: Vec<Segment>,
    evidence: Arc<[Evidence]>,
    start: i64,
    edits: Vec<Edit>,
    /// Every column's values, in column order.
    values: Vec<Arc<[f64]>>,
    /// Bytes of `values` this revision alone holds: the columns its op replaced.
    bytes: usize,
}

impl Revision {
    /// The state of `dataset` before `op`.
    fn before(op: Op, dataset: &Dataset) -> Self {
        Revision {
            op,
            segments: dataset.segments.clone(),
            evidence: Arc::clone(&dataset.evidence),
            start: dataset.start,
            edits: dataset.edits.clone(),
            values: dataset.columns.iter().map(|column| Arc::clone(&column.values)).collect(),
            bytes: 0,
        }
    }

    /// Swaps the kept state with `dataset`'s: undo and redo alike.
    fn swap_with(&mut self, dataset: &mut Dataset) {
        swap(&mut dataset.segments, &mut self.segments);
        swap(&mut dataset.evidence, &mut self.evidence);
        swap(&mut dataset.start, &mut self.start);
        swap(&mut dataset.edits, &mut self.edits);
        for (column, values) in dataset.columns.iter_mut().zip(&mut self.values) {
            swap(&mut column.values, values);
        }
    }

    /// Whether `dataset` still holds exactly the state kept here.
    fn unchanged_in(&self, dataset: &Dataset) -> bool {
        dataset.segments == self.segments
            && rules::same_evidence(&dataset.evidence, &self.evidence)
            && dataset.start == self.start
            && dataset.edits.len() == self.edits.len()
            && dataset.columns.iter().zip(&self.values).all(|(column, values)| Arc::ptr_eq(&column.values, values))
    }
}

/// The undo and redo stacks of one dataset.
#[derive(Debug, Clone, Default)]
pub struct History {
    /// Oldest first; undo takes from the back.
    done: VecDeque<Revision>,
    /// Most recently undone last.
    undone: Vec<Revision>,
    /// Sum of the revisions' `bytes`, on both stacks.
    bytes: usize,
}

impl History {
    /// Applies `op` to `dataset`, records it and returns its report. An op
    /// that changes nothing is not recorded, and any new edit clears the redo
    /// stack.
    ///
    /// # Errors
    /// The op's own validation; nothing is applied or recorded then.
    pub fn apply(&mut self, dataset: &mut Dataset, op: Op) -> CoreResult<OpReport> {
        let mut revision = Revision::before(op, dataset);
        let report = revision.op.apply(dataset)?;
        if revision.unchanged_in(dataset) {
            return Ok(report);
        }
        revision.bytes = dataset
            .columns
            .iter()
            .zip(&revision.values)
            .filter(|(column, values)| !Arc::ptr_eq(&column.values, values))
            .map(|(_, values)| values.len() * size_of::<f64>())
            .sum();
        self.bytes += revision.bytes;
        self.done.push_back(revision);
        for dropped in self.undone.drain(..) {
            self.bytes -= dropped.bytes;
        }
        while self.done.len() > MAX_REVISIONS || (self.bytes > MAX_HISTORY_BYTES && self.done.len() > 1) {
            let dropped = self.done.pop_front().expect("the loop runs only on a non-empty stack");
            self.bytes -= dropped.bytes;
        }
        Ok(report)
    }

    /// Undoes the last recorded op and returns it, or `None` when there is
    /// nothing to undo.
    pub fn undo(&mut self, dataset: &mut Dataset) -> Option<Op> {
        let mut revision = self.done.pop_back()?;
        revision.swap_with(dataset);
        // Flag definitions may have changed since: calculated columns follow the current ones.
        crate::calculated::refresh(dataset);
        let op = revision.op.clone();
        self.undone.push(revision);
        Some(op)
    }

    /// Redoes the last undone op and returns it, or `None` when there is
    /// nothing to redo.
    pub fn redo(&mut self, dataset: &mut Dataset) -> Option<Op> {
        let mut revision = self.undone.pop()?;
        revision.swap_with(dataset);
        crate::calculated::refresh(dataset);
        let op = revision.op.clone();
        self.done.push_back(revision);
        Some(op)
    }

    /// Whether [`History::undo`] has something to undo.
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    /// Whether [`History::redo`] has something to redo.
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }
}

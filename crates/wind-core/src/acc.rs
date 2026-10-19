//! Mergeable accumulators and per-day blocks (ARQUITETURA §4.3). A summary is
//! kept per calendar day, and any span of days (a month, the record, a date
//! filter) is the in-order merge of its days. An edit then recomputes only the
//! days it touches, and since folds always run in day order, results are
//! bit-for-bit the same however the work was split (P11).
use std::ops::Range;
use std::sync::OnceLock;

/// A summary that can absorb the summary of the samples that follow it.
/// `merge` must be associative up to rounding.
pub trait Acc: Clone {
    /// Absorb `later`, the summary of the samples right after these.
    fn merge(&mut self, later: &Self);
}

/// Neumaier-compensated sum: rounding error stays O(ε) instead of growing
/// with the number of terms (P12).
///
/// ```
/// use wind_core::acc::Sum;
/// let mut sum = Sum::default();
/// for term in [1e16, 1.0, -1e16] {
///     sum.add(term);
/// }
/// assert_eq!(sum.value(), 1.0); // a plain f64 sum gives 0.0
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sum {
    /// Running total.
    total: f64,
    /// Rounding error lost from `total` so far.
    compensation: f64,
}

impl Sum {
    /// The empty sum.
    pub const ZERO: Sum = Sum { total: 0.0, compensation: 0.0 };

    /// Add one term.
    #[inline]
    pub fn add(&mut self, term: f64) {
        let new_total = self.total + term;
        self.compensation += if self.total.abs() >= term.abs() {
            (self.total - new_total) + term
        } else {
            (term - new_total) + self.total
        };
        self.total = new_total;
    }

    /// The compensated total.
    pub fn value(&self) -> f64 {
        self.total + self.compensation
    }
}

impl Acc for Sum {
    fn merge(&mut self, later: &Self) {
        self.add(later.total);
        self.compensation += later.compensation;
    }
}

/// Running mean; angles (degrees) average as unit vectors.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeanAcc {
    /// Samples absorbed.
    pub count: usize,
    /// Sum of the samples (plain values).
    sum: Sum,
    /// Sum of cosines (angles).
    sum_cos: Sum,
    /// Sum of sines (angles).
    sum_sin: Sum,
}

impl MeanAcc {
    /// No samples yet.
    pub const EMPTY: MeanAcc = MeanAcc { count: 0, sum: Sum::ZERO, sum_cos: Sum::ZERO, sum_sin: Sum::ZERO };

    /// Add one sample; `angle` must be the same for every call.
    #[inline]
    pub fn add(&mut self, value: f64, angle: bool) {
        self.count += 1;
        if angle {
            let (sin, cos) = sincos_deg(value);
            self.sum_cos.add(cos);
            self.sum_sin.add(sin);
        } else {
            self.sum.add(value);
        }
    }

    /// Arithmetic mean, or vector mean in `0..360` for angles; NaN when empty.
    pub fn mean(&self, angle: bool) -> f64 {
        if self.count == 0 {
            f64::NAN
        } else if angle {
            libm::atan2(self.sum_sin.value(), self.sum_cos.value())
                .to_degrees()
                .rem_euclid(DEGREES_PER_TURN)
        } else {
            self.sum.value() / self.count as f64
        }
    }
}

impl Acc for MeanAcc {
    fn merge(&mut self, later: &Self) {
        self.count += later.count;
        self.sum.merge(&later.sum);
        self.sum_cos.merge(&later.sum_cos);
        self.sum_sin.merge(&later.sum_sin);
    }
}

/// Degrees in a full turn.
pub const DEGREES_PER_TURN: f64 = 360.0;
/// Resolution the angle table covers: loggers write directions to 0.1° or coarser.
const TABLE_STEPS_PER_DEGREE: f64 = 10.0;
/// Entries in the angle table: every tenth of a degree in `0..360`.
const TABLE_SIZE: usize = 3_600;

/// `(sin, cos)` of an angle in degrees. Whole tenths in `0..360` come from a
/// table built with the same `libm::sincos` call, so a hit is bit-identical to
/// computing it, ~10× cheaper.
pub fn sincos_deg(degrees: f64) -> (f64, f64) {
    static TABLE: OnceLock<Vec<(f64, f64)>> = OnceLock::new();
    let tenths = (degrees * TABLE_STEPS_PER_DEGREE).round();
    if (0.0..TABLE_SIZE as f64).contains(&tenths) && tenths / TABLE_STEPS_PER_DEGREE == degrees {
        let table = TABLE.get_or_init(|| {
            (0..TABLE_SIZE)
                .map(|entry| libm::sincos((entry as f64 / TABLE_STEPS_PER_DEGREE).to_radians()))
                .collect()
        });
        return table[tenths as usize];
    }
    libm::sincos(degrees.to_radians())
}

/// Calls `visit(block, step)` for every step that lies both in
/// `blocks[block]` and in one of `runs`. Both lists must be sorted and
/// disjoint; blocks may be empty. Cost O(steps visited + blocks + runs).
#[inline]
pub fn each_step(blocks: &[Range<usize>], runs: &[Range<usize>], mut visit: impl FnMut(usize, usize)) {
    let mut first_run = 0;
    for (block, block_steps) in blocks.iter().enumerate() {
        while first_run < runs.len() && runs[first_run].end <= block_steps.start {
            first_run += 1;
        }
        let mut run = first_run;
        while run < runs.len() && runs[run].start < block_steps.end {
            for step in runs[run].start.max(block_steps.start)..runs[run].end.min(block_steps.end) {
                visit(block, step);
            }
            run += 1;
        }
    }
}

/// One accumulator per block: `accs[block]` is reset with `init` and then
/// absorbs, through `push`, every step of `blocks[block]` inside `runs`.
/// `accs` and `blocks` must have the same length.
pub fn fill<A>(accs: &mut [A], blocks: &[Range<usize>], runs: &[Range<usize>], init: impl Fn() -> A, mut push: impl FnMut(&mut A, usize)) {
    debug_assert_eq!(accs.len(), blocks.len());
    accs.iter_mut().for_each(|acc| *acc = init());
    each_step(blocks, runs, |block, step| push(&mut accs[block], step));
}

/// In-order merge of `accs`; `None` when there are none.
pub fn fold<A: Acc>(accs: &[A]) -> Option<A> {
    let (first, rest) = accs.split_first()?;
    let mut total = first.clone();
    rest.iter().for_each(|acc| total.merge(acc));
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hundredths of a degree over a full turn: every tenth hits the table.
    const HUNDREDTHS_PER_DEGREE: f64 = 100.0;
    const HUNDREDTHS_PER_TURN: usize = 36_000;

    #[test]
    fn sincos_table_is_bit_identical_to_libm() {
        let outside_table = [-10.0, 360.0, 725.3, f64::MAX];
        let angles = (0..HUNDREDTHS_PER_TURN).map(|hundredths| hundredths as f64 / HUNDREDTHS_PER_DEGREE).chain(outside_table);
        for degrees in angles {
            let (sin, cos) = sincos_deg(degrees);
            let (want_sin, want_cos) = libm::sincos(degrees.to_radians());
            assert_eq!((sin.to_bits(), cos.to_bits()), (want_sin.to_bits(), want_cos.to_bits()), "{degrees}");
        }
    }
}

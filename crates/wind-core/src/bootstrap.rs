//! Confidence intervals by day-block bootstrap (ARQUITETURA B1 and §4.3).
//!
//! Ten-minute wind data is strongly autocorrelated: a gusty afternoon is many
//! steps that are not independent. Resampling single steps would treat them
//! as independent and give intervals far too narrow, so the blocks resampled
//! are whole days (a moving-block bootstrap with day blocks; Künsch 1989,
//! "The jackknife and the bootstrap for general stationary observations",
//! Annals of Statistics 17(3)). Each replicate draws as many days, with
//! replacement, as the record has days with data, merges their per-day
//! summaries and recomputes the statistic; the interval is the 2.5 % and
//! 97.5 % percentiles of the replicates (Efron's percentile interval).
//!
//! Deterministic: a fixed seed and [`SplitMix64`], so the same data always
//! gives the same interval, bit for bit, on every platform.
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time;
use crate::weibull::{self, Method};
use serde::Serialize;

/// Replicates per interval. 200 is the usual floor for a 95 % percentile
/// interval (Efron & Tibshirani 1993, §13.3); more narrows the Monte Carlo
/// error of its ends at a proportional cost.
pub const RESAMPLES: usize = 200;
/// Coverage of the intervals.
pub const CONFIDENCE: f64 = 0.95;
/// Seed of every bootstrap: any fixed value makes results reproducible.
const SEED: u64 = 0x0057_1D0B_007A_2026;
/// A replicate statistic that fails (no solution, too few speeds) is
/// dropped; with fewer valid replicates than this share, no interval.
const MIN_VALID_SHARE: f64 = 0.9;
/// Classes per m/s of the per-day histograms (0.01 m/s each), as the fit
/// groups speeds ([`weibull::fit`]): loggers write two decimals, so each
/// class holds one value and the binned fit is the exact fit.
const CLASSES_PER_M_S: f64 = weibull::CLASSES_PER_UNIT;
/// Most speed classes a histogram may have (1 000 m/s at 0.01 m/s): past
/// that the data holds an outlier and the interval is skipped.
const MAX_SPEED_CLASSES: f64 = 100_000.0;

/// SplitMix64 (Steele, Lea & Flood 2014, "Fast splittable pseudorandom
/// number generators", OOPSLA; constants of Vigna's reference splitmix64.c).
pub struct SplitMix64(u64);

impl SplitMix64 {
    /// A generator that starts from `seed`.
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A value in `0..bound` (`bound > 0`), by Lemire's multiply-shift
    /// ("Fast random integer generation in an interval", 2019), whose bias
    /// is below 2⁻³² for any bound here.
    pub fn below(&mut self, bound: usize) -> usize {
        ((u128::from(self.next_u64()) * bound as u128) >> 64) as usize
    }
}

/// A two-sided confidence interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Interval {
    /// Lower end.
    pub low: f64,
    /// Upper end.
    pub high: f64,
}

/// Intervals of a Weibull fit's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WeibullInterval {
    /// Shape k.
    pub k: Interval,
    /// Scale A.
    pub a: Interval,
}

/// The [`CONFIDENCE`] percentile interval of `replicates` (NaN ones are
/// failed replicates), or `None` when too many failed.
pub fn percentile_interval(replicates: &[f64]) -> Option<Interval> {
    let mut valid: Vec<f64> = replicates.iter().copied().filter(|value| value.is_finite()).collect();
    if (valid.len() as f64) < MIN_VALID_SHARE * replicates.len() as f64 || valid.is_empty() {
        return None;
    }
    valid.sort_unstable_by(f64::total_cmp);
    let tail = (1.0 - CONFIDENCE) / 2.0;
    let at = |share: f64| valid[((share * (valid.len() - 1) as f64).round() as usize).min(valid.len() - 1)];
    Some(Interval { low: at(tail), high: at(1.0 - tail) })
}

/// For each replicate, how many times each of `blocks` days is drawn.
/// Calls `replicate` with those counts, `RESAMPLES` times, in order.
pub fn each_replicate(blocks: usize, mut replicate: impl FnMut(&[u32])) {
    let mut random = SplitMix64::new(SEED);
    let mut draws = vec![0u32; blocks];
    for _ in 0..RESAMPLES {
        draws.iter_mut().for_each(|count| *count = 0);
        for _ in 0..blocks {
            draws[random.below(blocks)] += 1;
        }
        replicate(&draws);
    }
}

/// The interval of the Weibull fit of `column` under `view` by `method`;
/// `None` with fewer than two days of data, an outlier past the histogram's
/// range, or too many replicates without a fit.
///
/// Cost: one pass to build per-day histograms (sparse: one entry per
/// distinct speed of a day), then per replicate a merge of the drawn days
/// (O(steps)) and a fit over the distinct speeds (O(classes)). About half a
/// second natively for a million steps.
pub fn weibull_interval(dataset: &Dataset, view: &FilterView, column: usize, method: Method) -> Option<WeibullInterval> {
    let values = &dataset.columns[column].values;
    let days = time::days(dataset);
    // Per day with data: its (class, count) entries, flattened.
    let mut entries: Vec<(u32, u32)> = Vec::new();
    let mut day_ends: Vec<usize> = Vec::new();
    let mut top_class = 0u32;
    let mut today: Vec<u32> = Vec::new();
    for day in &days {
        today.clear();
        for run in view.valid_runs(&[column], day.clone()) {
            for &speed in &values[run] {
                if !(speed >= 0.0) {
                    continue;
                }
                let class = (speed * CLASSES_PER_M_S).round();
                if !(class < MAX_SPEED_CLASSES) {
                    return None;
                }
                today.push(class as u32);
            }
        }
        if today.is_empty() {
            continue;
        }
        today.sort_unstable();
        // Entries from here on are today's; never merge into yesterday's last.
        let today_start = entries.len();
        for &class in &today {
            match entries[today_start..].last_mut() {
                Some((last, count)) if *last == class => *count += 1,
                _ => entries.push((class, 1)),
            }
            top_class = top_class.max(class);
        }
        day_ends.push(entries.len());
    }
    if day_ends.len() < 2 {
        return None;
    }
    let mut counts = vec![0u64; top_class as usize + 1];
    let mut samples: Vec<(f64, f64)> = Vec::new();
    let (mut shapes, mut scales) = (Vec::with_capacity(RESAMPLES), Vec::with_capacity(RESAMPLES));
    each_replicate(day_ends.len(), |draws| {
        counts.iter_mut().for_each(|count| *count = 0);
        let mut start = 0;
        for (day, &end) in day_ends.iter().enumerate() {
            let times = u64::from(draws[day]);
            if times > 0 {
                for &(class, count) in &entries[start..end] {
                    counts[class as usize] += times * u64::from(count);
                }
            }
            start = end;
        }
        samples.clear();
        samples.extend(
            counts.iter().enumerate().filter(|(_, &count)| count > 0).map(|(class, &count)| (class as f64 / CLASSES_PER_M_S, count as f64)),
        );
        let (shape, scale) = weibull::fit_weighted(&samples, method).map_or((f64::NAN, f64::NAN), |fit| (fit.k, fit.a));
        shapes.push(shape);
        scales.push(scale);
    });
    Some(WeibullInterval { k: percentile_interval(&shapes)?, a: percentile_interval(&scales)? })
}

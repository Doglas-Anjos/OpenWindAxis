//! Weibull fits of wind-speed samples, the four published methods:
//! maximum likelihood (Stevens & Smulders 1979), least squares on the
//! linearized CDF, WAsP (matches the mean cubed speed and the share of samples
//! above the mean speed) and Openwind (matches the mean and mean cubed speed).
//!
//! Maximum likelihood and least squares follow the original program, as its
//! manual describes them and as its results show (ARQUITETURA
//! 2026-10-17):
//! - **Maximum likelihood** iterates the Stevens & Smulders equation from
//!   k = 2 until k changes less than 0.1 %, k at most 10. Speeds below the
//!   cutoff, the 0.5th percentile of the record with zeros included but
//!   never below the smallest positive speed, have no usable logarithm: they
//!   are fitted as censored. The speeds above are fitted, the low ones are
//!   replaced by draws from that fit below the cutoff, the whole is fitted
//!   again, and so on until k changes less than 0.1 % (20 rounds at most).
//!   The draws come from the original's generator and seed, so they repeat.
//!   With many zeros (calms, or a stalled anemometer) this gives a much
//!   lower k than leaving the zeros out.
//! - **Least squares** puts every speed on the line, at its plotting
//!   position (i + ½)/n among all n speeds sorted, held within [10⁻⁶,
//!   1 − 10⁻⁶]; zero speeds have no logarithm and stay off the line but keep
//!   their ranks.
use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// How to fit the distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Maximum likelihood (Stevens & Smulders 1979).
    #[default]
    MaxLikelihood,
    /// Straight line through the linearized empirical CDF.
    LeastSquares,
    /// Matches the mean cubed speed and the share of samples above the mean.
    Wasp,
    /// Matches the mean speed and the mean cubed speed.
    Openwind,
}

/// A two-parameter Weibull distribution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Weibull {
    /// Shape parameter k (dimensionless).
    pub k: f64,
    /// Scale parameter A (a.k.a. c), same units as the speeds.
    pub a: f64,
}

impl Weibull {
    /// Mean of the distribution, A·Γ(1 + 1/k).
    pub fn mean(&self) -> f64 {
        self.a * libm::tgamma(1.0 + 1.0 / self.k)
    }

    /// Cumulative probability of a speed at or below `speed`.
    pub fn cdf(&self, speed: f64) -> f64 {
        if speed <= 0.0 {
            0.0
        } else {
            1.0 - libm::exp(-libm::pow(speed / self.a, self.k))
        }
    }
}

/// Shapes searched by the moment-matching fits (WAsP, Openwind): wide enough
/// for any wind regime, narrow enough for Γ to stay finite.
const MOMENT_FIT_SHAPE_RANGE: (f64, f64) = (0.3, 20.0);
/// Bisection steps: halves the shape bracket to far below f64 precision.
const BISECTION_STEPS: usize = 100;
/// Where the Stevens & Smulders iteration starts: Rayleigh.
const FIRST_SHAPE: f64 = 2.0;
/// Largest k the iteration returns.
const MAX_SHAPE: f64 = 10.0;
/// Iterations allowed; convergence takes about ten.
const MAX_FIXED_POINT_STEPS: usize = 100;
/// Relative change in k that ends the iteration and the rounds of draws.
const SHAPE_TOLERANCE: f64 = 1e-3;
/// Spread of ln u below which all speeds count as equal.
const MIN_LOG_SPREAD: f64 = 1e-12;
/// Percent of the record below the cutoff of the likelihood fit.
const LOW_SPEED_PERCENT: f64 = 0.5;
/// Points of the quantile curve the cutoff is read from, as the original samples it.
const CUTOFF_CURVE_POINTS: usize = 200;
/// Gap in percent below which two curve points count as one.
const CURVE_GAP: f64 = 1e-5;
/// Rounds of drawing the low speeds and fitting again, at most.
const MAX_DRAW_ROUNDS: usize = 20;
/// Seed of the generator that draws the low speeds: the original's.
const DRAW_SEED: i32 = -4_723;
/// Plotting positions of the least-squares line stay within these, so that
/// ln(−ln(1 − F)) stays finite.
const PLOTTING_POSITION_RANGE: (f64, f64) = (1e-6, 1.0 - 1e-6);

/// Classes per unit when grouping speeds of two decimals, as loggers write
/// them: `class / 100` is the same double the import parsed from "8.37".
pub(crate) const CLASSES_PER_UNIT: f64 = 100.0;
/// Most classes grouping allocates (1 000 m/s at 0.01): past that, an outlier,
/// and the raw values are fitted.
const MAX_GROUPED_CLASSES: f64 = 100_000.0;

/// Fits the speeds `speeds`; NaN and negative values are skipped.
///
/// Speeds of at most two decimals (logger files) are grouped first: a year
/// of 10-minute data has some 2 500 distinct values, and the likelihood fit
/// costs an exponential per value and per iteration, so grouping makes it
/// ~30× faster on a million steps. Each group's value is the exact double of
/// its speeds, so the fit is the same up to the order of the sums.
///
/// # Errors
/// [`CoreError::TooFewSpeeds`], [`CoreError::TooFewDistinctSpeeds`], or a
/// method-specific failure (no root, outliers too large, flat CDF).
pub fn fit(speeds: &[f64], method: Method) -> CoreResult<Weibull> {
    if let Some(grouped) = group_two_decimals(speeds) {
        return fit_weighted(&grouped, method);
    }
    let samples: Vec<(f64, f64)> = speeds.iter().filter(|speed| **speed >= 0.0).map(|&speed| (speed, 1.0)).collect();
    fit_weighted(&samples, method)
}

/// `(speed, count)` per distinct speed when every speed kept (≥ 0) has at
/// most two decimals; `None` at the first one that has more, or past
/// [`MAX_GROUPED_CLASSES`]. O(n + classes).
fn group_two_decimals(speeds: &[f64]) -> Option<Vec<(f64, f64)>> {
    let mut counts: Vec<u64> = Vec::new();
    for &speed in speeds {
        if !(speed >= 0.0) {
            continue;
        }
        let class = (speed * CLASSES_PER_UNIT).round();
        if !(class < MAX_GROUPED_CLASSES) || class / CLASSES_PER_UNIT != speed {
            return None;
        }
        let class = class as usize;
        if class >= counts.len() {
            counts.resize(class + 1, 0);
        }
        counts[class] += 1;
    }
    Some(
        counts
            .iter()
            .enumerate()
            .filter(|(_, &count)| count > 0)
            .map(|(class, &count)| (class as f64 / CLASSES_PER_UNIT, count as f64))
            .collect(),
    )
}

/// Fits `(speed, weight)` pairs, each speed counted `weight` times: a
/// histogram of speeds fits like the speeds themselves (the bootstrap fits
/// per-day histograms this way, [`crate::bootstrap`]). With weights of 1 the
/// result is bit for bit that of [`fit`]. NaN and negative speeds are skipped.
///
/// # Errors
/// As [`fit`]; [`CoreError::TooFewSpeeds`] below a total weight of 2.
pub fn fit_weighted(samples: &[(f64, f64)], method: Method) -> CoreResult<Weibull> {
    let filtered;
    let samples = if samples.iter().all(|&(speed, weight)| speed >= 0.0 && weight > 0.0) {
        samples
    } else {
        filtered = samples.iter().copied().filter(|&(speed, weight)| speed >= 0.0 && weight > 0.0).collect::<Vec<_>>();
        &filtered[..]
    };
    let count: f64 = samples.iter().map(|&(_, weight)| weight).sum();
    if !(count >= 2.0) {
        return Err(CoreError::TooFewSpeeds);
    }
    match method {
        Method::MaxLikelihood => max_likelihood(samples),
        Method::LeastSquares => least_squares(samples, count),
        Method::Wasp | Method::Openwind => {
            let mean = samples.iter().map(|&(speed, weight)| weight * speed).sum::<f64>() / count;
            let mean_cube = samples.iter().map(|&(speed, weight)| weight * (speed * speed * speed)).sum::<f64>() / count;
            let (low, high) = MOMENT_FIT_SHAPE_RANGE;
            let no_solution = CoreError::WeibullNoSolution { method };
            if method == Method::Wasp {
                let above_mean: f64 = samples.iter().filter(|&&(speed, _)| speed > mean).map(|&(_, weight)| weight).sum();
                let share_above_mean = above_mean / count;
                let scale_for = |shape: f64| libm::cbrt(mean_cube / libm::tgamma(1.0 + 3.0 / shape));
                let share_mismatch = |shape: f64| libm::exp(-libm::pow(mean / scale_for(shape), shape)) - share_above_mean;
                let shape = bisect(share_mismatch, low, high).ok_or(no_solution)?;
                Ok(Weibull { k: shape, a: scale_for(shape) })
            } else {
                let energy_pattern_factor = mean_cube / (mean * mean * mean);
                let gamma_1 = |shape: f64| libm::tgamma(1.0 + 1.0 / shape);
                let epf_mismatch =
                    |shape: f64| libm::tgamma(1.0 + 3.0 / shape) / libm::pow(gamma_1(shape), 3.0) - energy_pattern_factor;
                let shape = bisect(epf_mismatch, low, high).ok_or(no_solution)?;
                Ok(Weibull { k: shape, a: mean / gamma_1(shape) })
            }
        }
    }
}

/// `samples` in increasing speed: borrowed when they already are (grouped
/// speeds are), else a sorted copy. O(n), or O(n log n) to sort.
fn by_speed(samples: &[(f64, f64)]) -> Cow<'_, [(f64, f64)]> {
    if samples.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
        Cow::Borrowed(samples)
    } else {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable_by(|left, right| left.0.total_cmp(&right.0));
        Cow::Owned(sorted)
    }
}

/// Copies of a sample: weights are counts.
fn copies(weight: f64) -> usize {
    weight.round() as usize
}

/// Maximum likelihood with the low speeds fitted as censored (module docs).
/// `samples` need not be sorted. O(n) per iteration; a few rounds of a few
/// iterations each.
fn max_likelihood(samples: &[(f64, f64)]) -> CoreResult<Weibull> {
    let sorted = by_speed(samples);
    let cutoff = low_speed_cutoff(&sorted).ok_or(CoreError::TooFewDistinctSpeeds)?;
    let (low, high) = sorted.split_at(sorted.partition_point(|&(speed, _)| speed < cutoff));
    let low_count: usize = low.iter().map(|&(_, weight)| copies(weight)).sum();
    let mut fit = stevens_smulders(high)?;
    if low_count == 0 {
        return Ok(fit);
    }
    let mut generator = ParkMiller::new(DRAW_SEED);
    let mut combined = high.to_vec();
    for _ in 0..MAX_DRAW_ROUNDS {
        combined.truncate(high.len());
        // A draw r ∈ (0, 1) from F restricted to [0, cutoff): F⁻¹(r · F(cutoff)).
        let below_cutoff = fit.cdf(cutoff);
        let draw = |r: f64| fit.a * libm::pow(-libm::log(1.0 - r * below_cutoff), 1.0 / fit.k);
        combined.extend((0..low_count).map(|_| (draw(generator.next()), 1.0)));
        let next = stevens_smulders(&combined)?;
        let change = (next.k - fit.k).abs() / fit.k;
        fit = next;
        if change < SHAPE_TOLERANCE {
            break;
        }
    }
    Ok(fit)
}

/// The cutoff of the likelihood fit, read as the original reads it: the
/// speed at [`LOW_SPEED_PERCENT`] on a curve of [`CUTOFF_CURVE_POINTS`]
/// order statistics of all the speeds (rank `r` at (r + ½)/n), linear
/// between points and held at the ends; never below the smallest positive
/// speed. `None` without a positive speed. `sorted` is in increasing speed.
fn low_speed_cutoff(sorted: &[(f64, f64)]) -> Option<f64> {
    let smallest_positive = sorted.iter().find(|&&(speed, _)| speed > 0.0)?.0;
    let total: usize = sorted.iter().map(|&(_, weight)| copies(weight)).sum();
    let points = CUTOFF_CURVE_POINTS.min(total);
    if points < 2 {
        return Some(smallest_positive);
    }
    let rank_of = |point: usize| ((total - 1) as f64 * point as f64 / (points - 1) as f64 + 0.5) as usize;
    let percent_of = |rank: usize| (rank as f64 + 0.5) * 100.0 / total as f64;
    // The speed at a rank: ends of each sample's run of ranks, then a search.
    let ends: Vec<usize> = sorted
        .iter()
        .scan(0, |end, &(_, weight)| {
            *end += copies(weight);
            Some(*end)
        })
        .collect();
    let speed_at = |rank: usize| sorted[ends.partition_point(|&end| end <= rank).min(sorted.len() - 1)].0;
    let upper = (0..points).find(|&point| percent_of(rank_of(point)) > LOW_SPEED_PERCENT);
    let at_percent = match upper {
        Some(0) => speed_at(rank_of(0)),
        None => speed_at(rank_of(points - 1)),
        Some(point) => {
            let (below, above) = (rank_of(point - 1), rank_of(point));
            let gap = percent_of(above) - percent_of(below);
            if gap.abs() < CURVE_GAP {
                (speed_at(below) + speed_at(above)) / 2.0
            } else {
                let share = (LOW_SPEED_PERCENT - percent_of(below)) / gap;
                speed_at(below) * (1.0 - share) + speed_at(above) * share
            }
        }
    };
    Some(at_percent.max(smallest_positive))
}

/// Stevens & Smulders: k = 1 / (Σ u^k ln u / Σ u^k − mean ln u) over the
/// positive speeds, iterated from k = 2 until k changes less than 0.1 %, at
/// most 10; then A = (Σ u^k / n)^(1/k) with n counting every speed.
fn stevens_smulders(samples: &[(f64, f64)]) -> CoreResult<Weibull> {
    let total: f64 = samples.iter().map(|&(_, weight)| weight).sum();
    let logs: Vec<(f64, f64)> =
        samples.iter().filter(|&&(speed, _)| speed > 0.0).map(|&(speed, weight)| (libm::log(speed), weight)).collect();
    let positive: f64 = logs.iter().map(|&(_, weight)| weight).sum();
    let mean_log = logs.iter().map(|&(log, weight)| weight * log).sum::<f64>() / positive;
    let top_log = logs.iter().fold(f64::NEG_INFINITY, |top, &(log, _)| top.max(log));
    if logs.len() < 2 || !(top_log - mean_log >= MIN_LOG_SPREAD) {
        return Err(CoreError::TooFewDistinctSpeeds);
    }
    // Σ w·u^k and Σ w·u^k·ln u, scaled by e^{−k·top} so a large k cannot overflow.
    let sums = |shape: f64| {
        logs.iter().fold((0.0, 0.0), |(power, weighted_log), &(log, weight)| {
            let scaled = weight * libm::exp(shape * (log - top_log));
            (power + scaled, weighted_log + scaled * log)
        })
    };
    let mut shape = FIRST_SHAPE;
    for _ in 0..MAX_FIXED_POINT_STEPS {
        let (power, weighted_log) = sums(shape);
        let next = (1.0 / (weighted_log / power - mean_log)).min(MAX_SHAPE);
        let change = (next - shape).abs() / shape;
        shape = next;
        if change < SHAPE_TOLERANCE {
            break;
        }
    }
    let (power, _) = sums(shape);
    Ok(Weibull { k: shape, a: libm::exp(top_log + libm::log(power / total) / shape) })
}

/// The "minimal standard" generator of Park & Miller, as Numerical Recipes'
/// ran0 writes it (with its mask): the one the original draws low speeds
/// with. Uniform in (0, 1).
struct ParkMiller(i32);

impl ParkMiller {
    const MULTIPLIER: i64 = 16_807;
    const MODULUS: i64 = 2_147_483_647;
    /// Schrage's factorisation of the modulus: `MODULUS / MULTIPLIER`.
    const QUOTIENT: i64 = 127_773;
    const MASK: i32 = 0x075B_D924;

    fn new(seed: i32) -> Self {
        ParkMiller(seed)
    }

    fn next(&mut self) -> f64 {
        let state = i64::from(self.0 ^ Self::MASK);
        // C division truncates toward zero, like Rust's; the result fits in i32.
        let mut next = Self::MULTIPLIER * state - Self::MODULUS * (state / Self::QUOTIENT);
        if next < 0 {
            next += Self::MODULUS;
        }
        self.0 = (next as i32) ^ Self::MASK;
        next as f64 / Self::MODULUS as f64
    }
}

/// Least squares through (ln u, ln(−ln(1 − F))), F each speed's plotting
/// position (module docs): slope k, intercept −k ln A. Each run of equal
/// speeds is one x with the sum of its y, so a grouped record costs a
/// logarithm pair per speed and the rest per group.
fn least_squares(samples: &[(f64, f64)], total: f64) -> CoreResult<Weibull> {
    let sorted = by_speed(samples);
    let (low, high) = PLOTTING_POSITION_RANGE;
    // (ln u, copies, Σ y) per positive speed; ranks count the zeros too.
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let mut rank = 0usize;
    for &(speed, weight) in sorted.iter() {
        let count = copies(weight);
        if speed > 0.0 {
            let sum_y: f64 = (rank..rank + count)
                .map(|rank| {
                    let position = ((rank as f64 + 0.5) / total).clamp(low, high);
                    libm::log(-libm::log(1.0 - position))
                })
                .sum();
            points.push((libm::log(speed), count as f64, sum_y));
        }
        rank += count;
    }
    let used: f64 = points.iter().map(|&(_, count, _)| count).sum();
    if points.len() < 2 {
        return Err(CoreError::TooFewDistinctSpeeds);
    }
    let mean_x = points.iter().map(|&(x, count, _)| count * x).sum::<f64>() / used;
    let mean_y = points.iter().map(|&(_, _, sum_y)| sum_y).sum::<f64>() / used;
    let (sum_xx, sum_xy) = points.iter().fold((0.0, 0.0), |(xx, xy), &(x, count, sum_y)| {
        let dx = x - mean_x;
        (xx + count * dx * dx, xy + dx * (sum_y - count * mean_y))
    });
    let shape = sum_xy / sum_xx;
    if !(shape > 0.0) {
        return Err(CoreError::NonPositiveShape);
    }
    Ok(Weibull { k: shape, a: libm::exp(-(mean_y - shape * mean_x) / shape) })
}

/// Ordinary least squares of `y` on `x` (the usual regression names: x the
/// predictor, y the response): (slope, intercept), `None` without spread in x.
pub(crate) fn linfit(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let count = x.len() as f64;
    if x.len() < 2 {
        return None;
    }
    let (mean_x, mean_y) = (x.iter().sum::<f64>() / count, y.iter().sum::<f64>() / count);
    let (mut sum_xx, mut sum_xy) = (0.0, 0.0);
    for (xi, yi) in x.iter().zip(y) {
        sum_xx += (xi - mean_x) * (xi - mean_x);
        sum_xy += (xi - mean_x) * (yi - mean_y);
    }
    if sum_xx <= 0.0 {
        return None;
    }
    let slope = sum_xy / sum_xx;
    Some((slope, mean_y - slope * mean_x))
}

/// Root of `f` in `[low, high]`, which must bracket a sign change.
fn bisect(f: impl Fn(f64) -> f64, mut low: f64, mut high: f64) -> Option<f64> {
    let (f_low, f_high) = (f(low), f(high));
    if !f_low.is_finite() || !f_high.is_finite() || (f_low < 0.0) == (f_high < 0.0) {
        return None;
    }
    for _ in 0..BISECTION_STEPS {
        let middle = 0.5 * (low + high);
        if (f(middle) < 0.0) == (f_low < 0.0) {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(0.5 * (low + high))
}

#[cfg(test)]
mod tests {
    //! Reference values from an independent Python implementation of the
    //! original's algorithms (ARQUITETURA 2026-10-17).
    use super::*;

    const QUANTILES: usize = 1_000;
    /// Zeros added to the quantiles: 11 % of the record, a stalled anemometer.
    const ZEROS: usize = 125;

    /// Weibull(2, 8) at exact quantiles, to a logger's two decimals.
    fn quantiles() -> Vec<f64> {
        (0..QUANTILES)
            .map(|i| {
                let speed = 8.0 * (-(1.0 - (i as f64 + 0.5) / QUANTILES as f64).ln()).sqrt();
                (speed * CLASSES_PER_UNIT).round() / CLASSES_PER_UNIT
            })
            .collect()
    }

    fn with_zeros() -> Vec<f64> {
        [quantiles(), vec![0.0; ZEROS]].concat()
    }

    fn assert_fit(fit: Weibull, k: f64, a: f64) {
        assert!((fit.k - k).abs() < 1e-9 && (fit.a - a).abs() < 1e-9, "{fit:?}, expected k {k}, A {a}");
    }

    #[test]
    fn the_generator_repeats_the_originals_draws() {
        let mut generator = ParkMiller::new(DRAW_SEED);
        for expected in [0.785072263695799, 0.7095359352927356, 0.17046446500833354] {
            assert!((generator.next() - expected).abs() < 1e-15);
        }
    }

    #[test]
    fn likelihood_fits_zeros_as_calms_below_the_cutoff() {
        assert_fit(fit(&quantiles(), Method::MaxLikelihood).unwrap(), 2.0028378946883847, 8.00110539321679);
        // Leaving the zeros out would give the same k as without them; the original draws them as calms.
        assert_fit(fit(&with_zeros(), Method::MaxLikelihood).unwrap(), 1.206622085094531, 6.615418036142252);
    }

    #[test]
    fn least_squares_puts_every_speed_on_the_line() {
        assert_fit(fit(&with_zeros(), Method::LeastSquares).unwrap(), 1.3267966490059087, 7.256687024289919);
    }
}

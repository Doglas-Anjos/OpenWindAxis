//! Analisar › Distribuição de velocidade: the four Weibull fits of one speed
//! column side by side, each with the figures the original's Wind Speed
//! Distribution window reports (checked in its code, ARQUITETURA 2026-10-18):
//! - **Mean** of the fitted distribution, A·Γ(1 + 1/k).
//! - **Share above the mean:** the fit's exp(−(ū/A)^k) at the observed mean ū.
//! - **Power density** at a constant 1.225 kg/m³: ½·ρ·A³·Γ(1 + 3/k).
//! - **R²** of the fit against the histogram: on bins from 0 to a round
//!   upper bound, 40 to 80 of them as an axis would pick them, the
//!   observed share per bin against the fit's; 1 − Σ(f − g)²/Σ(f − f̄)².
//!
//! The measured data's own mean, share above the mean and power density
//! come first. Reproduces the original's table to the shown digits.
use super::RHO_STD;
use crate::acc::DEGREES_PER_TURN;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::weibull::{self, Method, Weibull};

/// ½ of ½·ρ·u³.
const HALF: f64 = 0.5;
/// Relative tolerance of the original's float comparisons.
const SAME_TOLERANCE: f64 = 1e-6;
/// Bins of the R² histogram: as many as an axis of round ticks gives,
/// brought within this range by doubling or halving.
const LEAST_BINS: usize = 40;
const MOST_BINS: usize = 80;
/// Bins when the span has no minor tick at all.
const FALLBACK_BINS: usize = 4;
/// Percent in a share.
const PERCENT: f64 = 100.0;
/// Major and minor ticks of a whole turn, degrees.
const DIRECTION_TICKS: (f64, f64) = (30.0, 10.0);
/// The four fits, in the original's order.
pub const METHODS: [Method; 4] = [Method::MaxLikelihood, Method::LeastSquares, Method::Wasp, Method::Openwind];

/// One fit and its figures.
#[derive(Debug, Clone, PartialEq)]
pub struct DistributionFit {
    /// The fit.
    pub weibull: Weibull,
    /// Mean of the fitted distribution, same units as the speeds.
    pub mean: f64,
    /// Share of the fitted distribution above the observed mean.
    pub above_mean: f64,
    /// Power density of the fitted distribution at [`RHO_STD`], W/m².
    pub power_density: f64,
    /// Goodness of fit against the histogram.
    pub r_squared: f64,
}

/// The measured distribution and every fit of it.
#[derive(Debug, Clone)]
pub struct SpeedDistribution {
    /// Speeds the filter kept.
    pub samples: usize,
    /// Their mean.
    pub mean: f64,
    /// Share of them above the mean.
    pub above_mean: f64,
    /// Their power density at [`RHO_STD`], W/m².
    pub power_density: f64,
    /// Width of the bins R² is measured on: a default for the histogram drawn.
    pub bin_width: f64,
    /// One per [`METHODS`], in order; a fit that fails keeps its error.
    pub fits: Vec<(Method, CoreResult<DistributionFit>)>,
}

/// The speeds of `column` that `view` keeps, fitted by every method.
/// O(n log n) for the sort of the R² histogram, plus the fits.
///
/// # Errors
/// [`CoreError::NoDataAfterFilter`] when no speed is left.
pub fn speed_distribution(dataset: &Dataset, view: &FilterView, column: usize) -> CoreResult<SpeedDistribution> {
    let values = &dataset.columns[column].values;
    let mut speeds: Vec<f64> = view.valid_runs(&[column], 0..dataset.len).into_iter().flatten().map(|step| values[step]).filter(|value| !value.is_nan()).collect();
    if speeds.is_empty() {
        return Err(CoreError::NoDataAfterFilter);
    }
    let count = speeds.len() as f64;
    let mean = speeds.iter().sum::<f64>() / count;
    let above_mean = speeds.iter().filter(|&&speed| speed > mean).count() as f64 / count;
    let power_density = HALF * RHO_STD * speeds.iter().map(|speed| speed * speed * speed).sum::<f64>() / count;
    let fits: Vec<(Method, CoreResult<Weibull>)> = METHODS.iter().map(|&method| (method, weibull::fit(&speeds, method))).collect();
    speeds.sort_unstable_by(f64::total_cmp);
    let bins = histogram_bins(speeds[0], speeds[speeds.len() - 1]);
    let observed = cumulative_percent(&speeds, &bins.edges);
    let fits = fits
        .into_iter()
        .map(|(method, fitted)| {
            let figures = fitted.map(|weibull| DistributionFit {
                weibull,
                mean: weibull.mean(),
                above_mean: 1.0 - weibull.cdf(mean),
                power_density: HALF * RHO_STD * weibull.a.powi(3) * libm::tgamma(1.0 + 3.0 / weibull.k),
                r_squared: r_squared(&observed, &bins.edges, &weibull),
            });
            (method, figures)
        })
        .collect();
    Ok(SpeedDistribution { samples: speeds.len(), mean, above_mean, power_density, bin_width: bins.width, fits })
}

/// The bins R² is measured on.
struct Bins {
    /// Edges `i · width` from 0, one more than the bins.
    edges: Vec<f64>,
    width: f64,
}

/// Bins as the original lays them: the speeds' span widened to round ticks,
/// cut into the number of minor ticks brought within [`LEAST_BINS`] to
/// [`MOST_BINS`]; the edges start at 0 whatever the span's start.
fn histogram_bins(lowest: f64, highest: f64) -> Bins {
    let (low, high) = round_span(lowest, highest);
    let count = bin_count(low, high);
    let width = (high - low) / count as f64;
    Bins { edges: (0..=count).map(|bin| bin as f64 * width).collect(), width }
}

/// Whether two numbers agree within [`SAME_TOLERANCE`], relatively.
fn same(left: f64, right: f64) -> bool {
    if left == 0.0 {
        return right.abs() <= SAME_TOLERANCE;
    }
    if right == 0.0 {
        return left.abs() <= SAME_TOLERANCE;
    }
    ((left - right) / left).abs() <= SAME_TOLERANCE
}

/// Major and minor tick spacing of an axis from `low` to `high`, as the
/// original's charts pick them: 2, 4, 5, 10 or 20 tenths of the span's
/// power of ten, or quarters when both ends are multiples of 4.
fn ticks(low: f64, mut high: f64) -> (f64, f64) {
    if low == 0.0 && high == 0.0 {
        high = 1.0;
    } else if low == high {
        high = low + 1.0;
    }
    // A direction axis ticks every 30°, with a minor tick every 10°.
    if (0.0..=1.0).contains(&low) && (359.0..=DEGREES_PER_TURN).contains(&high) {
        return DIRECTION_TICKS;
    }
    let span = high - low;
    let power = 10f64.powf(span.log10().floor());
    let scaled = span * 10.0 / power;
    let multiple_of_four = |value: f64| same(value, (value * 0.25).round() * 4.0);
    let quarter = scaled * 0.25;
    let (major, minor) = if multiple_of_four(low) && multiple_of_four(high) && [3.0, 4.0, 6.0, 8.0].contains(&quarter) {
        (quarter, 1.0)
    } else {
        match scaled / 5.0 {
            fifth if fifth < 3.0 => (2.0, 1.0),
            fifth if fifth == 3.0 => (5.0, 1.0),
            fifth if fifth < 4.0 => (4.0, 1.0),
            fifth if fifth < 7.5 => (5.0, 1.0),
            fifth if fifth < 15.0 => (10.0, 5.0),
            _ => (20.0, 5.0),
        }
    };
    (power * major / 10.0, power * minor / 10.0)
}

/// `low` and `high` widened to whole major ticks, twice, as the original does.
fn round_span(mut low: f64, mut high: f64) -> (f64, f64) {
    for _ in 0..2 {
        let (major, _) = ticks(low, high);
        low = (low / major).floor() * major;
        let ratio = high / major;
        if !same(ratio, ratio.round()) {
            high = (ratio.trunc() + 1.0) * major;
        }
    }
    (low, high)
}

/// Bins between `low` and `high`: one per minor tick, doubled or halved
/// (by thirds or fifths when odd) into [`LEAST_BINS`]..=[`MOST_BINS`].
fn bin_count(low: f64, high: f64) -> usize {
    let (_, minor) = ticks(low, high);
    let mut count = match ((high - low) / minor).round() as usize {
        0 => FALLBACK_BINS,
        count => count,
    };
    while count < LEAST_BINS {
        count *= 2;
    }
    while count > MOST_BINS {
        count = match count {
            odd if odd % 2 == 1 && odd % 3 == 0 => odd / 3,
            odd if odd % 2 == 1 && odd % 5 == 0 => odd / 5,
            even => even / 2,
        };
    }
    count
}

/// Percent of `sorted` at or below each of `edges`.
fn cumulative_percent(sorted: &[f64], edges: &[f64]) -> Vec<f64> {
    let total = sorted.len() as f64;
    edges.iter().map(|&edge| sorted.partition_point(|&speed| speed <= edge) as f64 * PERCENT / total).collect()
}

/// 1 − Σ(f − g)²/Σ(f − f̄)², `f` the observed percent per bin and `g` the
/// fit's; 0 when the observed shares do not vary.
fn r_squared(observed: &[f64], edges: &[f64], fit: &Weibull) -> f64 {
    let fitted: Vec<f64> = edges.iter().map(|&edge| fit.cdf(edge) * PERCENT).collect();
    let shares = |cumulative: &[f64]| cumulative.windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<f64>>();
    let (observed, fitted) = (shares(observed), shares(&fitted));
    let mean = observed.iter().sum::<f64>() / observed.len() as f64;
    let residual: f64 = observed.iter().zip(&fitted).map(|(f, g)| (f - g) * (f - g)).sum();
    let spread: f64 = observed.iter().map(|f| (f - mean) * (f - mean)).sum();
    if spread > 0.0 {
        1.0 - residual / spread
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bins_are_laid_as_the_original_lays_them() {
        // Speeds from 0.27 to 14.9 m/s: 64 bins of 0.25 m/s up to 16.
        let bins = histogram_bins(0.27, 14.9);
        assert_eq!((bins.edges.len() - 1, bins.width), (64, 0.25));
        assert_eq!(round_span(0.27, 14.9), (0.0, 16.0));
        assert_eq!(ticks(0.0, 360.0), DIRECTION_TICKS, "a whole turn ticks every 30°");
        assert!((LEAST_BINS..=MOST_BINS).contains(&bin_count(0.0, 37.0)));
    }

    #[test]
    fn a_perfect_fit_has_an_r_squared_of_one() {
        let fit = Weibull { k: 2.0, a: 8.0 };
        let edges: Vec<f64> = (0..=64).map(|bin| bin as f64 * 0.25).collect();
        let observed: Vec<f64> = edges.iter().map(|&edge| fit.cdf(edge) * PERCENT).collect();
        assert!((r_squared(&observed, &edges, &fit) - 1.0).abs() < 1e-12);
        let flat = vec![0.0; edges.len()];
        assert_eq!(r_squared(&flat, &edges, &fit), 0.0, "nothing observed varies: no R²");
    }
}

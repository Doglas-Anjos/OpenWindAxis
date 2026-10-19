//! Extreme winds (ESPEC-INTERFACE §1, Analisar › Ventos extremos): the
//! 50-year wind speed from the record's peaks [manual:
//! dlg-analyze-extreme-wind.htm, def-gumbel-distribution.htm,
//! articles-xwa-mis.htm].
//!
//! - **Peaks**, two ways, as in the reference product: the maximum of each
//!   calendar month or year with enough data (periodic maxima), or the peak
//!   of each independent storm, a run above a threshold, storms closer than
//!   the independence time merged (method of independent storms; Cook 1982,
//!   "Towards better estimation of extreme winds", J. Wind Eng. Ind.
//!   Aerodyn. 9).
//! - **Gumbel fit** to the peaks, F(x) = exp(−exp(−(x − μ)/β)): maximum
//!   likelihood (the default) or least squares of x on the reduced variate
//!   at Gringorten's plotting positions (m − 0.44)/(N + 0.12) (Gringorten
//!   1963, J. Geophys. Res. 68(3)), the "simple" fit.
//! - **Annualized** exactly: the largest of r independent Gumbel(μ, β)
//!   peaks is Gumbel(μ + β·ln r, β), with r peaks per year. The T-year wind
//!   is then μ_year − β·ln(−ln(1 − 1/T)).
//! - **Interval [B1]:** the peaks are independent by construction, so the
//!   bootstrap resamples them one by one ([`crate::bootstrap`]).
use crate::bootstrap::{self, Interval};
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time::{days_in_month, days_in_year, Month, SECONDS_PER_DAY, SECONDS_PER_HOUR};
use std::ops::Range;

/// The return period the window leads with, years.
pub const DESIGN_RETURN_YEARS: f64 = 50.0;
/// Return periods of the return-level table, years.
pub const RETURN_PERIODS_YEARS: [f64; 6] = [2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
/// Fewest peaks a Gumbel fit takes.
pub const MIN_PEAKS: usize = 3;
/// Share of a period's steps that must hold data for its maximum to count.
pub const DEFAULT_MIN_RECOVERY: f64 = 0.5;
/// Storms closer than this are one storm, s: the reference product's 48 h.
pub const DEFAULT_INDEPENDENCE_S: i64 = 48 * SECONDS_PER_HOUR;
/// Storms per year the recommended threshold aims at, as the reference
/// product's "Set Recommended" does.
pub const TARGET_STORMS_PER_YEAR: f64 = 20.0;
/// Seconds in a Julian year (365.25 days).
pub const SECONDS_PER_YEAR: f64 = 31_557_600.0;
/// The Euler–Mascheroni constant: the Gumbel mean is μ + γ·β.
pub const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;
/// Gringorten's plotting position constants: (m − a)/(N + 1 − 2a), a = 0.44.
const GRINGORTEN_A: f64 = 0.44;
/// Bisection steps of the likelihood equation.
const BISECTION_STEPS: usize = 200;
/// The recommended threshold is found to this much, in the column's units:
/// half the 0.01 m/s loggers write.
const THRESHOLD_RESOLUTION: f64 = 0.005;
/// The likelihood equation's root is bracketed within this factor of the
/// moments estimate of β, both ways.
const SCALE_BRACKET: f64 = 1e3;

/// How the peaks are found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeakMethod {
    /// The maximum of each calendar month or year.
    PeriodicMaxima {
        /// The period.
        period: ExtremePeriod,
        /// Share of the period's steps that must hold data, 0..=1.
        min_recovery: f64,
    },
    /// The peak of each storm above `threshold` (`None`: the threshold that
    /// gives about [`TARGET_STORMS_PER_YEAR`]).
    IndependentStorms {
        /// m/s, or the column's units.
        threshold: Option<f64>,
        /// Storms closer than this merge, s.
        independence_s: i64,
    },
}

/// Period of the periodic maxima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtremePeriod {
    /// Calendar month: 12 peaks a year.
    Month,
    /// Calendar year.
    Year,
}

/// How the Gumbel distribution is fitted to the peaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GumbelFit {
    /// Maximum likelihood.
    #[default]
    MaxLikelihood,
    /// Least squares on Gringorten's plotting positions.
    Gringorten,
}

/// A Gumbel distribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gumbel {
    /// Mode μ.
    pub mode: f64,
    /// Scale β.
    pub scale: f64,
}

impl Gumbel {
    /// The value exceeded once in `return_years` on average, for the
    /// distribution of annual maxima.
    pub fn return_level(&self, return_years: f64) -> f64 {
        self.mode - self.scale * (-(1.0 - 1.0 / return_years).ln()).ln()
    }

    /// The distribution of the largest of `per_year` independent values.
    pub fn annualized(&self, per_year: f64) -> Gumbel {
        Gumbel { mode: self.mode + self.scale * per_year.ln(), scale: self.scale }
    }
}

/// One peak.
#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    /// The step of the peak.
    pub step: usize,
    /// Its value.
    pub value: f64,
    /// The period or storm it tops.
    pub span: Range<usize>,
    /// Share of the period's steps with data (periodic maxima); 1 for a storm.
    pub recovery: f64,
}

/// The extreme-wind estimate of one column.
#[derive(Debug, Clone, PartialEq)]
pub struct Extremes {
    /// The peaks, in time order.
    pub peaks: Vec<Peak>,
    /// Peaks per year.
    pub per_year: f64,
    /// The storm threshold used (independent storms only).
    pub threshold: Option<f64>,
    /// The fit to the peaks.
    pub fit: Gumbel,
    /// The distribution of annual maxima.
    pub annual: Gumbel,
    /// The [`DESIGN_RETURN_YEARS`] wind.
    pub design: f64,
    /// Its bootstrap interval, when asked for and enough replicates fit.
    pub interval: Option<Interval>,
    /// `(years, value)` for [`RETURN_PERIODS_YEARS`].
    pub return_levels: Vec<(f64, f64)>,
    /// Each peak with its empirical annual return period, years, from its
    /// Gringorten position: `(value, years)`, ascending.
    pub empirical: Vec<(f64, f64)>,
}

/// Extreme winds of `column` under `view`. `months` is the record's
/// calendar (`DayStats::months`). With `confidence`, the bootstrap adds a
/// fit per replicate.
///
/// # Errors
/// [`CoreError::TooFewPeaks`] or [`CoreError::GumbelNoSolution`].
pub fn extremes(dataset: &Dataset, view: &FilterView, months: &[Month], column: usize, method: PeakMethod, fit: GumbelFit, confidence: bool) -> CoreResult<Extremes> {
    let values = &dataset.columns[column].values;
    let runs = view.valid_runs(&[column], 0..dataset.len);
    let valid: Vec<usize> = runs.into_iter().flatten().filter(|&step| !values[step].is_nan()).collect();
    let (peaks, per_year, threshold) = match method {
        PeakMethod::PeriodicMaxima { period, min_recovery } => {
            let periods = periods(dataset, months, period);
            (periodic_maxima(values, &valid, &periods, min_recovery), period.per_year(), None)
        }
        PeakMethod::IndependentStorms { threshold, independence_s } => {
            let years = valid.len() as f64 * dataset.step_s as f64 / SECONDS_PER_YEAR;
            let gap_steps = (independence_s / dataset.step_s.max(1)).max(0) as usize;
            let threshold = threshold.unwrap_or_else(|| recommended_threshold(values, &valid, gap_steps, years));
            let peaks = storms(values, &valid, threshold, gap_steps);
            let per_year = peaks.len() as f64 / years;
            (peaks, per_year, Some(threshold))
        }
    };
    if peaks.len() < MIN_PEAKS {
        return Err(CoreError::TooFewPeaks { found: peaks.len(), needed: MIN_PEAKS });
    }
    let sample: Vec<f64> = peaks.iter().map(|peak| peak.value).collect();
    let fitted = gumbel(&sample, fit).ok_or(CoreError::GumbelNoSolution)?;
    let annual = fitted.annualized(per_year);
    let interval = confidence.then(|| design_interval(&sample, fit, per_year)).flatten();
    let mut sorted = sample.clone();
    sorted.sort_unstable_by(f64::total_cmp);
    let count = sorted.len() as f64;
    let empirical = sorted
        .iter()
        .enumerate()
        .map(|(rank, &value)| {
            let event_probability = (rank as f64 + 1.0 - GRINGORTEN_A) / (count + 1.0 - 2.0 * GRINGORTEN_A);
            (value, 1.0 / (1.0 - event_probability.powf(per_year)))
        })
        .collect();
    Ok(Extremes {
        peaks,
        per_year,
        threshold,
        fit: fitted,
        annual,
        design: annual.return_level(DESIGN_RETURN_YEARS),
        interval,
        return_levels: RETURN_PERIODS_YEARS.iter().map(|&years| (years, annual.return_level(years))).collect(),
        empirical,
    })
}

impl ExtremePeriod {
    fn per_year(self) -> f64 {
        match self {
            ExtremePeriod::Month => 12.0,
            ExtremePeriod::Year => 1.0,
        }
    }
}

/// The record's calendar months or years: their steps in the record and
/// the steps a whole period holds.
fn periods(dataset: &Dataset, months: &[Month], period: ExtremePeriod) -> Vec<(Range<usize>, f64)> {
    let steps_in_days = |days: u32| f64::from(days) * SECONDS_PER_DAY as f64 / dataset.step_s as f64;
    let mut periods: Vec<(Range<usize>, f64)> = Vec::new();
    let mut keys: Vec<(i32, u32)> = Vec::new();
    for month in months {
        let (key, whole) = match period {
            ExtremePeriod::Month => ((month.year, month.month), steps_in_days(days_in_month(month.year, month.month))),
            ExtremePeriod::Year => ((month.year, 1), steps_in_days(days_in_year(month.year))),
        };
        if keys.last() == Some(&key) {
            periods.last_mut().expect("a key has its period").0.end = month.step_end;
        } else {
            keys.push(key);
            periods.push((month.steps(), whole));
        }
    }
    periods
}

/// The maximum of each period whose share of valid steps reaches `min_recovery`.
fn periodic_maxima(values: &[f64], valid: &[usize], periods: &[(Range<usize>, f64)], min_recovery: f64) -> Vec<Peak> {
    let mut peaks = Vec::new();
    let mut at = 0;
    for (steps, whole) in periods {
        let first = at;
        while at < valid.len() && valid[at] < steps.end {
            at += 1;
        }
        let in_period = &valid[first..at];
        let recovery = in_period.len() as f64 / whole;
        let Some(&step) = in_period.iter().max_by(|&&left, &&right| values[left].total_cmp(&values[right])) else { continue };
        if recovery >= min_recovery {
            peaks.push(Peak { step, value: values[step], span: steps.clone(), recovery });
        }
    }
    peaks
}

/// The peak of each storm: runs of valid steps above `threshold`, merged
/// when fewer than `gap_steps` steps apart.
fn storms(values: &[f64], valid: &[usize], threshold: f64, gap_steps: usize) -> Vec<Peak> {
    let mut peaks: Vec<Peak> = Vec::new();
    for &step in valid.iter().filter(|&&step| values[step] > threshold) {
        match peaks.last_mut() {
            Some(storm) if step < storm.span.end + gap_steps => {
                storm.span.end = step + 1;
                if values[step] > storm.value {
                    (storm.step, storm.value) = (step, values[step]);
                }
            }
            _ => peaks.push(Peak { step, value: values[step], span: step..step + 1, recovery: 1.0 }),
        }
    }
    peaks
}

/// The highest threshold that still gives [`TARGET_STORMS_PER_YEAR`]: storms
/// thin out as the threshold rises, so bisect between the lowest and the
/// highest valid value. Each accepted threshold keeps only the steps above
/// it for the next tries, so after the first halving the search reads a
/// few thousand steps, not the record.
fn recommended_threshold(values: &[f64], valid: &[usize], gap_steps: usize, years: f64) -> f64 {
    let (mut low, mut high) = valid.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &step| (low.min(values[step]), high.max(values[step])));
    if !(low < high) || years <= 0.0 {
        return low;
    }
    let mut candidates = valid.to_vec();
    while high - low > THRESHOLD_RESOLUTION {
        let middle = (low + high) / 2.0;
        // Huge values have no float between `low` and `high` this close: stop there.
        if middle <= low || middle >= high {
            break;
        }
        let above: Vec<usize> = candidates.iter().copied().filter(|&step| values[step] > middle).collect();
        if storms(values, &above, middle, gap_steps).len() as f64 / years >= TARGET_STORMS_PER_YEAR {
            low = middle;
            candidates = above;
        } else {
            high = middle;
        }
    }
    low
}

/// The Gumbel fit of `sample` by `fit`; `None` without spread or a root.
pub fn gumbel(sample: &[f64], fit: GumbelFit) -> Option<Gumbel> {
    let count = sample.len() as f64;
    let mean = sample.iter().sum::<f64>() / count;
    let variance = sample.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (count - 1.0);
    if !(variance > 0.0) {
        return None;
    }
    match fit {
        GumbelFit::MaxLikelihood => max_likelihood(sample, mean, variance.sqrt() * 6f64.sqrt() / std::f64::consts::PI),
        GumbelFit::Gringorten => {
            let mut sorted = sample.to_vec();
            sorted.sort_unstable_by(f64::total_cmp);
            let reduced: Vec<f64> = (1..=sorted.len())
                .map(|rank| -(-((rank as f64 - GRINGORTEN_A) / (count + 1.0 - 2.0 * GRINGORTEN_A)).ln()).ln())
                .collect();
            let reduced_mean = reduced.iter().sum::<f64>() / count;
            let covariance: f64 = sorted.iter().zip(&reduced).map(|(value, y)| (value - mean) * (y - reduced_mean)).sum();
            let spread: f64 = reduced.iter().map(|y| (y - reduced_mean).powi(2)).sum();
            let scale = covariance / spread;
            Some(Gumbel { mode: mean - scale * reduced_mean, scale })
        }
    }
}

/// Maximum likelihood: β solves β = x̄ − Σx·e^(−x/β) / Σe^(−x/β), then
/// μ = −β·ln(mean of e^(−x/β)); by bisection around the moments estimate.
/// Exponents are taken from the smallest value, so they never overflow.
fn max_likelihood(sample: &[f64], mean: f64, moments_scale: f64) -> Option<Gumbel> {
    let smallest = sample.iter().copied().fold(f64::INFINITY, f64::min);
    let weights = |scale: f64| sample.iter().map(move |&value| (-(value - smallest) / scale).exp());
    let equation = |scale: f64| {
        let total: f64 = weights(scale).sum();
        let weighted: f64 = sample.iter().zip(weights(scale)).map(|(value, weight)| value * weight).sum();
        mean - scale - weighted / total
    };
    let (mut low, mut high) = (moments_scale / SCALE_BRACKET, moments_scale * SCALE_BRACKET);
    if equation(low).signum() == equation(high).signum() {
        return None;
    }
    for _ in 0..BISECTION_STEPS {
        let middle = (low + high) / 2.0;
        if middle <= low || middle >= high {
            break;
        }
        if equation(middle).signum() == equation(low).signum() {
            low = middle;
        } else {
            high = middle;
        }
    }
    let scale = (low + high) / 2.0;
    let mean_weight = weights(scale).sum::<f64>() / sample.len() as f64;
    Some(Gumbel { mode: smallest - scale * mean_weight.ln(), scale })
}

/// The bootstrap interval of the design wind: the peaks resampled with
/// replacement, refitted and annualized with the same rate.
fn design_interval(sample: &[f64], fit: GumbelFit, per_year: f64) -> Option<Interval> {
    let mut replicates = Vec::with_capacity(bootstrap::RESAMPLES);
    let mut drawn = Vec::with_capacity(sample.len());
    bootstrap::each_replicate(sample.len(), |draws| {
        drawn.clear();
        for (&value, &times) in sample.iter().zip(draws) {
            drawn.extend(std::iter::repeat(value).take(times as usize));
        }
        replicates.push(gumbel(&drawn, fit).map_or(f64::NAN, |fitted| fitted.annualized(per_year).return_level(DESIGN_RETURN_YEARS)));
    });
    bootstrap::percentile_interval(&replicates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_search_ends_on_values_too_large_for_its_resolution() {
        // Around 1e17 neighbouring floats are 16 apart, far coarser than the
        // search's resolution: it must stop when halving no longer moves.
        let values: Vec<f64> = (0..400).map(|step| 1e17 + (step % 40) as f64 * 64.0).collect();
        let valid: Vec<usize> = (0..values.len()).collect();
        let threshold = recommended_threshold(&values, &valid, 0, 1.0);
        assert!(threshold >= 1e17 && threshold <= 1e17 + 40.0 * 64.0, "{threshold}");
    }

    #[test]
    fn gringorten_recovers_values_on_its_own_positions() {
        let (mode, scale) = (20.0, 3.0);
        let count = 12.0;
        let sample: Vec<f64> = (1..=12).map(|rank| mode + scale * -(-((rank as f64 - GRINGORTEN_A) / (count + 1.0 - 2.0 * GRINGORTEN_A)).ln()).ln()).collect();
        let fitted = gumbel(&sample, GumbelFit::Gringorten).unwrap();
        assert!((fitted.mode - mode).abs() < 1e-9 && (fitted.scale - scale).abs() < 1e-9, "{fitted:?}");
    }

    #[test]
    fn annual_maxima_of_twelve_monthly_peaks() {
        let monthly = Gumbel { mode: 20.0, scale: 2.0 };
        let annual = monthly.annualized(12.0);
        assert!((annual.mode - (20.0 + 2.0 * 12f64.ln())).abs() < 1e-12);
        // The 50-year value: μ − β·ln(−ln(0.98)).
        assert!((annual.return_level(50.0) - (annual.mode + 2.0 * 3.901_938_658)).abs() < 1e-6);
    }
}

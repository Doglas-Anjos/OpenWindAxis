//! Long-term adjustment by measure-correlate-predict (MCP; ESPEC-INTERFACE
//! §1, Revisar › Ajuste de longo prazo; PLANO § Longo prazo): the site's
//! speed is related to a long reference record (reanalysis, a nearby mast)
//! over the period both cover, and the relation predicts the site over the
//! whole reference record [manual: dlg-revise-LTA.htm].
//!
//! - **Averaging:** both columns become means over fixed periods (an hour or
//!   a day) aligned to the epoch on the datasets' clocks; a period counts
//!   when at least [`MIN_PERIOD_COVERAGE`] of its steps hold data. The two
//!   datasets are taken to share a clock.
//! - **Eight speed methods, side by side** ([`McpMethod`]): four lines
//!   y = slope·x + intercept (least squares, which minimises the site's
//!   error and shrinks its spread; total least squares, orthogonal; variance
//!   ratio, slope σ_y/σ_x, which keeps the spread; speed ratio, through the
//!   origin), and three that match distributions or follow the data:
//!   SpeedSort (least squares of the sorted site speeds on the sorted
//!   reference speeds), Weibull (each reference speed mapped to the site
//!   speed of the same Weibull probability) and vertical slices (the mean
//!   site speed in each 1 m/s slice of the reference, joined by straight
//!   segments). Predictions below 0 are 0.
//! - **Matrix time series** (MTS, manual articles-mcp-matrix-time-series.htm):
//!   the joint distribution of reference and site speeds in bins (1 m/s
//!   each by default, the original's); each concurrent period's site speed
//!   becomes its percentile in the distribution of its reference bin (the
//!   cumulative counts interpolated inside a bin), the percentiles are
//!   smoothed by a centred moving average (3 h by default), the periods
//!   without a site speed get theirs from the Markov chain of the
//!   reconstruction ([`crate::edits`], over 0.05-wide bins of percentile),
//!   and each percentile goes back to a site speed through its reference
//!   bin's distribution. A reference bin with fewer than 10 pairs falls back
//!   on the least squares line, as the original's. The cross-validation and
//!   the interval use each bin's mean site speed.
//! - **Cross-validation [B2]:** each calendar month of the concurrent period
//!   in turn is left out, the method refitted on the rest and the left-out
//!   month's mean predicted. The method with the smallest error of those
//!   monthly means is recommended; each method's spread ratio (predicted
//!   over observed standard deviation, concurrent period) says how it
//!   treats the variance, which the energy depends on.
//! - **Long term:** the method applied to every reference period gives the
//!   site's long-term mean, its Weibull fit and the synthesized series; on
//!   request, the mean's interval [B1] by resampling the concurrent months
//!   (months, not periods: a month's hours are not independent).
//! - **By sector:** with the reference's direction, each method is also
//!   fitted per sector of that direction, and each period is predicted by
//!   its sector's fit; a sector with fewer than [`MIN_CONCURRENT_PERIODS`]
//!   pairs falls back to the fit of all sectors.
//! - **Direction:** with both directions, the site's direction is the
//!   reference's plus a constant offset, the circular mean of their
//!   differences (the reference product's constant offset); the long-term
//!   rose is the reference's turned by it.
//! - **Options** ([`McpOptions`], manual articles-mcp-*): yearly divisions
//!   (the months in 2, 3, 4, 6 or 12 groups, each fitted apart, crossed with
//!   the sectors); a cutoff speed (with sectors, the periods whose reference
//!   speed is below it form one omnidirectional group of their own, fitted
//!   and predicted apart; without, only the fits leave them out; the speed
//!   ratio leaves out a pair where either speed is below it); and lines
//!   through the origin. A subdivision is fitted from a day of periods (at
//!   least 8), the original's minimum; below it, the overall fit predicts.
use crate::acc::{sincos_deg, MeanAcc, DEGREES_PER_TURN};
use crate::analysis::{check_sectors, sector_of};
use crate::bootstrap::{self, Interval};
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::weibull::{self, Method, Weibull};
use chrono::{DateTime, Datelike};

/// Share of a period's steps that must hold data for its mean to count.
pub const MIN_PERIOD_COVERAGE: f64 = 0.5;
/// Fewest concurrent periods a method is fitted to.
pub const MIN_CONCURRENT_PERIODS: usize = 10;
/// Fewest left-out months a cross-validation needs.
const MIN_FOLDS: usize = 2;
/// Width of a vertical slice of reference speeds, m/s.
pub const SLICE_WIDTH_M_S: f64 = 1.0;
/// Fewest pairs a vertical slice needs to place a point.
pub const MIN_SLICE_PAIRS: usize = 5;
/// Fewest slices a vertical-slice curve needs.
const MIN_SLICES: usize = 2;
/// Months per year, for the folds' keys.
const MONTHS_PER_YEAR: i32 = 12;
/// Sectors of the direction roses of a direction offset.
pub const ROSE_SECTORS: usize = 16;
/// Fewest pairs a reference bin of a matrix time series needs; with fewer,
/// the least squares line predicts it (the original's backup).
pub const MTS_MIN_BIN_PAIRS: usize = 10;
/// Width of the percentile bins of the matrix time series' Markov chain.
const MTS_PERCENTILE_BIN: f64 = 0.05;
/// Seed of the matrix time series' Markov chain: fixed, so a run repeats.
const MTS_SEED: u64 = 0x4D54_5300_2026_1018;
/// The percentile given where the chain has nothing to draw from.
const MEDIAN_PERCENTILE: f64 = 0.5;

/// The settings of the matrix time series (the original's defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MtsOptions {
    /// Width of the reference speed bins, m/s.
    pub reference_bin: f64,
    /// Width of the site speed bins, m/s.
    pub target_bin: f64,
    /// Window of the moving average of the percentiles, hours.
    pub window_h: f64,
}

impl Default for MtsOptions {
    fn default() -> Self {
        MtsOptions { reference_bin: 1.0, target_bin: 1.0, window_h: 3.0 }
    }
}

/// The yearly divisions offered: groups of 12, 6, 4, 3, 2 or 1 months.
pub const YEARLY_DIVISIONS: [usize; 6] = [1, 2, 3, 4, 6, 12];
/// A subdivision is fitted from this many periods at least, and from a day of them.
const MIN_DIVISION_PERIODS: usize = 8;
const SECONDS_PER_DAY: i64 = 86_400;

/// How the speed fits are made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpOptions {
    /// Groups of calendar months fitted apart: one of [`YEARLY_DIVISIONS`].
    pub yearly_divisions: usize,
    /// Cutoff of the reference speed, m/s; `None` = none.
    pub cutoff_m_s: Option<f64>,
    /// Lines through the origin (least squares, total least squares,
    /// variance ratio and SpeedSort).
    pub zero_intercept: bool,
    /// The matrix time series' settings.
    pub mts: MtsOptions,
}

impl Default for McpOptions {
    fn default() -> Self {
        McpOptions { yearly_divisions: 1, cutoff_m_s: None, zero_intercept: false, mts: MtsOptions::default() }
    }
}

/// How the concurrent periods are subdivided: by sector of the reference's
/// direction (plus the below-cutoff group) and by group of months.
#[derive(Debug, Clone, Copy)]
struct Layout {
    sectors: usize,
    below_cutoff: Option<f64>,
    months: usize,
}

impl Layout {
    fn new(sectors: usize, options: &McpOptions) -> Self {
        Layout { sectors, below_cutoff: options.cutoff_m_s.filter(|_| sectors > 1), months: options.yearly_divisions }
    }

    /// Groups per group of months.
    fn per_months(&self) -> usize {
        self.sectors + usize::from(self.below_cutoff.is_some())
    }

    /// Subdivisions in all; 1 = none.
    fn groups(&self) -> usize {
        self.per_months() * self.months
    }

    /// The subdivision of a period with reference speed `x`, reference
    /// direction `direction` (NaN: none) in calendar month `month0`
    /// (0 = January); `None` without subdivisions or without the direction
    /// they need.
    fn division(&self, x: f64, direction: f64, month0: usize) -> Option<usize> {
        if self.groups() == 1 {
            return None;
        }
        let sector = if self.sectors == 1 {
            0
        } else if self.below_cutoff.is_some_and(|cutoff| x < cutoff) {
            self.sectors
        } else if direction.is_nan() {
            return None;
        } else {
            sector_of(direction, self.sectors)
        };
        Some(month0 * self.months / MONTHS_PER_YEAR as usize * self.per_months() + sector)
    }
}

/// The speed methods, in the order results are given.
pub const MCP_METHODS: [McpMethod; 8] = [
    McpMethod::LeastSquares,
    McpMethod::TotalLeastSquares,
    McpMethod::VarianceRatio,
    McpMethod::SpeedRatio,
    McpMethod::SpeedSort,
    McpMethod::WeibullFit,
    McpMethod::VerticalSlice,
    McpMethod::MatrixTimeSeries,
];

/// A speed method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMethod {
    /// Ordinary least squares of the site on the reference.
    LeastSquares,
    /// Total (orthogonal) least squares.
    TotalLeastSquares,
    /// Slope σ_y/σ_x through the means: keeps the spread.
    VarianceRatio,
    /// Slope ȳ/x̄ through the origin.
    SpeedRatio,
    /// Least squares of the sorted site speeds on the sorted reference speeds.
    SpeedSort,
    /// Mapping through the two Weibull fits, probability for probability.
    WeibullFit,
    /// Mean site speed per slice of reference speed, joined by segments.
    VerticalSlice,
    /// The joint distribution's percentiles, carried in time.
    MatrixTimeSeries,
}

/// A line y = slope·x + intercept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McpLine {
    /// Slope.
    pub slope: f64,
    /// Intercept, in the site's units.
    pub intercept: f64,
}

/// How a method turns a reference speed into a site speed.
#[derive(Debug, Clone, PartialEq)]
pub enum Predictor {
    /// A line.
    Line(McpLine),
    /// The site speed of the same Weibull probability:
    /// y = A_s·((x/A_r)^k_r)^(1/k_s).
    Weibull {
        /// The reference's fit.
        reference: Weibull,
        /// The site's fit.
        site: Weibull,
    },
    /// Points `(reference, site)`, ascending, joined by segments and
    /// extended along the first and the last.
    Slices(Vec<(f64, f64)>),
    /// A matrix time series' joint distribution; as a function of the
    /// reference alone, the mean site speed of its bin.
    Matrix(Box<MatrixFit>),
}

/// The joint distribution of a matrix time series.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixFit {
    /// Width of the reference bins, m/s.
    pub reference_bin: f64,
    /// Width of the site bins, m/s.
    pub target_bin: f64,
    /// Per reference bin (from 0), the cumulative counts of the site bins
    /// (from 0); `None` with fewer than [`MTS_MIN_BIN_PAIRS`] pairs.
    pub cumulative: Vec<Option<Vec<f64>>>,
    /// The least squares line of the same pairs, for the sparse bins.
    pub fallback: McpLine,
}

impl MatrixFit {
    /// Reference bin `x`'s cumulative counts, unless it is sparse or empty.
    fn column(&self, x: f64) -> Option<&[f64]> {
        if !(x >= 0.0) {
            return None;
        }
        self.cumulative.get((x / self.reference_bin) as usize)?.as_deref()
    }

    fn line(&self, x: f64) -> f64 {
        self.fallback.slope * x + self.fallback.intercept
    }

    /// The mean site speed of reference bin `x` (each site bin at its centre).
    pub fn mean(&self, x: f64) -> f64 {
        let Some(cumulative) = self.column(x) else { return self.line(x) };
        let total = cumulative[cumulative.len() - 1];
        let mut previous = 0.0;
        let weighted: f64 = cumulative
            .iter()
            .enumerate()
            .map(|(bin, &count)| {
                let inside = count - previous;
                previous = count;
                inside * (bin as f64 + 0.5) * self.target_bin
            })
            .sum();
        weighted / total
    }

    /// Where site speed `y` falls in reference bin `x`'s distribution, 0 to
    /// 1, interpolated inside its site bin; NaN in a sparse bin.
    pub fn percentile(&self, x: f64, y: f64) -> f64 {
        let Some(cumulative) = self.column(x) else { return f64::NAN };
        let total = cumulative[cumulative.len() - 1];
        let position = (y / self.target_bin).max(0.0);
        let bin = (position as usize).min(cumulative.len() - 1);
        let below = if bin == 0 { 0.0 } else { cumulative[bin - 1] };
        let fraction = (position - bin as f64).clamp(0.0, 1.0);
        (below + (cumulative[bin] - below) * fraction) / total
    }

    /// The site speed at percentile `p` of reference bin `x`'s
    /// distribution; the fallback line in a sparse bin.
    pub fn quantile(&self, x: f64, p: f64) -> f64 {
        let Some(cumulative) = self.column(x) else { return self.line(x) };
        let total = cumulative[cumulative.len() - 1];
        let target = p.clamp(0.0, 1.0) * total;
        // The first site bin whose running count passes the target: never an empty one.
        let bin = cumulative.partition_point(|&count| count <= target);
        if bin == cumulative.len() {
            // The very top: the upper edge of the last bin with data.
            let last = cumulative.partition_point(|&count| count < total);
            return (last + 1) as f64 * self.target_bin;
        }
        let below = if bin == 0 { 0.0 } else { cumulative[bin - 1] };
        (bin as f64 + (target - below) / (cumulative[bin] - below)) * self.target_bin
    }
}

impl Predictor {
    /// The site's speed predicted from reference speed `reference`, never below 0.
    pub fn predict(&self, reference: f64) -> f64 {
        let predicted = match self {
            Predictor::Line(line) => line.slope * reference + line.intercept,
            Predictor::Weibull { reference: from, site: to } if reference > 0.0 => {
                to.a * ((reference / from.a).powf(from.k)).powf(1.0 / to.k)
            }
            Predictor::Weibull { .. } => 0.0,
            Predictor::Slices(points) => {
                // The segment around `reference`, or the first or last one beyond the ends.
                let after = points.partition_point(|&(x, _)| x < reference).clamp(1, points.len() - 1);
                let ((x0, y0), (x1, y1)) = (points[after - 1], points[after]);
                y0 + (y1 - y0) * (reference - x0) / (x1 - x0)
            }
            Predictor::Matrix(fit) => fit.mean(reference),
        };
        predicted.max(0.0)
    }

    /// The line, when the method draws one.
    pub fn line(&self) -> Option<McpLine> {
        match self {
            Predictor::Line(line) => Some(*line),
            _ => None,
        }
    }
}

/// One method's fit and how it fares.
#[derive(Debug, Clone)]
pub struct McpResult {
    /// The method.
    pub method: McpMethod,
    /// Its fit to every concurrent period.
    pub predictor: Predictor,
    /// Root mean square error of the left-out monthly means; NaN with fewer
    /// than two months.
    pub cv_rmse: f64,
    /// Mean error (predicted minus observed) of the left-out monthly means.
    pub cv_bias: f64,
    /// Standard deviation of the predictions over the observed one, in the
    /// concurrent period: below 1 the method shrinks the spread.
    pub spread_ratio: f64,
    /// Mean of the predictions over the whole reference record.
    pub long_term_mean: f64,
    /// Its bootstrap interval, when asked for and enough replicates fit.
    pub long_term_interval: Option<Interval>,
    /// Per subdivision (group of months × sector of the reference's
    /// direction, plus the below-cutoff group), its fit (`None`: too few
    /// pairs, the overall fit predicts it); empty without subdivisions.
    pub division_predictors: Vec<Option<Predictor>>,
    /// Weibull fit of those predictions (maximum likelihood), when one exists.
    pub long_term_weibull: Option<Weibull>,
    /// The matrix time series' site speed at every reference period with
    /// data, in order; `None` for the other methods, whose series is their
    /// prediction of the reference alone.
    pub synthesized: Option<Vec<f64>>,
}

/// A long-term adjustment.
#[derive(Debug, Clone)]
pub struct Mcp {
    /// Averaging period, s.
    pub period_s: i64,
    /// Concurrent periods.
    pub concurrent: usize,
    /// Start of the first and of the last concurrent period, epoch s.
    pub concurrent_span: (i64, i64),
    /// Pearson correlation of the concurrent periods.
    pub correlation: f64,
    /// Site mean over the concurrent periods.
    pub site_mean: f64,
    /// Reference mean over the concurrent periods.
    pub reference_concurrent_mean: f64,
    /// Reference mean over its whole record.
    pub reference_long_term_mean: f64,
    /// Reference periods with data.
    pub reference_periods: usize,
    /// Calendar months the cross-validation left out in turn.
    pub folds: usize,
    /// One result per method that could be fitted, in [`MCP_METHODS`] order.
    pub results: Vec<McpResult>,
    /// The method with the smallest cross-validation error; without a
    /// cross-validation, the first method fitted (least squares, usually).
    pub recommended: McpMethod,
    /// The concurrent pairs `(reference, site)`, for the scatter.
    pub pairs: Vec<(f64, f64)>,
    /// Sectors of the speed fits (1: one fit for all).
    pub sectors: usize,
    /// Subdivisions of the speed fits (1: one fit for all).
    pub divisions: usize,
    /// The direction's constant offset, with both directions.
    pub direction: Option<DirectionOffset>,
}

/// The direction columns of an adjustment: by sector of the reference's
/// direction, and the site's direction itself.
#[derive(Debug, Clone, Copy)]
pub struct Directions<'a> {
    /// The reference's direction, averaged like its speed.
    pub reference: &'a Averaged,
    /// The site's direction, for the offset; `None` without one.
    pub site: Option<&'a Averaged>,
    /// Sectors of the speed fits; 1 = one fit for all.
    pub sectors: usize,
}

/// The site's direction as the reference's plus a constant.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionOffset {
    /// Site minus reference, the circular mean of the concurrent periods, °
    /// (−180 to 180).
    pub offset_deg: f64,
    /// Concurrent periods with both directions.
    pub concurrent: usize,
    /// Frequency of the site's direction per [`ROSE_SECTORS`] sector, concurrent periods.
    pub site_frequency: Vec<f64>,
    /// Frequency of the predicted long-term direction per sector.
    pub long_term_frequency: Vec<f64>,
}

/// One concurrent period: reference speed, site speed, and its subdivision.
#[derive(Debug, Clone, Copy)]
struct Sample {
    x: f64,
    y: f64,
    division: Option<usize>,
}

/// A method's overall fit and its fit per subdivision.
struct Fitted {
    overall: Predictor,
    by_division: Vec<Option<Predictor>>,
}

impl Fitted {
    /// The predictor of `division`: its own, or the overall one.
    fn of(&self, division: Option<usize>) -> &Predictor {
        division.and_then(|division| self.by_division.get(division)?.as_ref()).unwrap_or(&self.overall)
    }

    /// As [`McpResult::predict`].
    fn predict(&self, reference: f64, division: Option<usize>) -> f64 {
        self.of(division).predict(reference)
    }
}

/// How a fit is made: the subdivisions, the options and the fewest periods a
/// subdivision is fitted from.
#[derive(Debug, Clone, Copy)]
struct Fitting {
    layout: Layout,
    options: McpOptions,
    min_division: usize,
}

/// A column averaged over the periods of a record: `means[i]` is the mean
/// over period `first + i` (NaN without enough data).
#[derive(Debug, Clone, PartialEq)]
pub struct Averaged {
    /// Index of the first period, epoch s / period.
    pub first: i64,
    /// One mean per period.
    pub means: Vec<f64>,
}

impl Averaged {
    /// The mean over period `period`, NaN outside the record.
    fn at(&self, period: i64) -> f64 {
        usize::try_from(period - self.first).ok().and_then(|index| self.means.get(index)).copied().unwrap_or(f64::NAN)
    }
}

/// `column` of `dataset` averaged over periods of `period_s` seconds, steps
/// excluded by `view` left out.
///
/// # Errors
/// [`CoreError::AveragingShorterThanStep`] when a period is shorter than a step.
pub fn average(dataset: &Dataset, view: &FilterView, column: usize, period_s: i64) -> CoreResult<Averaged> {
    if period_s < dataset.step_s || dataset.step_s <= 0 {
        return Err(CoreError::AveragingShorterThanStep { period_s, step_s: dataset.step_s });
    }
    let values = &dataset.columns[column].values;
    let is_angle = dataset.columns[column].is_angle();
    let first = dataset.start.div_euclid(period_s);
    let last = dataset.time_at(dataset.len.saturating_sub(1)).div_euclid(period_s);
    let mut sums = vec![MeanAcc::EMPTY; (last - first + 1).max(0) as usize];
    for step in view.valid_runs(&[column], 0..dataset.len).into_iter().flatten() {
        if !values[step].is_nan() {
            sums[(dataset.time_at(step).div_euclid(period_s) - first) as usize].add(values[step], is_angle);
        }
    }
    let needed = MIN_PERIOD_COVERAGE * (period_s as f64 / dataset.step_s as f64);
    let means = sums.iter().map(|sum| if sum.count as f64 >= needed { sum.mean(is_angle) } else { f64::NAN }).collect();
    Ok(Averaged { first, means })
}

/// The calendar month (0 = January) of the period starting at `period · period_s`.
fn month0_of(period: i64, period_s: i64) -> usize {
    DateTime::from_timestamp(period * period_s, 0).map_or(0, |time| time.month0() as usize)
}

/// The subdivision of period `period`, whose reference speed is `x`.
fn division_at(layout: &Layout, directions: Option<Directions>, period: i64, period_s: i64, x: f64) -> Option<usize> {
    let direction = directions.filter(|_| layout.sectors > 1).map_or(f64::NAN, |directions| directions.reference.at(period));
    layout.division(x, direction, month0_of(period, period_s))
}

/// Fails with [`CoreError::YearlyDivisionsInvalid`] outside [`YEARLY_DIVISIONS`].
fn check_options(options: &McpOptions) -> CoreResult<()> {
    if YEARLY_DIVISIONS.contains(&options.yearly_divisions) {
        Ok(())
    } else {
        Err(CoreError::YearlyDivisionsInvalid { divisions: options.yearly_divisions })
    }
}

/// Relates `site` (target speeds) to `reference` and predicts the long
/// term, as the module describes: by sector and with the direction's offset
/// when `directions` has them, and by `options`; with `confidence`, the
/// long-term means' intervals too.
///
/// # Errors
/// [`CoreError::TooFewConcurrentPeriods`], [`CoreError::SectorsOutOfRange`],
/// [`CoreError::YearlyDivisionsInvalid`], or [`CoreError::McpMethodUnavailable`]
/// when no method fits (a reference without spread).
pub fn mcp(site: &Averaged, reference: &Averaged, directions: Option<Directions>, period_s: i64, confidence: bool, options: &McpOptions) -> CoreResult<Mcp> {
    let sectors = directions.map_or(1, |directions| directions.sectors);
    check_sectors(sectors)?;
    check_options(options)?;
    let layout = Layout::new(sectors, options);
    let fitting = Fitting { layout, options: *options, min_division: ((SECONDS_PER_DAY + period_s - 1) / period_s).max(MIN_DIVISION_PERIODS as i64) as usize };
    let concurrent: Vec<(i64, Sample)> = (site.first..site.first + site.means.len() as i64)
        .filter_map(|period| {
            let (x, y) = (reference.at(period), site.at(period));
            (!x.is_nan() && !y.is_nan()).then(|| (period, Sample { x, y, division: division_at(&layout, directions, period, period_s, x) }))
        })
        .collect();
    if concurrent.len() < MIN_CONCURRENT_PERIODS {
        return Err(CoreError::TooFewConcurrentPeriods { found: concurrent.len(), needed: MIN_CONCURRENT_PERIODS });
    }
    let samples: Vec<Sample> = concurrent.iter().map(|&(_, sample)| sample).collect();
    let pairs: Vec<(f64, f64)> = samples.iter().map(|sample| (sample.x, sample.y)).collect();
    let moments = Moments::of(&pairs);
    // Calendar month of each concurrent period, as the folds.
    let month_of = |period: i64| DateTime::from_timestamp(period * period_s, 0).map_or(0, |time| time.year() * MONTHS_PER_YEAR + time.month() as i32);
    let months: Vec<i32> = concurrent.iter().map(|&(period, _)| month_of(period)).collect();
    let mut fold_keys = months.clone();
    fold_keys.dedup();
    let reference_periods: Vec<(i64, f64, Option<usize>)> = reference
        .means
        .iter()
        .enumerate()
        .filter(|(_, value)| !value.is_nan())
        .map(|(index, &value)| {
            let period = reference.first + index as i64;
            (period, value, division_at(&layout, directions, period, period_s, value))
        })
        .collect();
    let mean_over_reference = |fitted: &Fitted| reference_periods.iter().map(|&(_, x, division)| fitted.predict(x, division)).sum::<f64>() / reference_periods.len() as f64;
    let results: Vec<McpResult> = MCP_METHODS
        .iter()
        .filter_map(|&method| {
            let fitted = fit_divided(method, &samples, &fitting)?;
            let (cv_rmse, cv_bias) = cross_validate(method, &samples, &fitting, &months, &fold_keys);
            let synthesized = (method == McpMethod::MatrixTimeSeries).then(|| matrix_series(&fitted, &reference_periods, site, period_s, &options.mts));
            let (long_term, predicted): (Vec<f64>, Vec<f64>) = match &synthesized {
                Some(series) => {
                    // The series' own values at the concurrent periods: the variance it keeps.
                    let concurrent_periods: std::collections::HashSet<i64> = concurrent.iter().map(|&(period, _)| period).collect();
                    let at_concurrent = reference_periods.iter().zip(series).filter(|((period, _, _), _)| concurrent_periods.contains(period)).map(|(_, &value)| value).collect();
                    (series.clone(), at_concurrent)
                }
                None => (
                    reference_periods.iter().map(|&(_, x, division)| fitted.predict(x, division)).collect(),
                    samples.iter().map(|sample| fitted.predict(sample.x, sample.division)).collect(),
                ),
            };
            let spread_ratio = standard_deviation(&predicted) / moments.site_deviation();
            let long_term_mean = long_term.iter().sum::<f64>() / long_term.len() as f64;
            let long_term_interval = confidence.then(|| month_interval(method, &samples, &fitting, &months, &fold_keys, &mean_over_reference)).flatten();
            let long_term_weibull = weibull::fit(&long_term, Method::MaxLikelihood).ok();
            let Fitted { overall: predictor, by_division: division_predictors } = fitted;
            Some(McpResult { method, predictor, cv_rmse, cv_bias, spread_ratio, long_term_mean, long_term_interval, division_predictors, long_term_weibull, synthesized })
        })
        .collect();
    if results.is_empty() {
        return Err(CoreError::McpMethodUnavailable);
    }
    let recommended = results
        .iter()
        .filter(|result| result.cv_rmse.is_finite())
        .min_by(|left, right| left.cv_rmse.total_cmp(&right.cv_rmse))
        .map_or(results[0].method, |result| result.method);
    let direction = directions.and_then(|directions| Some(direction_offset(directions.site?, directions.reference, site, reference)));
    Ok(Mcp {
        period_s,
        concurrent: pairs.len(),
        concurrent_span: (concurrent[0].0 * period_s, concurrent[concurrent.len() - 1].0 * period_s),
        correlation: moments.sxy / (moments.sxx * moments.syy).sqrt(),
        site_mean: moments.mean_y,
        reference_concurrent_mean: moments.mean_x,
        reference_long_term_mean: reference_periods.iter().map(|&(_, x, _)| x).sum::<f64>() / reference_periods.len() as f64,
        reference_periods: reference_periods.len(),
        folds: if fold_keys.len() >= MIN_FOLDS { fold_keys.len() } else { 0 },
        results,
        recommended,
        pairs,
        sectors,
        divisions: layout.groups(),
        direction: direction.flatten(),
    })
}

/// The matrix time series' site speed at every reference period of
/// `periods` (period, reference speed, subdivision): the percentile of the
/// site's speed where it has one, smoothed, the others drawn by the Markov
/// chain, each turned back through its bin's distribution.
fn matrix_series(fitted: &Fitted, periods: &[(i64, f64, Option<usize>)], site: &Averaged, period_s: i64, options: &MtsOptions) -> Vec<f64> {
    let matrix = |division: Option<usize>| match fitted.of(division) {
        Predictor::Matrix(fit) => Some(fit.as_ref()),
        _ => None,
    };
    let raw: Vec<f64> = periods
        .iter()
        .map(|&(period, x, division)| {
            let y = site.at(period);
            match matrix(division) {
                Some(fit) if !y.is_nan() => fit.percentile(x, y),
                _ => f64::NAN,
            }
        })
        .collect();
    // A centred moving average over the window, of the percentiles present.
    let window = ((options.window_h * 3600.0 / period_s as f64).round() as usize).max(1);
    let (before, after) = ((window - 1) / 2, window - 1 - (window - 1) / 2);
    let mut percentiles: Vec<f64> = (0..raw.len())
        .map(|index| {
            if raw[index].is_nan() {
                return f64::NAN;
            }
            let around = &raw[index.saturating_sub(before)..(index + after + 1).min(raw.len())];
            let present: Vec<f64> = around.iter().copied().filter(|value| !value.is_nan()).collect();
            present.iter().sum::<f64>() / present.len() as f64
        })
        .collect();
    let gaps: Vec<usize> = (0..percentiles.len()).filter(|&index| percentiles[index].is_nan()).collect();
    crate::edits::markov_fill(&mut percentiles, &gaps, MTS_PERCENTILE_BIN, MTS_SEED);
    periods
        .iter()
        .zip(&percentiles)
        .map(|(&(_, x, division), &p)| {
            let p = if p.is_nan() { MEDIAN_PERCENTILE } else { p };
            matrix(division).map_or_else(|| fitted.predict(x, division), |fit| fit.quantile(x, p)).max(0.0)
        })
        .collect()
}

/// The site's direction as the reference's plus their circular mean
/// difference over the periods where both directions and both speeds have
/// data; `None` without such a period.
fn direction_offset(site_direction: &Averaged, reference_direction: &Averaged, site: &Averaged, reference: &Averaged) -> Option<DirectionOffset> {
    let (mut sum_sin, mut sum_cos, mut concurrent) = (0.0, 0.0, 0usize);
    let mut site_counts = [0usize; ROSE_SECTORS];
    for period in site_direction.first..site_direction.first + site_direction.means.len() as i64 {
        let (to, from) = (site_direction.at(period), reference_direction.at(period));
        if to.is_nan() || from.is_nan() || site.at(period).is_nan() || reference.at(period).is_nan() {
            continue;
        }
        let (sin, cos) = sincos_deg(to - from);
        (sum_sin, sum_cos, concurrent) = (sum_sin + sin, sum_cos + cos, concurrent + 1);
        site_counts[sector_of(to, ROSE_SECTORS)] += 1;
    }
    if concurrent == 0 {
        return None;
    }
    let offset_deg = sum_sin.atan2(sum_cos).to_degrees();
    let mut long_term_counts = [0usize; ROSE_SECTORS];
    let long_term: Vec<f64> = reference_direction.means.iter().copied().filter(|direction| !direction.is_nan()).collect();
    for direction in &long_term {
        long_term_counts[sector_of((direction + offset_deg).rem_euclid(DEGREES_PER_TURN), ROSE_SECTORS)] += 1;
    }
    let shares = |counts: [usize; ROSE_SECTORS], total: usize| counts.iter().map(|&count| count as f64 / total as f64).collect();
    Some(DirectionOffset { offset_deg, concurrent, site_frequency: shares(site_counts, concurrent), long_term_frequency: shares(long_term_counts, long_term.len()) })
}

/// The site predicted over every reference period with data by `result`
/// (by subdivision when it has subdivision fits, as `options` laid them
/// out): `(start of the period, epoch s, speed, direction)`, the direction
/// the reference's turned by `offset_deg`, NaN without one.
pub fn synthesize(reference: &Averaged, directions: Option<Directions>, period_s: i64, result: &McpResult, offset_deg: Option<f64>, options: &McpOptions) -> Vec<(i64, f64, f64)> {
    let layout = Layout::new(directions.map_or(1, |directions| directions.sectors), options);
    reference
        .means
        .iter()
        .enumerate()
        .filter(|(_, value)| !value.is_nan())
        .enumerate()
        .map(|(rank, (index, &value))| {
            let period = reference.first + index as i64;
            let direction = directions.map_or(f64::NAN, |directions| directions.reference.at(period));
            let division = division_at(&layout, directions, period, period_s, value);
            let turned = offset_deg.map_or(f64::NAN, |offset| (direction + offset).rem_euclid(DEGREES_PER_TURN));
            // The matrix time series carries its own series, one value per reference period with data.
            let speed = result.synthesized.as_ref().map_or_else(|| result.predict(value, division), |series| series[rank]);
            (period * period_s, speed, turned)
        })
        .collect()
}

impl McpResult {
    /// The site's speed predicted from reference speed `reference` in
    /// subdivision `division` (`None`: none, the overall fit).
    pub fn predict(&self, reference: f64, division: Option<usize>) -> f64 {
        division.and_then(|division| self.division_predictors.get(division)?.as_ref()).unwrap_or(&self.predictor).predict(reference)
    }
}

/// Means and centred sums of a set of pairs (two passes: stable far from 0).
struct Moments {
    count: f64,
    mean_x: f64,
    mean_y: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl Moments {
    fn of(pairs: &[(f64, f64)]) -> Self {
        let count = pairs.len() as f64;
        let (sum_x, sum_y) = pairs.iter().fold((0.0, 0.0), |(x, y), &(px, py)| (x + px, y + py));
        let (mean_x, mean_y) = (sum_x / count, sum_y / count);
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for &(x, y) in pairs {
            let (dx, dy) = (x - mean_x, y - mean_y);
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        Moments { count, mean_x, mean_y, sxx, syy, sxy }
    }

    /// The sums about the origin rather than the means: lines through 0.
    fn about_origin(pairs: &[(f64, f64)]) -> Self {
        let count = pairs.len() as f64;
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for &(x, y) in pairs {
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        Moments { count, mean_x: 0.0, mean_y: 0.0, sxx, syy, sxy }
    }

    fn site_deviation(&self) -> f64 {
        (self.syy / (self.count - 1.0)).sqrt()
    }
}

/// The fit of `method` to `pairs` (through the origin with `zero_intercept`,
/// for the lines that have an intercept); `None` when the method cannot
/// draw one (a Weibull without a solution, too few slices) or the data leave
/// its line undefined (a reference without spread, site and reference
/// uncorrelated for the orthogonal line, a reference mean of 0).
fn fit(method: McpMethod, pairs: &[(f64, f64)], options: &McpOptions) -> Option<Predictor> {
    let zero_intercept = options.zero_intercept;
    let moments_of = |pairs: &[(f64, f64)]| if zero_intercept { Moments::about_origin(pairs) } else { Moments::of(pairs) };
    let line_through_means = |moments: &Moments, slope: f64| Predictor::Line(McpLine { slope, intercept: moments.mean_y - slope * moments.mean_x });
    let spread = |moments: Moments| (moments.sxx > 0.0).then_some(moments);
    Some(match method {
        McpMethod::LeastSquares => {
            let moments = spread(moments_of(pairs))?;
            line_through_means(&moments, moments.sxy / moments.sxx)
        }
        McpMethod::TotalLeastSquares => {
            let moments = spread(moments_of(pairs)).filter(|moments| moments.sxy != 0.0)?;
            let Moments { sxx, syy, sxy, .. } = moments;
            line_through_means(&moments, (syy - sxx + ((syy - sxx).powi(2) + 4.0 * sxy * sxy).sqrt()) / (2.0 * sxy))
        }
        McpMethod::VarianceRatio => {
            let moments = spread(moments_of(pairs))?;
            line_through_means(&moments, (moments.syy / moments.sxx).sqrt().copysign(moments.sxy))
        }
        McpMethod::SpeedRatio => {
            let moments = Moments::of(pairs);
            (moments.mean_x != 0.0).then_some(())?;
            Predictor::Line(McpLine { slope: moments.mean_y / moments.mean_x, intercept: 0.0 })
        }
        McpMethod::SpeedSort => {
            let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = pairs.iter().copied().unzip();
            xs.sort_unstable_by(f64::total_cmp);
            ys.sort_unstable_by(f64::total_cmp);
            let sorted: Vec<(f64, f64)> = xs.into_iter().zip(ys).collect();
            let moments = spread(moments_of(&sorted))?;
            line_through_means(&moments, moments.sxy / moments.sxx)
        }
        McpMethod::WeibullFit => {
            let (xs, ys): (Vec<f64>, Vec<f64>) = pairs.iter().copied().unzip();
            let (reference, site) = (weibull::fit(&xs, Method::MaxLikelihood).ok()?, weibull::fit(&ys, Method::MaxLikelihood).ok()?);
            Predictor::Weibull { reference, site }
        }
        McpMethod::VerticalSlice => {
            let mut slices: std::collections::BTreeMap<i64, (usize, f64, f64)> = std::collections::BTreeMap::new();
            for &(x, y) in pairs {
                let slice = slices.entry((x / SLICE_WIDTH_M_S).floor() as i64).or_default();
                *slice = (slice.0 + 1, slice.1 + x, slice.2 + y);
            }
            let points: Vec<(f64, f64)> = slices
                .values()
                .filter(|(count, _, _)| *count >= MIN_SLICE_PAIRS)
                .map(|&(count, x, y)| (x / count as f64, y / count as f64))
                .collect();
            if points.len() < MIN_SLICES {
                return None;
            }
            Predictor::Slices(points)
        }
        McpMethod::MatrixTimeSeries => {
            let McpOptions { mts, .. } = *options;
            let Predictor::Line(fallback) = fit(McpMethod::LeastSquares, pairs, options)? else { return None };
            let (reference_bins, target_bins) = pairs.iter().fold((0usize, 0usize), |(r, t), &(x, y)| {
                (r.max((x.max(0.0) / mts.reference_bin) as usize + 1), t.max((y.max(0.0) / mts.target_bin) as usize + 1))
            });
            let mut counts = vec![vec![0.0; target_bins]; reference_bins];
            for &(x, y) in pairs {
                counts[(x.max(0.0) / mts.reference_bin) as usize][(y.max(0.0) / mts.target_bin) as usize] += 1.0;
            }
            let cumulative = counts
                .into_iter()
                .map(|column| {
                    let mut total = 0.0;
                    let running: Vec<f64> = column.iter().map(|count| {
                        total += count;
                        total
                    }).collect();
                    (total >= MTS_MIN_BIN_PAIRS as f64).then_some(running)
                })
                .collect();
            Predictor::Matrix(Box::new(MatrixFit { reference_bin: mts.reference_bin, target_bin: mts.target_bin, cumulative, fallback }))
        }
    })
}

/// Whether the pair `(x, y)` enters `method`'s fits under `fitting`'s cutoff:
/// the speed ratio leaves out a pair with either speed below it; without
/// sectors every method leaves out a reference speed below it (with sectors
/// those periods form their own group instead).
fn enters_fit(method: McpMethod, fitting: &Fitting, x: f64, y: f64) -> bool {
    let Some(cutoff) = fitting.options.cutoff_m_s else { return true };
    if method == McpMethod::SpeedRatio && (x < cutoff || y < cutoff) {
        return false;
    }
    fitting.layout.sectors > 1 || x >= cutoff
}

/// `method` fitted to all `samples` and, with subdivisions, to the samples
/// of each subdivision that has enough of them.
fn fit_divided(method: McpMethod, samples: &[Sample], fitting: &Fitting) -> Option<Fitted> {
    let pairs_where = |keep: &dyn Fn(&Sample) -> bool| {
        samples.iter().filter(|sample| keep(sample) && enters_fit(method, fitting, sample.x, sample.y)).map(|sample| (sample.x, sample.y)).collect::<Vec<_>>()
    };
    let options = &fitting.options;
    let overall = fit(method, &pairs_where(&|_| true), options)?;
    let groups = fitting.layout.groups();
    let by_division = if groups > 1 {
        (0..groups)
            .map(|division| {
                let pairs = pairs_where(&|sample| sample.division == Some(division));
                (pairs.len() >= fitting.min_division).then(|| fit(method, &pairs, options)).flatten()
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Fitted { overall, by_division })
}

/// The samples of the months `kept` asks for, each as many times as it says.
fn samples_of_months(samples: &[Sample], months: &[i32], kept: impl Fn(i32) -> usize) -> Vec<Sample> {
    let mut chosen = Vec::with_capacity(samples.len());
    for (&sample, &month) in samples.iter().zip(months) {
        for _ in 0..kept(month) {
            chosen.push(sample);
        }
    }
    chosen
}

/// Leave-one-month-out errors of the monthly means: `(rmse, bias)`, NaN
/// with fewer than [`MIN_FOLDS`] months.
fn cross_validate(method: McpMethod, samples: &[Sample], fitting: &Fitting, months: &[i32], folds: &[i32]) -> (f64, f64) {
    if folds.len() < MIN_FOLDS {
        return (f64::NAN, f64::NAN);
    }
    let errors: Vec<f64> = folds
        .iter()
        .filter_map(|&left_out| {
            let training = samples_of_months(samples, months, |month| usize::from(month != left_out));
            if training.len() < MIN_CONCURRENT_PERIODS {
                return None;
            }
            let fitted = fit_divided(method, &training, fitting)?;
            let held = samples_of_months(samples, months, |month| usize::from(month == left_out));
            let count = held.len() as f64;
            let predicted = held.iter().map(|sample| fitted.predict(sample.x, sample.division)).sum::<f64>() / count;
            let observed = held.iter().map(|sample| sample.y).sum::<f64>() / count;
            Some(predicted - observed)
        })
        .collect();
    if errors.len() < MIN_FOLDS {
        return (f64::NAN, f64::NAN);
    }
    let count = errors.len() as f64;
    ((errors.iter().map(|error| error * error).sum::<f64>() / count).sqrt(), errors.iter().sum::<f64>() / count)
}

/// The bootstrap interval of the long-term mean: the concurrent months
/// drawn with replacement, the method refitted, the reference record
/// predicted. `None` with fewer than [`MIN_FOLDS`] months.
fn month_interval(
    method: McpMethod,
    samples: &[Sample],
    fitting: &Fitting,
    months: &[i32],
    folds: &[i32],
    mean_over_reference: &impl Fn(&Fitted) -> f64,
) -> Option<Interval> {
    if folds.len() < MIN_FOLDS {
        return None;
    }
    let mut replicates = Vec::with_capacity(bootstrap::RESAMPLES);
    bootstrap::each_replicate(folds.len(), |draws| {
        let drawn = samples_of_months(samples, months, |month| folds.binary_search(&month).map_or(0, |fold| draws[fold] as usize));
        let fitted = (drawn.len() >= MIN_CONCURRENT_PERIODS).then(|| fit_divided(method, &drawn, fitting)).flatten();
        replicates.push(fitted.map_or(f64::NAN, |fitted| mean_over_reference(&fitted)));
    });
    bootstrap::percentile_interval(&replicates)
}

/// One quantity's correlation between the site and the reference shifted
/// (the original's optimal time offset analysis, manual
/// def-optimal-time-offset-analysis.htm).
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetCorrelation {
    /// Shifts tried, in periods: shift `s` sets the reference's period
    /// `p − s` against the site's `p`, i.e. moves the reference `s` periods later.
    pub shifts: Vec<i64>,
    /// Correlation at each shift; NaN with fewer than
    /// [`MIN_CONCURRENT_PERIODS`] pairs.
    pub correlations: Vec<f64>,
    /// The shift of the highest correlation (the nearest to 0 on a tie);
    /// `None` without any.
    pub best: Option<i64>,
}

/// Pearson correlation of `pairs`, or, for angles (`circular`), the
/// circular one (Jammalamadaka and SenGupta: sines of the deviations from
/// each side's circular mean). NaN with too few pairs or no spread.
fn correlation_of(pairs: &[(f64, f64)], circular: bool) -> f64 {
    if pairs.len() < MIN_CONCURRENT_PERIODS {
        return f64::NAN;
    }
    if !circular {
        let moments = Moments::of(pairs);
        return moments.sxy / (moments.sxx * moments.syy).sqrt();
    }
    let mean_of = |angles: &mut dyn Iterator<Item = f64>| {
        let (sin, cos) = angles.fold((0.0, 0.0), |(sin, cos), angle| {
            let (s, c) = sincos_deg(angle);
            (sin + s, cos + c)
        });
        sin.atan2(cos)
    };
    let (mean_x, mean_y) = (mean_of(&mut pairs.iter().map(|pair| pair.0)), mean_of(&mut pairs.iter().map(|pair| pair.1)));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for &(x, y) in pairs {
        let (dx, dy) = ((x.to_radians() - mean_x).sin(), (y.to_radians() - mean_y).sin());
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    sxy / (sxx * syy).sqrt()
}

/// The correlation of `site` with `reference` shifted by every whole number
/// of periods from −`max_shift` to `max_shift`; `circular` for directions.
/// O(periods × shifts).
pub fn offset_correlation(site: &Averaged, reference: &Averaged, circular: bool, max_shift: i64) -> OffsetCorrelation {
    let shifts: Vec<i64> = (-max_shift..=max_shift).collect();
    let correlations: Vec<f64> = shifts
        .iter()
        .map(|&shift| {
            let pairs: Vec<(f64, f64)> = (site.first..site.first + site.means.len() as i64)
                .filter_map(|period| {
                    let (x, y) = (reference.at(period - shift), site.at(period));
                    (!x.is_nan() && !y.is_nan()).then_some((x, y))
                })
                .collect();
            correlation_of(&pairs, circular)
        })
        .collect();
    let mut by_closeness: Vec<usize> = (0..shifts.len()).collect();
    by_closeness.sort_by_key(|&index| shifts[index].abs());
    let best = by_closeness
        .into_iter()
        .filter(|&index| correlations[index].is_finite())
        .reduce(|best, index| if correlations[index] > correlations[best] { index } else { best })
        .map(|index| shifts[index]);
    OffsetCorrelation { shifts, correlations, best }
}

/// Sample standard deviation.
fn standard_deviation(values: &[f64]) -> f64 {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    (values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (count - 1.0)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// x̄ = 2.5, ȳ = 3.5; Sxx = 5, Syy = 5, Sxy = 4.
    const PAIRS: [(f64, f64); 4] = [(1.0, 2.0), (2.0, 3.0), (3.0, 5.0), (4.0, 4.0)];

    fn line_of(method: McpMethod, pairs: &[(f64, f64)]) -> McpLine {
        fit(method, pairs, &McpOptions::default()).and_then(|predictor| predictor.line()).unwrap()
    }

    #[test]
    fn the_lines_on_hand_checked_pairs() {
        let close = |line: McpLine, slope: f64, intercept: f64| (line.slope - slope).abs() < 1e-12 && (line.intercept - intercept).abs() < 1e-12;
        assert!(close(line_of(McpMethod::LeastSquares, &PAIRS), 0.8, 1.5), "Sxy/Sxx");
        // Equal spreads: the orthogonal line and the variance ratio both have slope 1.
        assert!(close(line_of(McpMethod::TotalLeastSquares, &PAIRS), 1.0, 1.0));
        assert!(close(line_of(McpMethod::VarianceRatio, &PAIRS), 1.0, 1.0));
        assert!(close(line_of(McpMethod::SpeedRatio, &PAIRS), 1.4, 0.0), "3.5 / 2.5");
        // Sorted, the site reads 2, 3, 4, 5 against 1, 2, 3, 4: slope 1, intercept 1.
        assert!(close(line_of(McpMethod::SpeedSort, &PAIRS), 1.0, 1.0));
        assert_eq!(Predictor::Line(McpLine { slope: 1.0, intercept: -5.0 }).predict(2.0), 0.0, "never below 0");
    }

    #[test]
    fn degenerate_data_draw_no_line_and_no_adjustment() {
        let flat_reference: Vec<(f64, f64)> = (0..20).map(|index| (5.0, 4.0 + index as f64 * 0.1)).collect();
        for method in [McpMethod::LeastSquares, McpMethod::TotalLeastSquares, McpMethod::VarianceRatio, McpMethod::SpeedSort] {
            assert!(fit(method, &flat_reference, &McpOptions::default()).is_none(), "{method:?} on a reference without spread");
        }
        let uncorrelated = [(1.0, 5.0), (2.0, 4.0), (3.0, 4.0), (4.0, 5.0)];
        assert!(fit(McpMethod::TotalLeastSquares, &uncorrelated, &McpOptions::default()).is_none(), "Sxy = 0: no orthogonal line");
        assert!(fit(McpMethod::SpeedRatio, &[(0.0, 1.0), (0.0, 2.0)], &McpOptions::default()).is_none(), "a reference mean of 0");
        let averaged = |values: Vec<f64>| Averaged { first: 0, means: values };
        let site = averaged((0..20).map(|index| 4.0 + index as f64 * 0.1).collect());
        let constant = mcp(&site, &averaged(vec![5.0; 20]), None, 3600, false, &McpOptions::default()).unwrap();
        let fitted: Vec<McpMethod> = constant.results.iter().map(|result| result.method).collect();
        assert_eq!(fitted, [McpMethod::SpeedRatio], "a constant reference only scales");
        assert_eq!(constant.recommended, McpMethod::SpeedRatio, "the recommendation is one of the results");
        let error = mcp(&site, &averaged(vec![0.0; 20]), None, 3600, false, &McpOptions::default()).unwrap_err();
        assert_eq!(error, CoreError::McpMethodUnavailable, "no method fits a calm reference");
    }

    #[test]
    fn the_options_divide_cut_and_pass_through_the_origin() {
        assert!(close_line(line_through_origin(McpMethod::LeastSquares, &PAIRS), 39.0 / 30.0, 0.0), "Σxy/Σx² = 39/30");
        // A year of daily periods: the site is 1.5 × the reference in the first half, 2 × in the second.
        let days = 365;
        let reference: Vec<f64> = (0..days).map(|day| 4.0 + (day % 7) as f64).collect();
        let site: Vec<f64> = reference.iter().enumerate().map(|(day, x)| if day < 181 { 1.5 * x } else { 2.0 * x }).collect();
        let averaged = |values: Vec<f64>| Averaged { first: 0, means: values };
        let halves = McpOptions { yearly_divisions: 2, zero_intercept: true, ..McpOptions::default() };
        let found = mcp(&averaged(site.clone()), &averaged(reference.clone()), None, SECONDS_PER_DAY, false, &halves).unwrap();
        assert_eq!(found.divisions, 2);
        let speed_ratio = found.results.iter().find(|result| result.method == McpMethod::LeastSquares).unwrap();
        assert!((speed_ratio.predict(10.0, Some(0)) - 15.0).abs() < 1e-9 && (speed_ratio.predict(10.0, Some(1)) - 20.0).abs() < 1e-9);
        assert!(mcp(&averaged(site.clone()), &averaged(reference.clone()), None, SECONDS_PER_DAY, false, &McpOptions { yearly_divisions: 5, ..McpOptions::default() }).is_err());
        // Without sectors a cutoff only thins the fit: the 9 and 10 m/s days, of both halves.
        let cut = McpOptions { cutoff_m_s: Some(8.5), ..McpOptions::default() };
        let found = mcp(&averaged(site), &averaged(reference), None, SECONDS_PER_DAY, false, &cut).unwrap();
        let line = found.results.iter().find(|result| result.method == McpMethod::LeastSquares).unwrap().predictor.line().unwrap();
        assert!((line.slope * 10.0 + line.intercept - 17.5).abs() < 0.5, "1.75 × 10, the mean of the two slopes: {line:?}");
    }

    fn close_line(line: McpLine, slope: f64, intercept: f64) -> bool {
        (line.slope - slope).abs() < 1e-12 && (line.intercept - intercept).abs() < 1e-12
    }

    fn line_through_origin(method: McpMethod, pairs: &[(f64, f64)]) -> McpLine {
        fit(method, pairs, &McpOptions { zero_intercept: true, ..McpOptions::default() }).and_then(|predictor| predictor.line()).unwrap()
    }

    #[test]
    fn the_matrix_keeps_the_distribution_and_the_record() {
        // The site is the reference × 1.2 plus a spread that depends on the period.
        let periods = 2000;
        let reference: Vec<f64> = (0..periods).map(|period| 2.0 + (period % 11) as f64 + 0.3 * ((period / 11) % 3) as f64).collect();
        let site: Vec<f64> = reference.iter().enumerate().map(|(period, x)| 1.2 * x + [-0.6, 0.0, 0.6][period % 3]).collect();
        let averaged = |values: Vec<f64>| Averaged { first: 0, means: values };
        // The site stops halfway; the reference goes on.
        let mut site_half = site.clone();
        site_half[periods / 2..].fill(f64::NAN);
        let options = McpOptions { mts: MtsOptions { window_h: 1.0, ..MtsOptions::default() }, ..McpOptions::default() };
        let found = mcp(&averaged(site_half), &averaged(reference.clone()), None, 3600, false, &options).unwrap();
        let matrix = found.results.iter().find(|result| result.method == McpMethod::MatrixTimeSeries).unwrap();
        let series = matrix.synthesized.as_ref().unwrap();
        // Where the site measured, the series gives it back (no smoothing).
        assert!(series[..periods / 2].iter().zip(&site).all(|(synthesized, observed)| (synthesized - observed).abs() < 1e-9));
        // Its spread is the site's, where a line would shrink it.
        assert!((matrix.spread_ratio - 1.0).abs() < 1e-9, "{}", matrix.spread_ratio);
        assert!((matrix.long_term_mean - site.iter().sum::<f64>() / periods as f64).abs() < 0.1, "{}", matrix.long_term_mean);
        let Predictor::Matrix(fit) = &matrix.predictor else { panic!() };
        assert!((fit.quantile(5.5, fit.percentile(5.5, 6.3)) - 6.3).abs() < 1e-9, "the percentile and the quantile undo each other");
        assert_eq!(fit.quantile(500.0, 0.5), fit.fallback.slope * 500.0 + fit.fallback.intercept, "an unseen bin uses the line");
    }

    #[test]
    fn the_time_offset_finds_a_lagging_reference() {
        // The reference is the site three periods earlier.
        let wave: Vec<f64> = (0..200).map(|period| 6.0 + 3.0 * (period as f64 * 0.37).sin() + (period % 5) as f64 * 0.2).collect();
        let site = Averaged { first: 0, means: wave.clone() };
        let reference = Averaged { first: -3, means: wave };
        let found = offset_correlation(&site, &reference, false, 6);
        assert_eq!(found.best, Some(3), "move the reference 3 periods later");
        assert!((found.correlations[found.shifts.iter().position(|&shift| shift == 3).unwrap()] - 1.0).abs() < 1e-12);
        // Directions through north correlate circularly.
        let directions: Vec<f64> = (0..100).map(|period| (350.0 + period as f64 * 7.0) % 360.0).collect();
        let turned = Averaged { first: 0, means: directions.iter().map(|direction| (direction + 20.0) % 360.0).collect() };
        let circular = offset_correlation(&Averaged { first: 0, means: directions }, &turned, true, 0);
        assert!((circular.correlations[0] - 1.0).abs() < 1e-9, "{:?}", circular.correlations);
    }

    #[test]
    fn slices_join_their_means_and_extend_at_the_ends() {
        let slices = Predictor::Slices(vec![(1.5, 2.0), (2.5, 4.0), (3.5, 5.0)]);
        assert_eq!(slices.predict(2.0), 3.0, "halfway along the first segment");
        assert_eq!(slices.predict(3.0), 4.5);
        assert_eq!(slices.predict(4.5), 6.0, "the last segment, extended");
        assert_eq!(slices.predict(0.5), 0.0, "the first, extended, then never below 0");
    }

    #[test]
    fn the_weibull_mapping_keeps_probabilities() {
        let reference = Weibull { k: 2.0, a: 8.0 };
        let site = Weibull { k: 2.5, a: 10.0 };
        let mapping = Predictor::Weibull { reference, site };
        // At x = A_r the probability is 1 − e⁻¹ on both sides, so y = A_s.
        assert!((mapping.predict(8.0) - 10.0).abs() < 1e-12);
        let probability = |fit: Weibull, value: f64| 1.0 - (-(value / fit.a).powf(fit.k)).exp();
        assert!((probability(reference, 5.0) - probability(site, mapping.predict(5.0))).abs() < 1e-12);
    }
}

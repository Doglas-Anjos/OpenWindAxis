//! Analisar › Ventos extremos on the wire: the request, the answer, and the
//! call every adapter answers through ([`wind_core::analysis::extremes`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::column_index;
use crate::responses::IntervalDto;
use crate::DatasetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis::{self, ExtremePeriod, Extremes, Gumbel, GumbelFit, PeakMethod, DEFAULT_INDEPENDENCE_S, DEFAULT_MIN_RECOVERY, DESIGN_RETURN_YEARS};
use wind_core::error::CoreResult;
use wind_core::model::Dataset;
use wind_core::time::SECONDS_PER_HOUR;

/// Milliseconds in a second: instants on the wire are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;
/// Return periods of the drawn return-level curve, years: from just over one
/// year (the curve diverges at one) to two centuries.
const CURVE_YEARS: [f64; 16] = [1.01, 1.05, 1.1, 1.25, 1.5, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0, 50.0, 100.0, 200.0];

/// Period of the periodic maxima.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExtremePeriodDto {
    /// Calendar month.
    Month,
    /// Calendar year.
    Year,
}

/// How the peaks are found, tagged by `kind`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum PeakMethodDto {
    /// The maximum of each calendar month or year with enough data.
    PeriodicMaxima {
        /// The period.
        period: ExtremePeriodDto,
        /// Share of the period's steps that must hold data, 0–1; omitted = 0.5.
        #[serde(default)]
        #[ts(optional)]
        min_recovery: Option<f64>,
    },
    /// The peak of each independent storm.
    IndependentStorms {
        /// Storm threshold, in the column's units; omitted = the one that
        /// gives about 20 storms a year.
        #[serde(default)]
        #[ts(optional)]
        threshold: Option<f64>,
        /// Storms closer than this merge, hours; omitted = 48.
        #[serde(default)]
        #[ts(optional)]
        independence_hours: Option<f64>,
    },
}

/// How the Gumbel distribution is fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GumbelFitDto {
    /// Maximum likelihood: the default.
    #[default]
    MaxLikelihood,
    /// Least squares on Gringorten's plotting positions.
    Gringorten,
}

/// The 50-year wind of one column (a mean speed, or its gust or maximum).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtremesRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// How the peaks are found.
    pub method: PeakMethodDto,
    /// Omitted = maximum likelihood.
    #[serde(default)]
    #[ts(optional)]
    pub fit: Option<GumbelFitDto>,
    /// Add the bootstrap interval of the 50-year wind.
    #[serde(default)]
    #[ts(optional)]
    pub confidence: Option<bool>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// One peak.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PeakDto {
    /// Start of the peak's step, epoch ms.
    pub time_ms: f64,
    /// Its value.
    pub value: f64,
    /// Start of the period or storm, epoch ms.
    pub from_ms: f64,
    /// End of the period or storm (after its last step), epoch ms.
    pub to_ms: f64,
    /// Share of the period's steps with data; 1 for a storm.
    pub recovery: f64,
}

/// A Gumbel distribution: mode μ and scale β.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GumbelDto {
    /// μ.
    pub mode: f64,
    /// β.
    pub scale: f64,
}

impl From<Gumbel> for GumbelDto {
    fn from(gumbel: Gumbel) -> Self {
        GumbelDto { mode: gumbel.mode, scale: gumbel.scale }
    }
}

/// A value and its return period.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLevelDto {
    /// Years.
    pub years: f64,
    /// The value exceeded once in that many years on average.
    pub value: f64,
}

/// The answer to an [`ExtremesRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtremesDto {
    /// The peaks, in time order.
    pub peaks: Vec<PeakDto>,
    /// Peaks per year.
    pub per_year: f64,
    /// The storm threshold used (independent storms only).
    pub threshold: Option<f64>,
    /// The fit to the peaks.
    pub fit: GumbelDto,
    /// The distribution of annual maxima.
    pub annual: GumbelDto,
    /// Return period of `design`, years (50).
    pub design_years: f64,
    /// The 50-year value.
    pub design: f64,
    /// Its bootstrap interval, when asked for and the peaks allow one.
    pub interval: Option<IntervalDto>,
    /// The return-level table.
    pub return_levels: Vec<ReturnLevelDto>,
    /// The fitted curve, for drawing.
    pub curve: Vec<ReturnLevelDto>,
    /// Each peak with its empirical annual return period, ascending.
    pub empirical: Vec<ReturnLevelDto>,
}

impl ExtremesDto {
    fn new(dataset: &Dataset, extremes: Extremes) -> Self {
        let at = |step: usize| dataset.time_at(step) as f64 * MS_PER_SECOND;
        let level = |(years, value): (f64, f64)| ReturnLevelDto { years, value };
        ExtremesDto {
            peaks: extremes
                .peaks
                .iter()
                .map(|peak| PeakDto { time_ms: at(peak.step), value: peak.value, from_ms: at(peak.span.start), to_ms: at(peak.span.end), recovery: peak.recovery })
                .collect(),
            per_year: extremes.per_year,
            threshold: extremes.threshold,
            fit: extremes.fit.into(),
            annual: extremes.annual.into(),
            design_years: DESIGN_RETURN_YEARS,
            design: extremes.design,
            interval: extremes.interval.map(Into::into),
            return_levels: extremes.return_levels.iter().copied().map(level).collect(),
            curve: CURVE_YEARS.iter().map(|&years| ReturnLevelDto { years, value: extremes.annual.return_level(years) }).collect(),
            empirical: extremes.empirical.iter().map(|&(value, years)| ReturnLevelDto { years, value }).collect(),
        }
    }
}

/// The extreme winds of the request's column.
///
/// # Errors
/// An unknown column, the filter's, or the analysis's own.
pub fn extremes(prepared: Prepared, request: ExtremesRequest) -> CoreResult<ExtremesDto> {
    let data = prepared.data;
    let column = column_index(data, &request.column)?;
    let method = match request.method {
        PeakMethodDto::PeriodicMaxima { period, min_recovery } => PeakMethod::PeriodicMaxima {
            period: match period {
                ExtremePeriodDto::Month => ExtremePeriod::Month,
                ExtremePeriodDto::Year => ExtremePeriod::Year,
            },
            min_recovery: min_recovery.unwrap_or(DEFAULT_MIN_RECOVERY),
        },
        PeakMethodDto::IndependentStorms { threshold, independence_hours } => PeakMethod::IndependentStorms {
            threshold,
            independence_s: independence_hours.map_or(DEFAULT_INDEPENDENCE_S, |hours| (hours * SECONDS_PER_HOUR as f64).round() as i64),
        },
    };
    let fit = match request.fit.unwrap_or_default() {
        GumbelFitDto::MaxLikelihood => GumbelFit::MaxLikelihood,
        GumbelFitDto::Gringorten => GumbelFit::Gringorten,
    };
    let view = prepared.view(request.filter)?;
    let extremes = analysis::extremes(data, &view, prepared.stats.months(), column, method, fit, request.confidence.unwrap_or(false))?;
    Ok(ExtremesDto::new(data, extremes))
}

//! Revisar › Ajuste de longo prazo on the wire ([`wind_core::mcp`]): a site
//! column and a reference column, each from a dataset of the session.
use crate::analyses::Prepared;
use crate::ops::column_index;
use crate::responses::IntervalDto;
use crate::{finite, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::{CoreError, CoreResult};
use wind_core::mcp::{self, Averaged, Directions, McpMethod, McpOptions, McpResult};
use wind_core::time::{format_utc, SECONDS_PER_DAY, SECONDS_PER_HOUR};

/// Milliseconds in a second: instants on the wire are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;
/// Concurrent pairs returned for the scatter, at most: every k-th one.
pub const MAX_MCP_PAIRS: usize = 5_000;
/// Points of each method's drawn curve.
const CURVE_POINTS: usize = 40;
/// Time stamps of the synthesized series' CSV: ISO 8601, UTC.
const SERIES_TIME_FORMAT: &str = "%Y-%m-%d %H:%M";

/// The averaging period of the concurrent comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AveragingDto {
    /// Hourly means.
    Hour,
    /// Daily means.
    Day,
}

impl AveragingDto {
    fn seconds(self) -> i64 {
        match self {
            AveragingDto::Hour => SECONDS_PER_HOUR,
            AveragingDto::Day => SECONDS_PER_DAY,
        }
    }
}

/// A speed method, as [`McpMethod`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum McpMethodDto {
    /// Ordinary least squares.
    LeastSquares,
    /// Total (orthogonal) least squares.
    TotalLeastSquares,
    /// Slope σ_y/σ_x through the means.
    VarianceRatio,
    /// Slope ȳ/x̄ through the origin.
    SpeedRatio,
    /// Least squares of the sorted speeds.
    SpeedSort,
    /// Mapping through the two Weibull fits.
    WeibullFit,
    /// Mean site speed per 1 m/s slice of the reference.
    VerticalSlice,
    /// The joint distribution's percentiles carried in time.
    MatrixTimeSeries,
}

impl From<McpMethod> for McpMethodDto {
    fn from(method: McpMethod) -> Self {
        match method {
            McpMethod::LeastSquares => McpMethodDto::LeastSquares,
            McpMethod::TotalLeastSquares => McpMethodDto::TotalLeastSquares,
            McpMethod::VarianceRatio => McpMethodDto::VarianceRatio,
            McpMethod::SpeedRatio => McpMethodDto::SpeedRatio,
            McpMethod::SpeedSort => McpMethodDto::SpeedSort,
            McpMethod::WeibullFit => McpMethodDto::WeibullFit,
            McpMethod::VerticalSlice => McpMethodDto::VerticalSlice,
            McpMethod::MatrixTimeSeries => McpMethodDto::MatrixTimeSeries,
        }
    }
}

/// A column of a dataset of the session.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpColumnDto {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
}

/// The direction columns of an adjustment.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpDirectionsDto {
    /// The reference's direction column id.
    pub reference: String,
    /// The site's direction column id, for the direction offset.
    #[serde(default)]
    #[ts(optional)]
    pub site: Option<String>,
    /// Sectors of the speed fits by reference direction; omitted = 1.
    #[serde(default)]
    #[ts(optional)]
    pub sectors: Option<usize>,
}

/// The site's direction as the reference's plus a constant.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DirectionOffsetDto {
    /// Site minus reference, °.
    pub offset_deg: f64,
    /// Concurrent periods with both directions.
    pub concurrent: usize,
    /// Frequency of the site's direction per sector (16), concurrent periods.
    pub site_frequency: Vec<f64>,
    /// Frequency of the predicted long-term direction per sector.
    pub long_term_frequency: Vec<f64>,
}

/// How the speed fits are made (manual articles-mcp-*); every field may be omitted.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOptionsDto {
    /// Groups of calendar months fitted apart: 1, 2, 3, 4, 6 or 12; omitted = 1.
    #[serde(default)]
    #[ts(optional)]
    pub yearly_divisions: Option<usize>,
    /// Cutoff of the reference speed, m/s; omitted = none.
    #[serde(default)]
    #[ts(optional)]
    pub cutoff: Option<f64>,
    /// Lines through the origin; omitted = false.
    #[serde(default)]
    #[ts(optional)]
    pub zero_intercept: Option<bool>,
    /// Matrix time series: width of the reference bins, m/s; omitted = 1.
    #[serde(default)]
    #[ts(optional)]
    pub mts_reference_bin: Option<f64>,
    /// Matrix time series: width of the site bins, m/s; omitted = 1.
    #[serde(default)]
    #[ts(optional)]
    pub mts_target_bin: Option<f64>,
    /// Matrix time series: moving average window of the percentiles, hours; omitted = 3.
    #[serde(default)]
    #[ts(optional)]
    pub mts_window_h: Option<f64>,
}

impl McpOptionsDto {
    fn into_core(self) -> McpOptions {
        let defaults = McpOptions::default();
        McpOptions {
            yearly_divisions: self.yearly_divisions.unwrap_or(defaults.yearly_divisions),
            cutoff_m_s: self.cutoff,
            zero_intercept: self.zero_intercept.unwrap_or(defaults.zero_intercept),
            mts: wind_core::mcp::MtsOptions {
                reference_bin: self.mts_reference_bin.filter(|width| *width > 0.0).unwrap_or(defaults.mts.reference_bin),
                target_bin: self.mts_target_bin.filter(|width| *width > 0.0).unwrap_or(defaults.mts.target_bin),
                window_h: self.mts_window_h.filter(|hours| *hours >= 0.0).unwrap_or(defaults.mts.window_h),
            },
        }
    }
}

/// Relate a site speed to a reference and predict the long term.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpRequest {
    /// The site's speed.
    pub site: McpColumnDto,
    /// The long reference's speed.
    pub reference: McpColumnDto,
    /// The averaging period.
    pub averaging: AveragingDto,
    /// Directions and sectors; omitted = one fit for all directions, no
    /// direction offset.
    #[serde(default)]
    #[ts(optional)]
    pub directions: Option<McpDirectionsDto>,
    /// Add each method's bootstrap interval of the long-term mean.
    #[serde(default)]
    #[ts(optional)]
    pub confidence: Option<bool>,
    /// Yearly divisions, cutoff and zero intercept; omitted = none of them.
    #[serde(default)]
    #[ts(optional)]
    pub options: Option<McpOptionsDto>,
}

/// The site's long-term series by one method, as CSV.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSeriesRequest {
    /// The site's speed.
    pub site: McpColumnDto,
    /// The long reference's speed.
    pub reference: McpColumnDto,
    /// The averaging period.
    pub averaging: AveragingDto,
    /// As in [`McpRequest`]; with a site direction the series has the
    /// direction too.
    #[serde(default)]
    #[ts(optional)]
    pub directions: Option<McpDirectionsDto>,
    /// The method whose fit predicts the series.
    pub method: McpMethodDto,
    /// As in [`McpRequest`].
    #[serde(default)]
    #[ts(optional)]
    pub options: Option<McpOptionsDto>,
}

/// One method's line and how it fares; `null` where a figure is undefined.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpResultDto {
    /// The method.
    pub method: McpMethodDto,
    /// Slope of y = slope·x + intercept, for the methods that draw a line.
    pub slope: Option<f64>,
    /// Intercept, in the site's units.
    pub intercept: Option<f64>,
    /// The method's prediction across the concurrent reference speeds,
    /// `[reference, site]`, for drawing.
    pub curve: Vec<[f64; 2]>,
    /// Error of the left-out monthly means (root mean square).
    pub cv_rmse: Option<f64>,
    /// Mean error of the left-out monthly means (predicted − observed).
    pub cv_bias: Option<f64>,
    /// Predicted over observed standard deviation, concurrent period.
    pub spread_ratio: Option<f64>,
    /// Site mean predicted over the whole reference record.
    pub long_term_mean: Option<f64>,
    /// Its bootstrap interval (concurrent months resampled), when asked for.
    pub long_term_interval: Option<IntervalDto>,
    /// Subdivisions with a fit of their own (the others use the overall fit).
    pub divisions_fitted: usize,
    /// Weibull k of the long-term predictions.
    pub long_term_k: Option<f64>,
    /// Weibull A of the long-term predictions.
    pub long_term_a: Option<f64>,
}

impl McpResultDto {
    /// `result`, with its prediction drawn across `lowest..=highest`.
    fn new(result: &McpResult, lowest: f64, highest: f64) -> Self {
        let line = result.predictor.line();
        let at = |point: usize| lowest + (highest - lowest) * point as f64 / (CURVE_POINTS - 1) as f64;
        McpResultDto {
            method: result.method.into(),
            slope: line.map(|line| line.slope),
            intercept: line.map(|line| line.intercept),
            curve: (0..CURVE_POINTS).map(|point| [at(point), result.predictor.predict(at(point))]).collect(),
            cv_rmse: finite(result.cv_rmse),
            cv_bias: finite(result.cv_bias),
            spread_ratio: finite(result.spread_ratio),
            long_term_mean: finite(result.long_term_mean),
            long_term_interval: result.long_term_interval.map(Into::into),
            divisions_fitted: result.division_predictors.iter().flatten().count(),
            long_term_k: result.long_term_weibull.map(|fit| fit.k),
            long_term_a: result.long_term_weibull.map(|fit| fit.a),
        }
    }
}

/// The answer to an [`McpRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpDto {
    /// Averaging period, s.
    pub period_s: f64,
    /// Concurrent periods.
    pub concurrent: usize,
    /// Start of the first concurrent period, epoch ms.
    pub concurrent_from_ms: f64,
    /// Start of the last concurrent period, epoch ms.
    pub concurrent_to_ms: f64,
    /// Pearson correlation of the concurrent periods.
    pub correlation: Option<f64>,
    /// Site mean over the concurrent periods.
    pub site_mean: f64,
    /// Reference mean over the concurrent periods.
    pub reference_concurrent_mean: f64,
    /// Reference mean over its whole record.
    pub reference_long_term_mean: f64,
    /// Reference periods with data.
    pub reference_periods: usize,
    /// Months the cross-validation left out in turn (0: none).
    pub folds: usize,
    /// One result per method.
    pub results: Vec<McpResultDto>,
    /// The method with the smallest cross-validation error.
    pub recommended: McpMethodDto,
    /// Sampled concurrent reference values, for the scatter.
    pub pairs_x: Vec<f64>,
    /// The matching site values.
    pub pairs_y: Vec<f64>,
    /// Sectors of the speed fits (1: one fit for all).
    pub sectors: usize,
    /// Subdivisions of the speed fits: groups of months × (sectors, plus the
    /// below-cutoff group with sectors and a cutoff); 1 = one fit for all.
    pub divisions: usize,
    /// The direction offset, with both directions.
    pub direction: Option<DirectionOffsetDto>,
}

/// The optimal time offset analysis between a site and a reference
/// (manual def-optimal-time-offset-analysis.htm): speed, and direction and
/// temperature when both sides name one.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeOffsetRequest {
    /// The site's speed.
    pub site: McpColumnDto,
    /// The reference's speed.
    pub reference: McpColumnDto,
    /// The averaging period, the step of the shifts.
    pub averaging: AveragingDto,
    /// The site's and the reference's direction column ids.
    #[serde(default)]
    #[ts(optional)]
    pub directions: Option<[String; 2]>,
    /// The site's and the reference's temperature column ids.
    #[serde(default)]
    #[ts(optional)]
    pub temperatures: Option<[String; 2]>,
    /// Farthest shift tried each way, hours; omitted = 24.
    #[serde(default)]
    #[ts(optional)]
    pub max_shift_h: Option<f64>,
}

/// One quantity's correlation at each shift.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OffsetCorrelationDto {
    /// Correlation per shift of [`TimeOffsetDto::shifts_h`].
    pub correlations: Vec<Option<f64>>,
    /// At no shift.
    pub zero_shift: Option<f64>,
    /// The highest.
    pub best: Option<f64>,
    /// Its shift, hours (the reference moved later by it).
    pub best_shift_h: Option<f64>,
}

/// The answer to a [`TimeOffsetRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TimeOffsetDto {
    /// Shifts tried, hours: positive moves the reference later.
    pub shifts_h: Vec<f64>,
    /// Speed.
    pub speed: OffsetCorrelationDto,
    /// Direction (circular correlation), with both directions.
    pub direction: Option<OffsetCorrelationDto>,
    /// Temperature, with both temperatures.
    pub temperature: Option<OffsetCorrelationDto>,
    /// The shift recommended for the reference, hours: the speed's best, as
    /// the original gives the speed top priority.
    pub recommended_h: Option<f64>,
}

/// Default farthest shift of the time offset analysis, hours.
const DEFAULT_MAX_SHIFT_H: f64 = 24.0;

/// The optimal time offset analysis of the request.
///
/// # Errors
/// An unknown column or a period shorter than a step.
pub fn time_offset(site: Prepared, reference: Prepared, request: &TimeOffsetRequest) -> CoreResult<TimeOffsetDto> {
    let period_s = request.averaging.seconds();
    let hours_per_period = period_s as f64 / SECONDS_PER_HOUR as f64;
    let max_shift = (request.max_shift_h.unwrap_or(DEFAULT_MAX_SHIFT_H) / hours_per_period).round().max(0.0) as i64;
    let average = |prepared: Prepared, id: &str| -> CoreResult<Averaged> { mcp::average(prepared.data, prepared.view, column_index(prepared.data, id)?, period_s) };
    let scan = |ids: &[String; 2], circular: bool| -> CoreResult<OffsetCorrelationDto> {
        let found = mcp::offset_correlation(&average(site, &ids[0])?, &average(reference, &ids[1])?, circular, max_shift);
        let at = |shift: i64| found.shifts.iter().position(|&each| each == shift).and_then(|index| finite(found.correlations[index]));
        Ok(OffsetCorrelationDto {
            correlations: found.correlations.iter().map(|&value| finite(value)).collect(),
            zero_shift: at(0),
            best: found.best.and_then(at),
            best_shift_h: found.best.map(|shift| shift as f64 * hours_per_period),
        })
    };
    let speed = scan(&[request.site.column.clone(), request.reference.column.clone()], false)?;
    Ok(TimeOffsetDto {
        shifts_h: (-max_shift..=max_shift).map(|shift| shift as f64 * hours_per_period).collect(),
        recommended_h: speed.best_shift_h,
        speed,
        direction: request.directions.as_ref().map(|ids| scan(ids, true)).transpose()?,
        temperature: request.temperatures.as_ref().map(|ids| scan(ids, false)).transpose()?,
    })
}

/// The columns of an adjustment averaged over its period: site and
/// reference speeds, and the directions when asked for.
struct Inputs {
    site: Averaged,
    reference: Averaged,
    reference_direction: Option<Averaged>,
    site_direction: Option<Averaged>,
    sectors: usize,
}

impl Inputs {
    fn read(site: Prepared, reference: Prepared, site_column: &McpColumnDto, reference_column: &McpColumnDto, directions: Option<&McpDirectionsDto>, period_s: i64) -> CoreResult<Self> {
        let average = |prepared: Prepared, id: &str| -> CoreResult<Averaged> { mcp::average(prepared.data, prepared.view, column_index(prepared.data, id)?, period_s) };
        Ok(Inputs {
            site: average(site, &site_column.column)?,
            reference: average(reference, &reference_column.column)?,
            reference_direction: directions.map(|directions| average(reference, &directions.reference)).transpose()?,
            site_direction: directions.and_then(|directions| directions.site.as_deref()).map(|id| average(site, id)).transpose()?,
            sectors: directions.and_then(|directions| directions.sectors).unwrap_or(1),
        })
    }

    fn directions(&self) -> Option<Directions<'_>> {
        Some(Directions { reference: self.reference_direction.as_ref()?, site: self.site_direction.as_ref(), sectors: self.sectors })
    }
}

/// The long-term adjustment of the request's site column.
///
/// # Errors
/// An unknown column, a period shorter than a step, or too few concurrent periods.
pub fn long_term(site: Prepared, reference: Prepared, request: &McpRequest) -> CoreResult<McpDto> {
    let period_s = request.averaging.seconds();
    let inputs = Inputs::read(site, reference, &request.site, &request.reference, request.directions.as_ref(), period_s)?;
    let options = request.options.clone().unwrap_or_default().into_core();
    let found = mcp::mcp(&inputs.site, &inputs.reference, inputs.directions(), period_s, request.confidence.unwrap_or(false), &options)?;
    let stride = found.pairs.len().div_ceil(MAX_MCP_PAIRS).max(1);
    let (lowest, highest) = found.pairs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &(x, _)| (low.min(x), high.max(x)));
    let sampled = found.pairs.iter().step_by(stride);
    Ok(McpDto {
        period_s: period_s as f64,
        concurrent: found.concurrent,
        concurrent_from_ms: found.concurrent_span.0 as f64 * MS_PER_SECOND,
        concurrent_to_ms: found.concurrent_span.1 as f64 * MS_PER_SECOND,
        correlation: finite(found.correlation),
        site_mean: found.site_mean,
        reference_concurrent_mean: found.reference_concurrent_mean,
        reference_long_term_mean: found.reference_long_term_mean,
        reference_periods: found.reference_periods,
        folds: found.folds,
        results: found.results.iter().map(|result| McpResultDto::new(result, lowest, highest)).collect(),
        recommended: found.recommended.into(),
        pairs_x: sampled.clone().map(|&(x, _)| x).collect(),
        pairs_y: sampled.map(|&(_, y)| y).collect(),
        sectors: found.sectors,
        divisions: found.divisions,
        direction: found.direction.map(|direction| DirectionOffsetDto {
            offset_deg: direction.offset_deg,
            concurrent: direction.concurrent,
            site_frequency: direction.site_frequency,
            long_term_frequency: direction.long_term_frequency,
        }),
    })
}

/// The site's long-term series by the request's method, as CSV text:
/// a header, then one `time,speed` row per reference period with data.
///
/// # Errors
/// As [`long_term`].
pub fn long_term_series(site: Prepared, reference: Prepared, request: &McpSeriesRequest) -> CoreResult<String> {
    let period_s = request.averaging.seconds();
    let inputs = Inputs::read(site, reference, &request.site, &request.reference, request.directions.as_ref(), period_s)?;
    let options = request.options.clone().unwrap_or_default().into_core();
    let found = mcp::mcp(&inputs.site, &inputs.reference, inputs.directions(), period_s, false, &options)?;
    let wanted = request.method;
    let chosen = found.results.iter().find(|result| McpMethodDto::from(result.method) == wanted).ok_or(CoreError::McpMethodUnavailable)?;
    let offset = found.direction.as_ref().map(|direction| direction.offset_deg);
    let direction_header = match (&request.directions, offset) {
        (Some(McpDirectionsDto { site: Some(site), .. }), Some(_)) => format!(",{site} (longo prazo)"),
        _ => String::new(),
    };
    let mut csv = format!("Data/Hora (UTC),{} (longo prazo){direction_header}\n", request.site.column);
    for (time, speed, direction) in mcp::synthesize(&inputs.reference, inputs.directions(), period_s, chosen, offset, &options) {
        // With a direction column, a period without the reference's direction leaves its field empty.
        let direction = match (direction_header.is_empty(), direction.is_nan()) {
            (true, _) => String::new(),
            (false, true) => ",".to_string(),
            (false, false) => format!(",{direction}"),
        };
        csv.push_str(&format!("{},{speed}{direction}\n", format_utc(time, SERIES_TIME_FORMAT)));
    }
    Ok(csv)
}

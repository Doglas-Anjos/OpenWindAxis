//! The analysis calls, answered once for every adapter (browser, command
//! line): request DTO in, response DTO out. The adapters only find the
//! dataset and turn errors into their own form, so a new request field (a
//! filter, a method option) is wired in one place.
use crate::filter::FilterDto;
use crate::ops::{column_index, column_indices};
use crate::requests::{
    CoverageRequest, DiurnalRequest, HistogramRequest, MonthlyMeansRequest, ShearRequest, WindRoseRequest, DEFAULT_ROSE_SPEED_EDGES,
};
use crate::responses::{CoverageDto, DiurnalDto, HistogramDto, MonthlyMeansDto, ShearDto, WindRoseDto};
use crate::ErrorDto;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::borrow::Cow;
use wind_core::{analysis, bootstrap};
use wind_core::error::CoreResult;
use wind_core::filter::FilterView;
use wind_core::model::Dataset;
use wind_core::series::DayStats;

/// The calm threshold of a rose request without one: no calms, every
/// direction counts.
const NO_CALM_THRESHOLD: f64 = 0.0;

/// A dataset with the default view and per-day summaries its adapter keeps
/// in step with every edit.
#[derive(Clone, Copy)]
pub struct Prepared<'a> {
    /// The dataset.
    pub data: &'a Dataset,
    /// Default flag filtering.
    pub view: &'a FilterView,
    /// Per-day summaries under `view`.
    pub stats: &'a DayStats,
}

impl Prepared<'_> {
    /// The view an analysis reads: the kept one without a filter, else one
    /// built for the filter (one pass over the record).
    pub(crate) fn view(&self, filter: Option<FilterDto>) -> CoreResult<Cow<'_, FilterView>> {
        match filter {
            None => Ok(Cow::Borrowed(self.view)),
            Some(filter) => Ok(Cow::Owned(FilterView::with_criteria(self.data, &filter.into_core(self.data)?)?)),
        }
    }
}

/// Data recovery by month. With a filter the per-day summaries are rebuilt
/// under it (one pass over the record); without, the kept ones answer.
///
/// # Errors
/// The filter's, or an unknown column in it.
pub fn coverage(prepared: Prepared, request: CoverageRequest) -> CoreResult<CoverageDto> {
    let coverage = match request.filter {
        None => analysis::coverage(prepared.stats),
        Some(filter) => {
            let view = prepared.view(Some(filter))?;
            analysis::coverage(&DayStats::new(prepared.data, &view))
        }
    };
    Ok(CoverageDto::from(coverage))
}

/// Frequency histogram of one column, with an optional Weibull fit.
///
/// # Errors
/// An unknown column, the filter's, or the analysis's own.
pub fn histogram(prepared: Prepared, request: HistogramRequest) -> CoreResult<HistogramDto> {
    let column = column_index(prepared.data, &request.column)?;
    let view = prepared.view(request.filter)?;
    let fit = request.weibull.map(Into::into);
    let mut histogram = HistogramDto::from(analysis::histogram(prepared.data, &view, column, request.bin_width, fit)?);
    if let (Some(method), Some(weibull), Some(true)) = (fit, histogram.weibull.as_mut(), request.confidence) {
        if let Some(interval) = bootstrap::weibull_interval(prepared.data, &view, column, method) {
            weibull.k_interval = Some(interval.k.into());
            weibull.a_interval = Some(interval.a.into());
        }
    }
    Ok(histogram)
}

/// Wind rose of a direction column, with speed classes when a speed is given.
///
/// # Errors
/// An unknown column, the filter's, or the analysis's own.
pub fn wind_rose(prepared: Prepared, request: WindRoseRequest) -> CoreResult<WindRoseDto> {
    let direction = column_index(prepared.data, &request.direction)?;
    let speed = request.speed.as_deref().map(|id| column_index(prepared.data, id)).transpose()?;
    let edges = request.speed_edges.unwrap_or_else(|| DEFAULT_ROSE_SPEED_EDGES.to_vec());
    let view = prepared.view(request.filter)?;
    let calm_threshold = request.calm_threshold.unwrap_or(NO_CALM_THRESHOLD);
    Ok(WindRoseDto::from(analysis::wind_rose(prepared.data, &view, direction, speed, request.sectors, &edges, calm_threshold)?))
}

/// Mean by hour of day, over the record and per month.
///
/// # Errors
/// An unknown column, or the filter's.
pub fn diurnal(prepared: Prepared, request: DiurnalRequest) -> CoreResult<DiurnalDto> {
    let column = column_index(prepared.data, &request.column)?;
    let view = prepared.view(request.filter)?;
    Ok(DiurnalDto::from(analysis::diurnal(prepared.data, &view, prepared.stats.months(), column)))
}

/// Mean by calendar month over every year of the record.
///
/// # Errors
/// An unknown column, or the filter's.
pub fn monthly_means(prepared: Prepared, request: MonthlyMeansRequest) -> CoreResult<MonthlyMeansDto> {
    let column = column_index(prepared.data, &request.column)?;
    let view = prepared.view(request.filter)?;
    Ok(MonthlyMeansDto::from(analysis::monthly_means(prepared.data, &view, prepared.stats.months(), column)))
}

/// Wind shear between speed columns, optionally by direction sector.
///
/// # Errors
/// An unknown column, the filter's, or the analysis's own.
pub fn shear(prepared: Prepared, request: ShearRequest) -> CoreResult<ShearDto> {
    let columns = column_indices(prepared.data, &request.columns)?;
    let direction = request.direction.as_deref().map(|id| column_index(prepared.data, id)).transpose()?;
    let view = prepared.view(request.filter)?;
    let aggregation = request.aggregation.map(Into::into).unwrap_or_default();
    Ok(ShearDto::from(analysis::shear(prepared.data, &view, &columns, direction, request.sectors, aggregation)?))
}

/// Code of an [`ErrorDto`] for an analysis name [`run`] does not know.
pub const UNKNOWN_ANALYSIS: &str = "unknown_analysis";
/// The analyses [`run`] answers, by the names `wind analyze` and recipes use.
pub const ANALYSIS_NAMES: [&str; 15] = [
    "coverage", "histogram", "distribution", "rose", "diurnal", "monthly", "shear", "shear_view", "shear_frequency", "turbulence", "distortion",
    "distortion_detail", "extremes", "table", "scatter",
];

/// Any analysis by name, its request and answer as JSON: the one dispatch
/// the command line, the browser's recipes and the replay share, so a
/// recipe replays through exactly the call that made it. The request's
/// `dataset` field is read but not used: `prepared` is the dataset.
///
/// # Errors
/// [`UNKNOWN_ANALYSIS`], a request that does not parse ([`ErrorDto::bad_request`]),
/// or the analysis's own error.
pub fn run(prepared: Prepared, name: &str, request: Value) -> Result<Value, ErrorDto> {
    fn answer<Request: DeserializeOwned, Response: Serialize>(
        prepared: Prepared,
        request: Value,
        call: impl FnOnce(Prepared, Request) -> CoreResult<Response>,
    ) -> Result<Value, ErrorDto> {
        let request: Request = serde_json::from_value(request).map_err(ErrorDto::bad_request)?;
        let response = call(prepared, request).map_err(|error| ErrorDto::from(&error))?;
        Ok(serde_json::to_value(response).expect("DTOs hold plain fields and string keys only"))
    }
    match name {
        "coverage" => answer(prepared, request, coverage),
        "histogram" => answer(prepared, request, histogram),
        "distribution" => answer(prepared, request, crate::distribution::speed_distribution),
        "rose" => answer(prepared, request, wind_rose),
        "diurnal" => answer(prepared, request, diurnal),
        "monthly" => answer(prepared, request, monthly_means),
        "shear" => answer(prepared, request, shear),
        "shear_view" => answer(prepared, request, crate::shear_views::shear_view),
        "shear_frequency" => answer(prepared, request, crate::shear_views::shear_frequency),
        "turbulence" => answer(prepared, request, crate::turbulence::turbulence),
        "distortion" => answer(prepared, request, crate::distortion::tower_distortion),
        "distortion_detail" => answer(prepared, request, crate::distortion::distortion_detail),
        "extremes" => answer(prepared, request, crate::extremes::extremes),
        "table" => answer(prepared, request, crate::means::monthly_table),
        "scatter" => answer(prepared, request, crate::scatter::scatter),
        _ => Err(ErrorDto { code: UNKNOWN_ANALYSIS.into(), message: format!("unknown analysis {name:?}; known: {}", ANALYSIS_NAMES.join(", ")) }),
    }
}

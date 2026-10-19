//! Analisar › Dispersão on the wire ([`wind_core::analysis::scatter`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::column_index;
use crate::{finite, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis;
use wind_core::error::CoreResult;

/// Points a scatter request draws when it names no limit: enough for the
/// cloud's shape, few enough for a canvas to stay quick.
pub const DEFAULT_SCATTER_POINTS: usize = 20_000;

/// One column against another.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScatterRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id on the horizontal axis.
    pub x: String,
    /// Column id on the vertical axis.
    pub y: String,
    /// Most points returned; omitted = 20 000.
    #[serde(default)]
    #[ts(optional)]
    pub max_points: Option<usize>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// The answer to a [`ScatterRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScatterDto {
    /// Sampled x values, in time order.
    pub xs: Vec<f64>,
    /// The matching y values.
    pub ys: Vec<f64>,
    /// Steps where both are valid.
    pub count: usize,
    /// One point drawn per this many valid steps.
    pub stride: usize,
    /// Least squares line y = slope·x + intercept, over every valid step.
    pub slope: Option<f64>,
    /// Its intercept.
    pub intercept: Option<f64>,
    /// Its R².
    pub r_squared: Option<f64>,
}

/// The scatter of the request's two columns.
///
/// # Errors
/// An unknown column, the filter's, or fewer than two common steps.
pub fn scatter(prepared: Prepared, request: ScatterRequest) -> CoreResult<ScatterDto> {
    let (x, y) = (column_index(prepared.data, &request.x)?, column_index(prepared.data, &request.y)?);
    let view = prepared.view(request.filter)?;
    let found = analysis::scatter(prepared.data, &view, x, y, request.max_points.unwrap_or(DEFAULT_SCATTER_POINTS))?;
    Ok(ScatterDto {
        xs: found.points.iter().map(|&(x, _)| x).collect(),
        ys: found.points.iter().map(|&(_, y)| y).collect(),
        count: found.count,
        stride: found.stride,
        slope: finite(found.slope),
        intercept: finite(found.intercept),
        r_squared: finite(found.r_squared),
    })
}

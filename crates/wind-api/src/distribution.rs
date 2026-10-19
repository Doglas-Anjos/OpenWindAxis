//! Analisar › Distribuição de velocidade on the wire: the four Weibull fits
//! of one speed column and their figures ([`wind_core::analysis::speed_distribution`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::column_index;
use crate::{finite, DatasetId, ErrorDto, WeibullMethod};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis;
use wind_core::error::CoreResult;

/// The speed distribution of one column.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpeedDistributionRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Speed column id.
    pub column: String,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// One Weibull fit and its figures; the numbers are `null` when it failed.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DistributionFitDto {
    /// The algorithm.
    pub method: WeibullMethod,
    /// Shape k.
    pub k: Option<f64>,
    /// Scale A, m/s.
    pub a: Option<f64>,
    /// Mean of the fitted distribution, m/s.
    pub mean: Option<f64>,
    /// Share of the fitted distribution above the measured mean.
    pub above_mean: Option<f64>,
    /// Power density of the fitted distribution at 1.225 kg/m³, W/m².
    pub power_density_w_m2: Option<f64>,
    /// Goodness of fit against the histogram.
    pub r_squared: Option<f64>,
    /// Why the fit failed.
    pub error: Option<ErrorDto>,
}

/// The measured distribution and its four fits.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpeedDistributionDto {
    /// Speeds the filter kept.
    pub samples: usize,
    /// Their mean, m/s.
    pub mean: f64,
    /// Share of them above the mean.
    pub above_mean: f64,
    /// Their power density at 1.225 kg/m³, W/m².
    pub power_density_w_m2: f64,
    /// Bin width R² is measured on, m/s: a default for the histogram drawn.
    pub bin_width: f64,
    /// Maximum likelihood, least squares, WAsP and Openwind, in that order.
    pub fits: Vec<DistributionFitDto>,
}

/// The four fits of `request.column`.
///
/// # Errors
/// An unknown column, the filter's, or no speed left after it.
pub fn speed_distribution(prepared: Prepared, request: SpeedDistributionRequest) -> CoreResult<SpeedDistributionDto> {
    let column = column_index(prepared.data, &request.column)?;
    let view = prepared.view(request.filter)?;
    let distribution = analysis::speed_distribution(prepared.data, &view, column)?;
    let fits = distribution
        .fits
        .into_iter()
        .map(|(method, fit)| match fit {
            Ok(fit) => DistributionFitDto {
                method: method.into(),
                k: finite(fit.weibull.k),
                a: finite(fit.weibull.a),
                mean: finite(fit.mean),
                above_mean: finite(fit.above_mean),
                power_density_w_m2: finite(fit.power_density),
                r_squared: finite(fit.r_squared),
                error: None,
            },
            Err(error) => DistributionFitDto {
                method: method.into(),
                k: None,
                a: None,
                mean: None,
                above_mean: None,
                power_density_w_m2: None,
                r_squared: None,
                error: Some((&error).into()),
            },
        })
        .collect();
    Ok(SpeedDistributionDto {
        samples: distribution.samples,
        mean: distribution.mean,
        above_mean: distribution.above_mean,
        power_density_w_m2: distribution.power_density,
        bin_width: distribution.bin_width,
        fits,
    })
}

//! Analisar › Turbulência on the wire: the request, the answer, and the call
//! every adapter answers through ([`wind_core::analysis::turbulence`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::{column_index, column_indices};
use crate::{finite, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis::{self, IecEdition, TiStats, Turbulence, TurbulenceVersus, DEFAULT_SPEED_BIN_M_S};
use wind_core::error::CoreResult;
use wind_core::model::Dataset;

/// Edition of IEC 61400-1 the figures and categories follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IecEditionDto {
    /// 1999: characteristic TI (mean + SD), categories A and B.
    Ed2,
    /// 2005: representative TI (mean + 1.28·SD), categories A, B and C.
    Ed3,
    /// 2019: representative TI, categories A+, A, B and C. The default.
    #[default]
    Ed4,
}

impl From<IecEditionDto> for IecEdition {
    fn from(edition: IecEditionDto) -> Self {
        match edition {
            IecEditionDto::Ed2 => IecEdition::Second,
            IecEditionDto::Ed3 => IecEdition::Third,
            IecEditionDto::Ed4 => IecEdition::Fourth,
        }
    }
}

/// What the TI is shown against, tagged by `kind`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum TurbulenceVersusDto {
    /// Speed bins centred on multiples of the width.
    Speed {
        /// m/s; omitted = 1.
        #[serde(default)]
        #[ts(optional)]
        bin_width: Option<f64>,
    },
    /// Direction sectors, the first centred on north.
    Direction {
        /// Direction column id.
        direction: String,
        /// Sectors, 1..=360.
        sectors: usize,
    },
    /// Calendar month over every year.
    Month,
    /// Hour of day.
    Hour,
    /// One bin per sensor, at its height.
    Height,
}

/// Turbulence of some speed sensors. Each needs a turbulence intensity
/// column (a standard deviation child).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurbulenceRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Mean speed column ids.
    pub speeds: Vec<String>,
    /// What the TI is shown against.
    pub versus: TurbulenceVersusDto,
    /// Omitted = the fourth edition.
    #[serde(default)]
    #[ts(optional)]
    pub edition: Option<IecEditionDto>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// TI statistics of one bin; `null` where a figure is undefined.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TiStatsDto {
    /// Steps in the bin.
    pub count: usize,
    /// Mean TI.
    pub mean: Option<f64>,
    /// Sample standard deviation of the TI (two steps or more).
    pub deviation: Option<f64>,
    /// Highest TI.
    pub peak: Option<f64>,
    /// Representative TI (editions 3 and 4) or characteristic TI (edition 2).
    pub figure: Option<f64>,
}

impl From<TiStats> for TiStatsDto {
    fn from(stats: TiStats) -> Self {
        TiStatsDto { count: stats.count, mean: finite(stats.mean), deviation: finite(stats.deviation), peak: finite(stats.peak), figure: finite(stats.figure) }
    }
}

/// One bin of one sensor.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurbulenceBinDto {
    /// Speed column id.
    pub sensor: String,
    /// Bin centre: speed (m/s), sector centre (°), month (1–12), hour
    /// (0–23) or the sensor's height (m; `null` without one).
    pub center: Option<f64>,
    /// Its statistics.
    #[serde(flatten)]
    #[ts(flatten)]
    pub stats: TiStatsDto,
}

/// A sensor's 15 m/s bin (14.5–15.5 m/s) and IEC turbulence category.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SensorAt15Dto {
    /// Speed column id.
    pub sensor: String,
    /// Its height, m.
    pub height_m: Option<f64>,
    /// The 15 m/s bin.
    #[serde(flatten)]
    #[ts(flatten)]
    pub stats: TiStatsDto,
    /// "A+", "A", "B", "C", or "S" above them; `null` with fewer than two
    /// steps in the bin.
    pub category: Option<String>,
}

/// One category's normal turbulence model, for the TI-versus-speed graph.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NtmCurveDto {
    /// "A+", "A", "B" or "C".
    pub category: String,
    /// TI at each speed of [`TurbulenceDto::ntm_speeds`].
    pub ti: Vec<Option<f64>>,
}

/// The answer to a [`TurbulenceRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurbulenceDto {
    /// Every sensor's bins, sensor by sensor, in bin order.
    pub bins: Vec<TurbulenceBinDto>,
    /// Every sensor's 15 m/s bin and category.
    pub at15: Vec<SensorAt15Dto>,
    /// The edition followed.
    pub edition: IecEditionDto,
    /// Speeds the NTM curves are given at, m/s: the speed bins' centres
    /// (empty unless versus speed).
    pub ntm_speeds: Vec<f64>,
    /// Each category's NTM at those speeds.
    pub ntm: Vec<NtmCurveDto>,
}

impl TurbulenceDto {
    fn new(dataset: &Dataset, turbulence: Turbulence, edition: IecEditionDto, versus_speed: bool) -> Self {
        let id = |sensor: usize| dataset.columns[sensor].id.clone();
        let core_edition = IecEdition::from(edition);
        let mut ntm_speeds: Vec<f64> = if versus_speed { turbulence.bins.iter().map(|bin| bin.center).filter(|&speed| speed > 0.0).collect() } else { Vec::new() };
        ntm_speeds.sort_by(f64::total_cmp);
        ntm_speeds.dedup();
        let ntm = core_edition
            .categories()
            .iter()
            .map(|category| NtmCurveDto {
                category: category.name.to_string(),
                ti: ntm_speeds.iter().map(|&speed| finite(core_edition.ntm(category, speed))).collect(),
            })
            .collect();
        TurbulenceDto {
            bins: turbulence.bins.iter().map(|bin| TurbulenceBinDto { sensor: id(bin.sensor), center: finite(bin.center), stats: bin.stats.into() }).collect(),
            at15: turbulence
                .at_15
                .iter()
                .map(|sensor| SensorAt15Dto {
                    sensor: id(sensor.sensor),
                    height_m: dataset.columns[sensor.sensor].height_m,
                    stats: sensor.stats.into(),
                    category: sensor.category.map(str::to_string),
                })
                .collect(),
            edition,
            ntm_speeds,
            ntm,
        }
    }
}

/// Turbulence of the request's sensors.
///
/// # Errors
/// An unknown column, the filter's, or the analysis's own.
pub fn turbulence(prepared: Prepared, request: TurbulenceRequest) -> CoreResult<TurbulenceDto> {
    let data = prepared.data;
    let sensors = column_indices(data, &request.speeds)?;
    let versus = match request.versus {
        TurbulenceVersusDto::Speed { bin_width } => TurbulenceVersus::Speed { bin_width: bin_width.unwrap_or(DEFAULT_SPEED_BIN_M_S) },
        TurbulenceVersusDto::Direction { direction, sectors } => TurbulenceVersus::Direction { column: column_index(data, &direction)?, sectors },
        TurbulenceVersusDto::Month => TurbulenceVersus::Month,
        TurbulenceVersusDto::Hour => TurbulenceVersus::Hour,
        TurbulenceVersusDto::Height => TurbulenceVersus::Height,
    };
    let edition = request.edition.unwrap_or_default();
    let view = prepared.view(request.filter)?;
    let turbulence = analysis::turbulence(data, &view, prepared.stats.months(), &sensors, versus, edition.into())?;
    Ok(TurbulenceDto::new(data, turbulence, edition, matches!(versus, TurbulenceVersus::Speed { .. })))
}

//! Analisar › Distorção da torre on the wire: every pair of colocated
//! anemometers, with its measure by direction sector and its two factors
//! ([`wind_core::analysis::tower_distortion`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::shading::colocated_pairs;
use crate::{finite, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use crate::ops::column_index;
use wind_core::analysis::{self, DistortionMeasure, SectorStats, DISTORTION_SECTORS};
use wind_core::error::{CoreError, CoreResult};
use wind_core::primary::COLOCATION_THRESHOLD_M;

/// What the window compares, A / B or A − B.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DistortionMeasureDto {
    /// A / B, the default.
    #[default]
    Ratio,
    /// A − B, m/s.
    Difference,
}

impl From<DistortionMeasureDto> for DistortionMeasure {
    fn from(measure: DistortionMeasureDto) -> Self {
        match measure {
            DistortionMeasureDto::Ratio => DistortionMeasure::Ratio,
            DistortionMeasureDto::Difference => DistortionMeasure::Difference,
        }
    }
}

/// The tower distortion of every pair of colocated anemometers.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TowerDistortionRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Omitted = the ratio.
    #[serde(default)]
    #[ts(optional)]
    pub measure: Option<DistortionMeasureDto>,
    /// Anemometers within this many metres of height are a pair; omitted = 1.
    #[serde(default)]
    #[ts(optional)]
    pub colocation_m: Option<f64>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// One pair's other displays: medians by sector per year and per month,
/// and the steps for the scatter plots.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DistortionDetailRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// First anemometer's column id.
    pub a: String,
    /// Second anemometer's column id.
    pub b: String,
    /// Direction column id.
    pub direction: String,
    /// Omitted = the ratio.
    #[serde(default)]
    #[ts(optional)]
    pub measure: Option<DistortionMeasureDto>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// One calendar year's medians by sector.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct YearMediansDto {
    /// The year.
    pub year: i32,
    /// Median per sector; `null` where empty.
    pub medians: Vec<Option<f64>>,
}

/// The answer to a [`DistortionDetailRequest`].
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DistortionDetailDto {
    /// Sectors (72).
    pub sectors: usize,
    /// Per calendar year with data.
    pub by_year: Vec<YearMediansDto>,
    /// Per calendar month, January first: median per sector.
    pub by_month: Vec<Vec<Option<f64>>>,
    /// The scatter's steps (at most [`MAX_DISTORTION_POINTS`]): direction, °.
    pub point_directions: Vec<f64>,
    /// Their value.
    pub point_values: Vec<f64>,
    /// Their year.
    pub point_years: Vec<i32>,
    /// Their month, 0 = January.
    pub point_months: Vec<u8>,
}

/// Steps the scatter plots show at most: every k-th one beyond.
pub const MAX_DISTORTION_POINTS: usize = 20_000;

/// The detail of one pair of `prepared`.
///
/// # Errors
/// [`CoreError::UnknownColumn`], or those of [`analysis::distortion_detail`].
pub fn distortion_detail(prepared: Prepared, request: DistortionDetailRequest) -> CoreResult<DistortionDetailDto> {
    let dataset = prepared.data;
    let (a, b, direction) = (column_index(dataset, &request.a)?, column_index(dataset, &request.b)?, column_index(dataset, &request.direction)?);
    let view = prepared.view(request.filter)?;
    let detail = analysis::distortion_detail(dataset, &view, a, b, direction, request.measure.unwrap_or_default().into(), MAX_DISTORTION_POINTS)?;
    let nullable = |values: &[f64]| values.iter().map(|&value| finite(value)).collect();
    Ok(DistortionDetailDto {
        sectors: DISTORTION_SECTORS,
        by_year: detail.by_year.iter().map(|(year, medians)| YearMediansDto { year: *year, medians: nullable(medians) }).collect(),
        by_month: detail.by_month.iter().map(|medians| nullable(medians)).collect(),
        point_directions: detail.points.iter().map(|point| point.0).collect(),
        point_values: detail.points.iter().map(|point| point.1).collect(),
        point_years: detail.points.iter().map(|point| point.2).collect(),
        point_months: detail.points.iter().map(|point| point.3).collect(),
    })
}

/// One direction sector; `null` where it has no step.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SectorStatsDto {
    /// Steps in it.
    pub count: usize,
    /// Mean.
    pub mean: Option<f64>,
    /// Smallest value.
    pub min: Option<f64>,
    /// Largest value.
    pub max: Option<f64>,
    /// Sample standard deviation.
    pub deviation: Option<f64>,
    /// 25th percentile (nearest rank).
    pub lower_quartile: Option<f64>,
    /// Median (nearest rank).
    pub median: Option<f64>,
    /// 75th percentile (nearest rank).
    pub upper_quartile: Option<f64>,
}

impl From<SectorStats> for SectorStatsDto {
    fn from(stats: SectorStats) -> Self {
        SectorStatsDto {
            count: stats.count,
            mean: finite(stats.mean),
            min: finite(stats.min),
            max: finite(stats.max),
            deviation: finite(stats.deviation),
            lower_quartile: finite(stats.lower_quartile),
            median: finite(stats.median),
            upper_quartile: finite(stats.upper_quartile),
        }
    }
}

/// One pair's distortion.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DistortionPairDto {
    /// Speed column id A.
    pub a: String,
    /// Speed column id B.
    pub b: String,
    /// Direction column id, the nearest in height.
    pub direction: String,
    /// Height of A, m.
    pub height_m: f64,
    /// Steps with both speeds and the direction.
    pub occurrences: usize,
    /// Weighted departure of the sectors' median ratio from 1.
    pub distortion_factor: Option<f64>,
    /// Weighted standard deviation of the ratios within sectors.
    pub scatter_factor: Option<f64>,
    /// The measure by sector, sector `i` centred on `i · 360/sectors`°.
    pub sectors: Vec<SectorStatsDto>,
}

/// Every pair's distortion.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TowerDistortionDto {
    /// Direction sectors.
    pub sectors: usize,
    /// What the sectors hold.
    pub measure: DistortionMeasureDto,
    /// Pairs in column order; empty when no two anemometers stand together.
    pub pairs: Vec<DistortionPairDto>,
}

/// The distortion of every pair of colocated anemometers.
///
/// # Errors
/// The filter's; [`CoreError::RuleThresholdInvalid`] for a negative or
/// non-finite colocation distance.
pub fn tower_distortion(prepared: Prepared, request: TowerDistortionRequest) -> CoreResult<TowerDistortionDto> {
    let dataset = prepared.data;
    let colocation_m = request.colocation_m.unwrap_or(COLOCATION_THRESHOLD_M);
    if !(colocation_m >= 0.0 && colocation_m.is_finite()) {
        return Err(CoreError::RuleThresholdInvalid { threshold: colocation_m });
    }
    let measure = request.measure.unwrap_or_default();
    let view = prepared.view(request.filter)?;
    let pairs = colocated_pairs(dataset, colocation_m)
        .into_iter()
        .map(|(a, b, direction)| {
            let distortion = analysis::tower_distortion(dataset, &view, a, b, direction, measure.into())?;
            Ok(DistortionPairDto {
                a: dataset.columns[a].id.clone(),
                b: dataset.columns[b].id.clone(),
                direction: dataset.columns[direction].id.clone(),
                height_m: dataset.columns[a].height_m.unwrap_or_default(),
                occurrences: distortion.occurrences,
                distortion_factor: finite(distortion.distortion_factor),
                scatter_factor: finite(distortion.scatter_factor),
                sectors: distortion.sectors.into_iter().map(Into::into).collect(),
            })
        })
        .collect::<CoreResult<_>>()?;
    Ok(TowerDistortionDto { sectors: DISTORTION_SECTORS, measure, pairs })
}

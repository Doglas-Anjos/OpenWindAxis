//! Flag › Sombra da torre on the wire: every pair of colocated anemometers
//! of a dataset, with its ratio pattern and shaded sectors
//! ([`wind_core::shading`]). The window turns them into direction-in-sector
//! flag rules, run like any other ([`crate::rules`]).
use crate::analyses::Prepared;
use crate::{finite_all, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::{CoreError, CoreResult};
use wind_core::filter::FilterView;
use wind_core::model::{ColumnType, Dataset, Subtype};
use wind_core::primary::COLOCATION_THRESHOLD_M;
use wind_core::shading::{self, DEFAULT_SHADED_WIDTH_DEG, SHADING_SECTORS};

/// The anemometer pairs of a dataset and where each is shaded.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TowerShadingRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Index into `DatasetDto::flags`: the flag the rules will apply. Data
    /// under it stays in the detection, so the pattern survives flagging.
    pub flag: usize,
}

/// One pair of colocated anemometers and its ratio pattern.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShadingPairDto {
    /// Speed column id A (the numerator of the ratio).
    pub a: String,
    /// Speed column id B.
    pub b: String,
    /// Direction column id, the nearest in height to the pair.
    pub direction: String,
    /// Height of A, m.
    pub height_m: f64,
    /// Median of A/B per sector, sector `i` centred on `i · 360/sectors`°;
    /// `null` where the sector has no data.
    pub medians: Vec<Option<f64>>,
    /// Ratios per sector.
    pub counts: Vec<usize>,
    /// Centre of the sector where A is shaded (lowest median), degrees;
    /// `null` when no sector has enough data.
    pub shaded_a: Option<f64>,
    /// Centre of the sector where B is shaded (highest median), degrees.
    pub shaded_b: Option<f64>,
}

/// Every anemometer pair of the dataset, with its pattern.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TowerShadingDto {
    /// Direction sectors of each pattern.
    pub sectors: usize,
    /// Width a shaded sector starts with, degrees.
    pub default_width_deg: f64,
    /// Pairs in column order; empty when no two anemometers stand together.
    pub pairs: Vec<ShadingPairDto>,
}

/// Every pair of measured mean speed columns within `threshold_m` of
/// height of each other, with the measured mean direction nearest in
/// height (the first of equals); pairs without a direction are left out.
pub(crate) fn colocated_pairs(dataset: &Dataset, threshold_m: f64) -> Vec<(usize, usize, usize)> {
    let measured_means = |kind: ColumnType| -> Vec<(usize, f64)> {
        dataset
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.kind == kind && column.subtype == Subtype::Mean && column.derived.is_none())
            .filter_map(|(index, column)| column.height_m.map(|height| (index, height)))
            .collect()
    };
    let (speeds, directions) = (measured_means(ColumnType::WindSpeed), measured_means(ColumnType::WindDirection));
    let mut pairs = Vec::new();
    for (position, &(a, height)) in speeds.iter().enumerate() {
        for &(b, other_height) in &speeds[position + 1..] {
            if (height - other_height).abs() > threshold_m {
                continue;
            }
            let nearest = directions.iter().min_by(|left, right| (left.1 - height).abs().total_cmp(&(right.1 - height).abs()));
            if let Some(&(direction, _)) = nearest {
                pairs.push((a, b, direction));
            }
        }
    }
    pairs
}

/// The anemometer pairs of the dataset and their patterns, under default
/// filtering that keeps `request.flag`. One pass over the record per pair.
///
/// # Errors
/// [`CoreError::NoSuchFlag`] for a flag the dataset lacks.
pub fn tower_shading(prepared: Prepared, request: TowerShadingRequest) -> CoreResult<TowerShadingDto> {
    let dataset = prepared.data;
    if request.flag >= dataset.flags.len() {
        return Err(CoreError::NoSuchFlag { flag: request.flag });
    }
    let view = FilterView::keeping(dataset, request.flag);
    let pairs = colocated_pairs(dataset, COLOCATION_THRESHOLD_M)
        .into_iter()
        .map(|(a, b, direction)| {
            let pattern = shading::detect(dataset, &view, a, b, direction)?;
            Ok(ShadingPairDto {
                a: dataset.columns[a].id.clone(),
                b: dataset.columns[b].id.clone(),
                direction: dataset.columns[direction].id.clone(),
                height_m: dataset.columns[a].height_m.unwrap_or_default(),
                medians: finite_all(&pattern.medians),
                counts: pattern.counts,
                shaded_a: pattern.shaded_a,
                shaded_b: pattern.shaded_b,
            })
        })
        .collect::<CoreResult<_>>()?;
    Ok(TowerShadingDto { sectors: SHADING_SECTORS, default_width_deg: DEFAULT_SHADED_WIDTH_DEG, pairs })
}

#[cfg(test)]
mod contract {
    use super::*;
    use crate::rules::CriterionDto;
    use wind_core::import::{import_text, ImportConfig};
    use wind_core::rules::Criterion;

    /// Two anemometers at 30 m, one at 20 m, vanes at 28 and 20 m.
    const CSV: &str = "Timestamp,Vel 30m A,Vel 30m B,Vel 20m,Dir 28m,Dir 20m\n\
                       2024-01-01 00:00,5,5,4,90,90\n2024-01-01 00:10,6,6,5,95,95\n";

    #[test]
    fn colocated_anemometers_pair_up_with_the_nearest_vane() {
        let (dataset, _) = import_text("torre", CSV, &ImportConfig::default()).unwrap();
        let ids = |(a, b, direction): (usize, usize, usize)| {
            [a, b, direction].map(|column| dataset.columns[column].id.clone())
        };
        let pairs: Vec<_> = colocated_pairs(&dataset, COLOCATION_THRESHOLD_M).into_iter().map(ids).collect();
        assert_eq!(pairs, [["Vel 30m A", "Vel 30m B", "Dir 28m"]], "the 20 m anemometer has no partner");
    }

    #[test]
    fn a_sector_rule_names_its_vane_by_id() {
        let (dataset, _) = import_text("torre", CSV, &ImportConfig::default()).unwrap();
        let json = r#"{"kind": "direction_in_sector", "direction": "Dir 28m", "center": 305, "width": 30}"#;
        let dto: CriterionDto = serde_json::from_str(json).unwrap();
        let criterion = dto.clone().into_core(&dataset).unwrap();
        assert_eq!(criterion, Criterion::DirectionInSector { direction: 3, center: 305.0, width: 30.0 });
        assert_eq!(CriterionDto::new(&dataset, criterion), dto, "and comes back the same");
    }
}

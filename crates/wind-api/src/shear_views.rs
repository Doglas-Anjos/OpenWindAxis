//! The Wind Shear window's displays on the wire
//! ([`wind_core::analysis::shear_view`], [`wind_core::analysis::shear_frequency`]):
//! the mean speed, speed SD or direction shear by up to two divisions, and
//! the frequency of the step-by-step parameter.
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::{column_index, column_indices};
use crate::{finite, finite_all, DatasetId, ShearAggregation};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis::{self, Division, HeightFitKind, ShearQuantity, ShearViewOptions};
use wind_core::error::CoreResult;
use wind_core::model::{ColumnType, Dataset, Subtype};
use wind_core::primary;

/// What a shear display's profile is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ShearQuantityDto {
    /// Mean speed: α and z₀. The default.
    #[default]
    Speed,
    /// Mean speed SD: α only.
    SpeedSd,
    /// Mean direction: the veer rate, °/100 m.
    Direction,
}

impl From<ShearQuantityDto> for ShearQuantity {
    fn from(quantity: ShearQuantityDto) -> Self {
        match quantity {
            ShearQuantityDto::Speed => ShearQuantity::Speed,
            ShearQuantityDto::SpeedSd => ShearQuantity::SpeedSd,
            ShearQuantityDto::Direction => ShearQuantity::Direction,
        }
    }
}

/// The magnitude of each vane's vector mean (manual dlg-analyze-wind-shear.htm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum VectorMagnitudeDto {
    /// The mean speed nearest each vane in height.
    Associated,
    /// The dataset's primary speed, the same for every vane: the default.
    #[default]
    Primary,
    /// None: unit vectors.
    Unit,
}

/// One way of subdividing the steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShearDivisionDto {
    /// Hour of day, 0–23.
    Hour,
    /// Calendar month, January first.
    Month,
    /// Each month of the record.
    MonthOfRecord,
    /// Direction sectors of a column.
    Sector {
        /// Direction column id.
        direction: String,
        /// 1–360.
        sectors: usize,
    },
    /// Bins of a column.
    Bins {
        /// Column id.
        column: String,
        /// Bin width, in the column's units.
        width: f64,
    },
}

/// A shear display.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShearViewRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Omitted = the mean speed.
    #[serde(default)]
    #[ts(optional)]
    pub quantity: Option<ShearQuantityDto>,
    /// The profile's column ids (speeds, speed SDs or directions), at two or more heights.
    pub columns: Vec<String>,
    /// Direction only; omitted = the primary speed.
    #[serde(default)]
    #[ts(optional)]
    pub magnitude: Option<VectorMagnitudeDto>,
    /// At most two; omitted = one profile.
    #[serde(default)]
    #[ts(optional)]
    pub divisions: Option<Vec<ShearDivisionDto>>,
    /// Omitted = AP-MoMM.
    #[serde(default)]
    #[ts(optional)]
    pub aggregation: Option<ShearAggregation>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// The groups along one division.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DivisionAxisDto {
    /// How many.
    pub groups: usize,
    /// Bins: the first bin's lower edge; 0 otherwise.
    pub start: f64,
    /// Bins: their width; 0 otherwise.
    pub width: f64,
    /// Month of the record: `[year, month]` of each group, 1 = January.
    pub months: Vec<(i32, u32)>,
}

/// The profile of one group; `null` where it cannot be computed.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShearCellDto {
    /// Steps in the group.
    pub samples: usize,
    /// Each column's mean (m/s, or the vector mean direction in degrees).
    pub means: Vec<Option<f64>>,
    /// Direction: each vane's magnitude speed mean, the hodograph arrow.
    pub magnitudes: Vec<Option<f64>>,
    /// α, or the veer rate in °/100 m.
    pub parameter: Option<f64>,
    /// Speed: log-law roughness length, m.
    pub roughness_m: Option<f64>,
    /// The displacement height averaged like the means, m.
    pub displacement_m: Option<f64>,
}

/// Which heights a fit goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HeightFitKindDto {
    /// Every selected height.
    All,
    /// A sensor at the lowest height and one at the highest.
    Extremes,
    /// A sensor at one height and one at the next.
    Neighbours,
}

impl From<HeightFitKind> for HeightFitKindDto {
    fn from(kind: HeightFitKind) -> Self {
        match kind {
            HeightFitKind::All => HeightFitKindDto::All,
            HeightFitKind::Extremes => HeightFitKindDto::Extremes,
            HeightFitKind::Neighbours => HeightFitKindDto::Neighbours,
        }
    }
}

/// A fit through some of the heights.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HeightFitDto {
    /// Which heights.
    pub kind: HeightFitKindDto,
    /// Column ids, lowest first.
    pub columns: Vec<String>,
    /// Power-law exponent.
    pub alpha: Option<f64>,
    /// Log-law roughness length, m.
    pub roughness_m: Option<f64>,
}

/// A shear display's answer.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShearViewDto {
    /// What the profile is made of.
    pub quantity: ShearQuantityDto,
    /// How it was averaged.
    pub aggregation: ShearAggregation,
    /// Height of each column, in request order, m.
    pub heights_m: Vec<f64>,
    /// Direction: the speed column weighing each vane, `null` for unit vectors.
    pub magnitude_columns: Vec<Option<String>>,
    /// One per division.
    pub axes: Vec<DivisionAxisDto>,
    /// One per group, the first division outermost.
    pub cells: Vec<ShearCellDto>,
    /// Undivided speed by an aggregate method: every height, the lowest and
    /// highest, then each neighbouring pair (sensors sharing a height each
    /// pair with the other height's).
    pub fits: Vec<HeightFitDto>,
}

/// The step-by-step parameter's frequency.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShearFrequencyRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Omitted = the mean speed.
    #[serde(default)]
    #[ts(optional)]
    pub quantity: Option<ShearQuantityDto>,
    /// The profile's column ids, at two or more heights.
    pub columns: Vec<String>,
    /// Bin width: of α, or °/100 m.
    pub width: f64,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// The step-by-step parameter in bins.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShearFrequencyDto {
    /// Steps with a parameter.
    pub samples: usize,
    /// Mean parameter.
    pub mean: Option<f64>,
    /// Bin `i` is `[start + i·width, start + (i+1)·width)`.
    pub start: f64,
    /// Bin width.
    pub width: f64,
    /// Share of the steps per bin.
    pub frequency: Vec<f64>,
}

/// Each vane's magnitude column by `magnitude`.
fn magnitude_columns(dataset: &Dataset, vanes: &[usize], magnitude: VectorMagnitudeDto) -> Vec<Option<usize>> {
    let speeds: Vec<usize> = (0..dataset.columns.len())
        .filter(|&index| dataset.columns[index].kind == ColumnType::WindSpeed && dataset.columns[index].subtype == Subtype::Mean)
        .collect();
    let nearest = |vane: usize| {
        let height = dataset.columns[vane].height_m.unwrap_or_default();
        speeds.iter().copied().min_by(|&a, &b| {
            let distance = |index: usize| (dataset.columns[index].height_m.unwrap_or_default() - height).abs();
            distance(a).total_cmp(&distance(b))
        })
    };
    vanes
        .iter()
        .map(|&vane| match magnitude {
            VectorMagnitudeDto::Associated => nearest(vane),
            VectorMagnitudeDto::Primary => primary::primary(dataset, ColumnType::WindSpeed),
            VectorMagnitudeDto::Unit => None,
        })
        .collect()
}

fn division(dataset: &Dataset, division: ShearDivisionDto) -> CoreResult<Division> {
    Ok(match division {
        ShearDivisionDto::Hour => Division::Hour,
        ShearDivisionDto::Month => Division::Month,
        ShearDivisionDto::MonthOfRecord => Division::MonthOfRecord,
        ShearDivisionDto::Sector { direction, sectors } => Division::Sector { direction: column_index(dataset, &direction)?, sectors },
        ShearDivisionDto::Bins { column, width } => Division::Bins { column: column_index(dataset, &column)?, width },
    })
}

/// A shear display of `prepared`.
///
/// # Errors
/// [`wind_core::error::CoreError::UnknownColumn`], or those of [`analysis::shear_view`].
pub fn shear_view(prepared: Prepared, request: ShearViewRequest) -> CoreResult<ShearViewDto> {
    let dataset = prepared.data;
    let columns = column_indices(dataset, &request.columns)?;
    let quantity = request.quantity.unwrap_or_default();
    let magnitudes = match quantity {
        ShearQuantityDto::Direction => magnitude_columns(dataset, &columns, request.magnitude.unwrap_or_default()),
        _ => Vec::new(),
    };
    let divisions = request.divisions.unwrap_or_default().into_iter().map(|part| division(dataset, part)).collect::<CoreResult<_>>()?;
    let aggregation = request.aggregation.unwrap_or(ShearAggregation::ApMomm);
    let options = ShearViewOptions { quantity: quantity.into(), columns, magnitudes: magnitudes.clone(), divisions, aggregation: aggregation.into() };
    let view = prepared.view(request.filter)?;
    let shear = analysis::shear_view(dataset, &view, &options)?;
    let id = |index: usize| dataset.columns[index].id.clone();
    Ok(ShearViewDto {
        quantity,
        aggregation,
        heights_m: shear.heights,
        magnitude_columns: magnitudes.into_iter().map(|column| column.map(id)).collect(),
        axes: shear
            .axes
            .into_iter()
            .map(|axis| DivisionAxisDto { groups: axis.groups, start: axis.start, width: axis.width, months: axis.months })
            .collect(),
        cells: shear
            .cells
            .into_iter()
            .map(|cell| ShearCellDto {
                samples: cell.samples,
                means: finite_all(&cell.means),
                magnitudes: finite_all(&cell.magnitudes),
                parameter: finite(cell.parameter),
                roughness_m: finite(cell.roughness_m),
                displacement_m: finite(cell.displacement_m),
            })
            .collect(),
        fits: shear
            .fits
            .into_iter()
            .map(|fit| HeightFitDto {
                kind: fit.kind.into(),
                columns: fit.columns.iter().map(|&position| id(options.columns[position])).collect(),
                alpha: finite(fit.alpha),
                roughness_m: finite(fit.roughness_m),
            })
            .collect(),
    })
}

/// The step-by-step parameter's frequency in `prepared`.
///
/// # Errors
/// [`wind_core::error::CoreError::UnknownColumn`], or those of [`analysis::shear_frequency`].
pub fn shear_frequency(prepared: Prepared, request: ShearFrequencyRequest) -> CoreResult<ShearFrequencyDto> {
    let dataset = prepared.data;
    let options = ShearViewOptions {
        quantity: request.quantity.unwrap_or_default().into(),
        columns: column_indices(dataset, &request.columns)?,
        magnitudes: Vec::new(),
        divisions: Vec::new(),
        aggregation: analysis::ShearAggregation::TsMean,
    };
    let view = prepared.view(request.filter)?;
    let frequency = analysis::shear_frequency(dataset, &view, &options, request.width)?;
    Ok(ShearFrequencyDto {
        samples: frequency.samples,
        mean: finite(frequency.mean),
        start: frequency.start,
        width: frequency.width,
        frequency: frequency.frequency,
    })
}

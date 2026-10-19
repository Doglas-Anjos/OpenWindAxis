//! Analyses over default-filtered data: data coverage, histogram with a
//! Weibull fit, wind rose, diurnal profile and wind shear. Every function
//! walks the valid runs of a [`FilterView`], so excluded data never counts
//! and no column is copied (ARQUITETURA P2). Sums are compensated (P12).
//!
//! Output field names are Rust names; `#[serde(rename)]` keeps the JSON the
//! web UI reads today until the DTO layer takes over the wire.
use crate::acc::DEGREES_PER_TURN;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::derived;

/// Air density of the ISO 2533 standard atmosphere at sea level, kg/m³: what
/// energy figures use when the dataset has no air density column
/// ([`crate::derived`]).
pub const RHO_STD: f64 = 1.225;
/// Most direction sectors a rose or a shear-by-sector accepts (one per degree).
pub const MAX_SECTORS: usize = 360;
/// Most histogram bins: beyond this a width is almost surely a mistake or an outlier.
pub const MAX_HISTOGRAM_BINS: usize = 10_000;
/// Air density per step for energy figures: the dataset's air density
/// column where it has a value the filter keeps, [`RHO_STD`] elsewhere.
pub(super) struct DensityLookup<'a> {
    pub(super) column: Option<(&'a [f64], crate::filter::Cursor<'a>)>,
}

impl<'a> DensityLookup<'a> {
    pub(super) fn new(dataset: &'a Dataset, view: &'a FilterView) -> Self {
        let column = derived::air_density_column(dataset).map(|column| (&dataset.columns[column].values[..], view.cursor(column)));
        DensityLookup { column }
    }

    /// ρ at `step`, kg/m³. Steps must not decrease between calls.
    #[inline]
    pub(super) fn at(&mut self, step: usize) -> f64 {
        let Some((values, excluded)) = self.column.as_mut() else { return RHO_STD };
        // The cursor advances on every call, so steps must keep increasing.
        let kept = !excluded.excludes(step);
        if kept && values[step].is_finite() {
            values[step]
        } else {
            RHO_STD
        }
    }
}

/// Sector of a direction; sector 0 is centred on north, counting clockwise.
pub(crate) fn sector_of(direction: f64, sectors: usize) -> usize {
    let width = DEGREES_PER_TURN / sectors as f64;
    ((direction.rem_euclid(DEGREES_PER_TURN) + width / 2.0) / width) as usize % sectors
}

/// `part / whole`, NaN when `whole` is 0.
pub(super) fn share(part: f64, whole: f64) -> f64 {
    if whole > 0.0 {
        part / whole
    } else {
        f64::NAN
    }
}

/// Fails with [`CoreError::SectorsOutOfRange`] outside `1..=MAX_SECTORS`.
pub(crate) fn check_sectors(sectors: usize) -> CoreResult<()> {
    if (1..=MAX_SECTORS).contains(&sectors) {
        Ok(())
    } else {
        Err(CoreError::SectorsOutOfRange { sectors, max: MAX_SECTORS })
    }
}

mod coverage;
mod distortion;
mod distribution;
mod extremes;
mod histogram;
mod rose;
mod scatter;
mod diurnal;
mod monthly;
mod shear;
mod shear_views;
mod turbulence;

pub use coverage::*;
pub use distortion::*;
pub use distribution::*;
pub use extremes::*;
pub use histogram::*;
pub use rose::*;
pub use scatter::*;
pub use diurnal::*;
pub use monthly::*;
pub use shear::*;
pub use shear_views::*;
pub use turbulence::*;

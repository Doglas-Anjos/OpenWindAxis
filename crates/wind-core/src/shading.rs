//! Tower shading: an anemometer downwind of its tower reads low. The
//! "Sombra da torre" window finds where from a pair of colocated anemometers
//! and writes the flag rules ([`Criterion::DirectionInSector`]) that flag
//! each one while the wind blows across the tower onto it (manual
//! dlg-flag-flag-tower-shading.htm). The details below match the original's
//! results (ARQUITETURA, 2026-10-17):
//! - **Ratio:** A/B in each step where A, B and the direction are valid and
//!   B is not zero; no speed threshold.
//! - **Sectors:** 72, the first centred on north; each keeps the median of
//!   its ratios.
//! - **Centres:** the sector with the lowest median is where A is shaded,
//!   the one with the highest where B is; only sectors with more than 20
//!   ratios compete, and ties go to the first. The median is nearest-rank,
//!   as the original's percentiles are ([`nearest_rank`]): the upper middle
//!   value of an even sector.
//! - **Width:** set by the user, [`DEFAULT_SHADED_WIDTH_DEG`] by default.
//! - **Filtering:** default filtering, but the data under the flag the rules
//!   apply stays ([`FilterView::keeping`]), so the pattern survives flagging.
//!
//! [`Criterion::DirectionInSector`]: crate::rules::Criterion::DirectionInSector
use crate::acc::DEGREES_PER_TURN;
use crate::analysis::{nearest_rank, sector_of};
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;

/// Direction sectors of the ratio pattern.
pub const SHADING_SECTORS: usize = 72;
/// Ratios a sector needs to compete for a shaded centre: the original
/// requires more than 20.
pub const MIN_SECTOR_RATIOS: usize = 21;
/// The median as a percentile.
const MEDIAN_PERCENT: f64 = 50.0;
/// Width of a shaded sector unless the user says otherwise, degrees: the
/// original's default (Tools › Options › Advanced).
pub const DEFAULT_SHADED_WIDTH_DEG: f64 = 30.0;

/// The ratio pattern of one anemometer pair against direction.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadingPattern {
    /// Median of A/B per sector, sector `i` centred on `i · 360/72`°; NaN
    /// where the sector has no ratio.
    pub medians: Vec<f64>,
    /// Ratios per sector.
    pub counts: Vec<usize>,
    /// Centre of the sector where A is shaded (lowest median), degrees;
    /// `None` when no sector has [`MIN_SECTOR_RATIOS`].
    pub shaded_a: Option<f64>,
    /// Centre of the sector where B is shaded (highest median), degrees.
    pub shaded_b: Option<f64>,
}

/// The ratio pattern of speeds `a` and `b` against `direction` under `view`.
/// O(steps + steps·log(steps/72)) for the medians; the ratios are held once.
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for an index `dataset` lacks.
pub fn detect(dataset: &Dataset, view: &FilterView, a: usize, b: usize, direction: usize) -> CoreResult<ShadingPattern> {
    if let Some(&column) = [a, b, direction].iter().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column });
    }
    let (speeds_a, speeds_b, directions) = (&dataset.columns[a].values, &dataset.columns[b].values, &dataset.columns[direction].values);
    let mut ratios = vec![Vec::new(); SHADING_SECTORS];
    for step in view.valid_runs(&[a, b, direction], 0..dataset.len).into_iter().flatten() {
        let (speed_a, speed_b, degrees) = (speeds_a[step], speeds_b[step], directions[step]);
        if speed_b != 0.0 && speed_a.is_finite() && speed_b.is_finite() && degrees.is_finite() {
            ratios[sector_of(degrees, SHADING_SECTORS)].push(speed_a / speed_b);
        }
    }
    let counts: Vec<usize> = ratios.iter().map(Vec::len).collect();
    let medians: Vec<f64> = ratios
        .into_iter()
        .map(|mut sector| {
            sector.sort_unstable_by(f64::total_cmp);
            nearest_rank(&sector, MEDIAN_PERCENT)
        })
        .collect();
    let center = |sector: usize| sector as f64 * DEGREES_PER_TURN / SHADING_SECTORS as f64;
    let competing = || (0..SHADING_SECTORS).filter(|&sector| counts[sector] >= MIN_SECTOR_RATIOS);
    // min_by keeps the first of equals and max_by the last, so the highest is sought backwards.
    let shaded_a = competing().min_by(|&left, &right| medians[left].total_cmp(&medians[right])).map(center);
    let shaded_b = competing().rev().max_by(|&left, &right| medians[left].total_cmp(&medians[right])).map(center);
    Ok(ShadingPattern { medians, counts, shaded_a, shaded_b })
}


//! Wind rose: frequency, mean speed, energy and calms by direction sector.
use crate::acc::Sum;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use serde::Serialize;
use super::{check_sectors, sector_of, share, DensityLookup};

/// Frequency, speed and energy by direction sector.
#[derive(Debug, Clone, Serialize)]
pub struct Rose {
    /// Steps with a valid direction (and speed, when one is given), calms
    /// included.
    #[serde(rename = "n")]
    pub samples: usize,
    /// Per sector, share of `samples`; sector 0 is centred on north, counting
    /// clockwise. With the calms, the shares add up to 1.
    #[serde(rename = "freq")]
    pub frequency: Vec<f64>,
    /// Share of `samples` below the calm threshold: no direction at all.
    pub calm: f64,
    /// Mean speed per sector; NaN without a speed column or samples.
    pub mean_speed: Vec<f64>,
    /// Share of the total energy Σ½·ρ·u³ per sector, with the air density
    /// per step as in the histogram.
    pub energy: Vec<f64>,
    /// Upper edges of the speed classes; one more open-ended class follows.
    pub speed_edges: Vec<f64>,
    /// `[class][sector]`: share of all samples.
    #[serde(rename = "freq_by_speed")]
    pub frequency_by_speed: Vec<Vec<f64>>,
}

/// Wind rose of `direction` in `sectors` sectors, with speed classes split at
/// `speed_edges` when a `speed` column is given. One pass over the steps where
/// both columns survive filtering.
///
/// # Errors
/// [`CoreError::SectorsOutOfRange`] outside `1..=MAX_SECTORS`.
pub fn wind_rose(
    dataset: &Dataset,
    view: &FilterView,
    direction: usize,
    speed: Option<usize>,
    sectors: usize,
    speed_edges: &[f64],
    calm_threshold: f64,
) -> CoreResult<Rose> {
    check_sectors(sectors)?;
    if !(calm_threshold >= 0.0 && calm_threshold.is_finite()) {
        return Err(CoreError::CalmThresholdInvalid { calm_threshold });
    }
    let columns: Vec<usize> = std::iter::once(direction).chain(speed).collect();
    let runs = view.valid_runs(&columns, 0..dataset.len);
    let directions = &dataset.columns[direction].values;
    let speeds = speed.map(|column| &dataset.columns[column].values[..]);
    let mut counts = vec![0usize; sectors];
    let (mut speed_sums, mut cube_sums) = (vec![Sum::ZERO; sectors], vec![Sum::ZERO; sectors]);
    let speed_classes = speed_edges.len() + 1;
    let mut counts_by_class = vec![vec![0usize; sectors]; speed_classes];
    let mut calms = 0usize;
    let mut density = DensityLookup::new(dataset, view);
    for run in &runs {
        for step in run.clone() {
            let speed_value = speeds.map_or(0.0, |speeds| speeds[step]);
            if directions[step].is_nan() || speed_value.is_nan() {
                continue;
            }
            // Without a speed there is no calm (manual def-calm-threshold.htm).
            if speeds.is_some() && speed_value < calm_threshold {
                calms += 1;
                continue;
            }
            let sector = sector_of(directions[step], sectors);
            counts[sector] += 1;
            speed_sums[sector].add(speed_value);
            cube_sums[sector].add(density.at(step) * speed_value * speed_value * speed_value);
            let class = speed_edges.iter().position(|&edge| speed_value < edge).unwrap_or(speed_edges.len());
            counts_by_class[class][sector] += 1;
        }
    }
    let samples: usize = counts.iter().sum::<usize>() + calms;
    let total_cube: f64 = cube_sums.iter().map(Sum::value).sum();
    let has_speed = speed.is_some();
    let shares_of_all = |row: &Vec<usize>| row.iter().map(|&count| share(count as f64, samples as f64)).collect();
    Ok(Rose {
        samples,
        frequency: shares_of_all(&counts),
        calm: share(calms as f64, samples as f64),
        mean_speed: (0..sectors)
            .map(|sector| if has_speed { share(speed_sums[sector].value(), counts[sector] as f64) } else { f64::NAN })
            .collect(),
        energy: cube_sums.iter().map(|cubes| if has_speed { share(cubes.value(), total_cube) } else { f64::NAN }).collect(),
        speed_edges: if has_speed { speed_edges.to_vec() } else { Vec::new() },
        frequency_by_speed: if has_speed { counts_by_class.iter().map(shares_of_all).collect() } else { Vec::new() },
    })
}

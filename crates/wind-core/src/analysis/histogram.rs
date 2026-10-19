//! Histogram of one column, with an optional Weibull fit and the wind power density.
use crate::acc::Sum;
use crate::error::{CoreError, CoreResult};
use crate::filter::{valid_values, FilterView};
use crate::model::Dataset;
use crate::weibull::{self, Method};
use serde::Serialize;
use super::{DensityLookup, MAX_HISTOGRAM_BINS};

/// Half of ½·ρ·u³, the wind power density.
const HALF: f64 = 0.5;

/// Distribution of one column after filtering.
#[derive(Debug, Clone, Serialize)]
pub struct Histogram {
    /// Samples that passed the filter.
    #[serde(rename = "n")]
    pub samples: usize,
    /// Arithmetic mean of the samples.
    pub mean: f64,
    /// Bin `i` is `[start + i·width, start + (i+1)·width)`.
    pub start: f64,
    /// Bin width, in the column's units.
    pub width: f64,
    /// Share of samples per bin.
    #[serde(rename = "freq")]
    pub frequency: Vec<f64>,
    /// Mean wind power density ½·ρ·u³, W/m² (meaningful for speeds), with
    /// the dataset's air density per step where it has one, else `RHO_STD`.
    #[serde(rename = "wpd")]
    pub power_density: f64,
    /// Mean air density used for `power_density`, kg/m³: `RHO_STD` for a
    /// dataset without an air density column.
    pub air_density: f64,
    /// Whether some of that density was measured (an air density column).
    pub density_measured: bool,
    /// Energy pattern factor mean(u³)/mean(u)³.
    #[serde(rename = "epf")]
    pub energy_pattern_factor: f64,
    /// Weibull curve, when a fit was asked for.
    pub weibull: Option<WeibullFit>,
}

/// A Weibull fit drawn over a histogram.
#[derive(Debug, Clone, Serialize)]
pub struct WeibullFit {
    /// Shape parameter k.
    pub k: f64,
    /// Scale parameter A, in the column's units.
    pub a: f64,
    /// Mean of the fitted distribution, A·Γ(1 + 1/k).
    pub mean: f64,
    /// Fitted share per bin, aligned with `Histogram::frequency`.
    #[serde(rename = "freq")]
    pub frequency: Vec<f64>,
}

/// Histogram of `column` in bins of `width`, with an optional Weibull fit.
///
/// Two passes over the valid runs (range and moments, then counts), no copy
/// of the column. With `fit`, the valid samples are collected once, because
/// the Weibull fits need raw samples (ARQUITETURA §4.3, raw path).
///
/// # Errors
/// [`CoreError::BinWidthNotPositive`], [`CoreError::NoDataAfterFilter`],
/// [`CoreError::TooManyBins`] (beyond [`MAX_HISTOGRAM_BINS`]), or a failed fit.
pub fn histogram(dataset: &Dataset, view: &FilterView, column: usize, width: f64, fit: Option<Method>) -> CoreResult<Histogram> {
    if !(width > 0.0) {
        return Err(CoreError::BinWidthNotPositive);
    }
    let values = &dataset.columns[column].values;
    let runs = view.valid_runs(&[column], 0..dataset.len);
    let (mut samples, mut low, mut high) = (0usize, f64::INFINITY, f64::NEG_INFINITY);
    let (mut sum, mut sum_cubes, mut sum_power, mut sum_density) = (Sum::ZERO, Sum::ZERO, Sum::ZERO, Sum::ZERO);
    let mut density = DensityLookup::new(dataset, view);
    for run in &runs {
        for step in run.clone() {
            let value = values[step];
            if value.is_nan() {
                continue;
            }
            let cube = value * value * value;
            let rho = density.at(step);
            samples += 1;
            low = low.min(value);
            high = high.max(value);
            sum.add(value);
            sum_cubes.add(cube);
            sum_power.add(HALF * rho * cube);
            sum_density.add(rho);
        }
    }
    if samples == 0 {
        return Err(CoreError::NoDataAfterFilter);
    }
    let start = (low / width).floor() * width;
    // Checked as f64: a huge outlier (a logger error code like 9.9e37) would
    // saturate the usize cast and wrap past the limit.
    let bins = ((high - start) / width).floor() + 1.0;
    if !(bins <= MAX_HISTOGRAM_BINS as f64) {
        return Err(CoreError::TooManyBins { bins, low, high, max: MAX_HISTOGRAM_BINS });
    }
    let bin_count = bins as usize;
    let mut counts = vec![0usize; bin_count];
    for value in valid_values(values, &runs) {
        counts[(((value - start) / width) as usize).min(bin_count - 1)] += 1;
    }
    let total = samples as f64;
    let (mean, mean_cube) = (sum.value() / total, sum_cubes.value() / total);
    let weibull = match fit {
        None => None,
        Some(method) => {
            let mut speeds = Vec::with_capacity(samples);
            speeds.extend(valid_values(values, &runs));
            let fitted = weibull::fit(&speeds, method)?;
            let edge = |bin: usize| start + bin as f64 * width;
            Some(WeibullFit {
                k: fitted.k,
                a: fitted.a,
                mean: fitted.mean(),
                frequency: (0..bin_count).map(|bin| fitted.cdf(edge(bin + 1)) - fitted.cdf(edge(bin))).collect(),
            })
        }
    };
    Ok(Histogram {
        samples,
        mean,
        start,
        width,
        frequency: counts.iter().map(|&count| count as f64 / total).collect(),
        power_density: sum_power.value() / total,
        air_density: sum_density.value() / total,
        density_measured: density.column.is_some(),
        energy_pattern_factor: mean_cube / (mean * mean * mean),
        weibull,
    })
}

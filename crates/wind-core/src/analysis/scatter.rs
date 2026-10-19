//! Scatter plot (ESPEC-INTERFACE §1, Analisar › Dispersão): one column
//! against another over the steps where both are valid, with the least
//! squares line y = a·x + b and its R² over all of them. A chart cannot draw
//! a million points usefully, so at most `max_points` are returned: every
//! k-th valid step, in time order, so the cloud keeps its shape and the
//! sample is the same on every run.
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;

/// Fewest steps a line takes.
const MIN_POINTS: usize = 2;

/// One column against another.
#[derive(Debug, Clone, PartialEq)]
pub struct Scatter {
    /// The sampled points `(x, y)`, in time order.
    pub points: Vec<(f64, f64)>,
    /// Steps where both columns are valid (all of them, not only the sample).
    pub count: usize,
    /// One point drawn per this many valid steps.
    pub stride: usize,
    /// Slope a of the least squares line.
    pub slope: f64,
    /// Intercept b.
    pub intercept: f64,
    /// Coefficient of determination R² of the line (the squared correlation).
    pub r_squared: f64,
}

/// `y` against `x` under `view`, with at most `max_points` points drawn.
/// Two passes over the valid steps: means, then centred sums (stable for
/// values far from zero, such as pressures).
///
/// # Errors
/// [`CoreError::NoDataAfterFilter`] with fewer than two steps where both are
/// valid.
pub fn scatter(dataset: &Dataset, view: &FilterView, x: usize, y: usize, max_points: usize) -> CoreResult<Scatter> {
    let (xs, ys) = (&dataset.columns[x].values, &dataset.columns[y].values);
    let steps: Vec<usize> = view
        .valid_runs(&[x, y], 0..dataset.len)
        .into_iter()
        .flatten()
        .filter(|&step| !xs[step].is_nan() && !ys[step].is_nan())
        .collect();
    if steps.len() < MIN_POINTS {
        return Err(CoreError::NoDataAfterFilter);
    }
    let count = steps.len() as f64;
    let (mean_x, mean_y) = steps.iter().fold((0.0, 0.0), |(sum_x, sum_y), &step| (sum_x + xs[step], sum_y + ys[step]));
    let (mean_x, mean_y) = (mean_x / count, mean_y / count);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for &step in &steps {
        let (dx, dy) = (xs[step] - mean_x, ys[step] - mean_y);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let slope = sxy / sxx;
    let stride = steps.len().div_ceil(max_points.max(1));
    Ok(Scatter {
        points: steps.iter().step_by(stride).map(|&step| (xs[step], ys[step])).collect(),
        count: steps.len(),
        stride,
        slope,
        intercept: mean_y - slope * mean_x,
        r_squared: sxy * sxy / (sxx * syy),
    })
}

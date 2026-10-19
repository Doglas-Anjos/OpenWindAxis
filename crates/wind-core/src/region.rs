//! The steps a shape drawn on a scatter plot holds (Flag › Flag por
//! dispersão, manual dlg-flag-flag-by-scatter-plot.htm): the window flags or
//! unflags them as one edit ([`crate::ops::Op::FlagSteps`]). A shape is a
//! polygon in the plot's data units: the free-form, rectangular and
//! triangular selections of the original are all polygons. The plot draws a
//! sample of the points, so the selection is resolved here, over every step
//! the window's filter keeps, not over the points drawn.
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use std::ops::Range;

/// Fewest corners a shape needs.
pub const MIN_VERTICES: usize = 3;

/// Runs of steps, sorted and disjoint, where `x` and `y` are both kept by
/// `view` and the point (x, y) lies inside any of `shapes` (even-odd rule).
/// O(steps · vertices) at worst; a bounding box skips most points.
///
/// # Errors
/// [`CoreError::NoSuchColumn`] for an index `dataset` lacks;
/// [`CoreError::ShapeTooFewVertices`] for a shape of fewer than three corners.
pub fn steps_inside(dataset: &Dataset, view: &FilterView, x: usize, y: usize, shapes: &[Vec<[f64; 2]>]) -> CoreResult<Vec<Range<usize>>> {
    if let Some(&column) = [x, y].iter().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column });
    }
    if let Some(shape) = shapes.iter().find(|shape| shape.len() < MIN_VERTICES) {
        return Err(CoreError::ShapeTooFewVertices { vertices: shape.len() });
    }
    let boxes: Vec<[f64; 4]> = shapes
        .iter()
        .map(|shape| {
            shape.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |[x0, x1, y0, y1], &[px, py]| {
                [x0.min(px), x1.max(px), y0.min(py), y1.max(py)]
            })
        })
        .collect();
    let (xs, ys) = (&dataset.columns[x].values, &dataset.columns[y].values);
    let held = |step: usize| {
        let point = [xs[step], ys[step]];
        point[0].is_finite()
            && point[1].is_finite()
            && shapes.iter().zip(&boxes).any(|(shape, &[x0, x1, y0, y1])| {
                (x0..=x1).contains(&point[0]) && (y0..=y1).contains(&point[1]) && inside(point, shape)
            })
    };
    let mut runs: Vec<Range<usize>> = Vec::new();
    for step in view.valid_runs(&[x, y], 0..dataset.len).into_iter().flatten() {
        if !held(step) {
            continue;
        }
        match runs.last_mut() {
            Some(run) if run.end == step => run.end += 1,
            _ => runs.push(step..step + 1),
        }
    }
    Ok(runs)
}

/// Whether `point` lies inside `polygon` by the even-odd rule: a ray to the
/// right crosses its edges an odd number of times.
fn inside([px, py]: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut crossings = false;
    let mut previous = polygon[polygon.len() - 1];
    for &corner in polygon {
        let ([x0, y0], [x1, y1]) = (previous, corner);
        if (y0 > py) != (y1 > py) && px < x0 + (py - y0) / (y1 - y0) * (x1 - x0) {
            crossings = !crossings;
        }
        previous = corner;
    }
    crossings
}

#[cfg(test)]
mod tests {
    use super::inside;

    #[test]
    fn the_even_odd_rule_tells_inside_from_outside() {
        let triangle = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        assert!(inside([2.0, 2.0], &triangle));
        assert!(!inside([6.0, 6.0], &triangle), "past the hypotenuse");
        assert!(!inside([-1.0, 1.0], &triangle));
        // A bow tie: the crossing point's lobes are both inside.
        let bow_tie = [[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 4.0]];
        assert!(inside([1.0, 2.0], &bow_tie) && inside([3.0, 2.0], &bow_tie));
        assert!(!inside([2.0, 3.5], &bow_tie));
    }
}

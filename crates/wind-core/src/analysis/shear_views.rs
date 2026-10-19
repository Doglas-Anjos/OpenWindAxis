//! The Wind Shear window's displays beyond the overall profile (manual
//! dlg-analyze-wind-shear.htm; ARQUITETURA 2026-10-18): the shear of the
//! mean speed, of the speed SD and of the direction (veer), subdivided by up
//! to two of hour, month, month of the record, direction sector and bins of
//! any column, by any of the four aggregation methods; the fits through each
//! pair of heights; and the frequency of the parameter step by step.
//!
//! As the original computes them (its manual and its results):
//! - the veer of a profile: each vane's direction relative to the first
//!   vane's, wrapped to [−180°, 180°), fitted against height by least squares
//!   (direction on height); the slope × 100, in °/100 m, positive when the
//!   wind turns clockwise going up. Over many steps, each vane's direction is
//!   its vector mean, weighed by a speed column or by unit vectors;
//! - the speed SD shear is always a power law;
//! - the time-series methods fit every step and take the mean or the median
//!   of the step parameters, each step with its own displacement height;
//! - MoMM inside a subdivision drops the completeness factors: each calendar
//!   month weighs its mean days (the manual's "highly subdivided cases").
//!
//! Only the steps where every column involved (the profile's, the
//! magnitudes' and the divisions') has a value count: the window's
//! concurrency requirement.
use crate::acc::{Sum, DEGREES_PER_TURN};
use crate::displacement;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time::{self, hour_of, HOURS_PER_DAY, MONTHS_PER_YEAR, SECONDS_PER_DAY};
use crate::weibull::linfit;
use serde::{Deserialize, Serialize};
use super::shear::{aggregate_profile, effective_heights, momm_weights, power_law, ShearAggregation, MEAN_DAYS_PER_MONTH};
use super::{check_sectors, sector_of, MAX_HISTOGRAM_BINS};

/// The veer rate is reported per 100 m of height.
const VEER_PER_METRES: f64 = 100.0;
/// Half a turn: relative directions are wrapped to [−180°, 180°).
const HALF_TURN: f64 = DEGREES_PER_TURN / 2.0;
/// Most divisions at once (the original's pairs, e.g. sector and month).
pub const MAX_DIVISIONS: usize = 2;
/// Most bins along a column division: each bin is a whole profile per month.
pub const MAX_SHEAR_BINS: usize = 200;

/// What a profile is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShearQuantity {
    /// Mean speed: power-law exponent and log-law roughness.
    Speed,
    /// Mean speed standard deviation: power-law exponent only.
    SpeedSd,
    /// Mean direction: the veer rate, °/100 m.
    Direction,
}

/// One way of subdividing the steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Division {
    /// 24 hours of day, 0 = midnight to 1 h.
    Hour,
    /// 12 calendar months, 0 = January.
    Month,
    /// Each calendar month of the record, in order.
    MonthOfRecord,
    /// Direction sectors of a column, sector 0 centred on north.
    Sector {
        /// The direction column.
        direction: usize,
        /// How many sectors.
        sectors: usize,
    },
    /// Bins of `width` of any column, from a multiple of `width`.
    Bins {
        /// The column binned.
        column: usize,
        /// Bin width, in the column's units.
        width: f64,
    },
}

/// What [`shear_view`] computes.
#[derive(Debug, Clone)]
pub struct ShearViewOptions {
    /// What the profile is made of.
    pub quantity: ShearQuantity,
    /// The profile's columns (speeds, speed SDs or directions), each with a height.
    pub columns: Vec<usize>,
    /// Direction only, one per column: the speed column that weighs its
    /// vector mean and whose mean is its hodograph arrow; `None` = unit
    /// vectors. Ignored for the other quantities.
    pub magnitudes: Vec<Option<usize>>,
    /// At most [`MAX_DIVISIONS`]; none = one profile of every step.
    pub divisions: Vec<Division>,
    /// How each group's profile is averaged and fitted.
    pub aggregation: ShearAggregation,
}

/// The groups along one division.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DivisionAxis {
    /// How many.
    pub groups: usize,
    /// Bins: the first bin's lower edge; 0 otherwise.
    pub start: f64,
    /// Bins: their width; 0 otherwise.
    pub width: f64,
    /// Month of the record: the year and month (1 = January) of each group.
    pub months: Vec<(i32, u32)>,
}

/// The profile of one group.
#[derive(Debug, Clone, Serialize)]
pub struct ShearCell {
    /// Steps in the group.
    pub samples: usize,
    /// Each column's mean by the aggregation (the simple mean for the
    /// time-series methods): m/s, or the vector mean direction in degrees.
    pub means: Vec<f64>,
    /// Direction only: the mean of each column's magnitude speed (the
    /// hodograph arrow), NaN for unit vectors; empty otherwise.
    pub magnitudes: Vec<f64>,
    /// Power-law exponent, or the veer rate in °/100 m; NaN when it cannot
    /// be fitted.
    pub parameter: f64,
    /// Speed only: log-law roughness length, m; NaN otherwise or unless the
    /// speed grows with height.
    pub roughness_m: f64,
    /// The displacement height, averaged like the means, m.
    pub displacement_m: f64,
}

/// Which heights a [`HeightFit`] goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightFitKind {
    /// Every selected height.
    All,
    /// A sensor at the lowest height and one at the highest.
    Extremes,
    /// A sensor at one height and one at the next.
    Neighbours,
}

/// A power law and log law through some of the profile's heights.
#[derive(Debug, Clone, Serialize)]
pub struct HeightFit {
    /// Which heights.
    pub kind: HeightFitKind,
    /// Positions in [`ShearViewOptions::columns`], lowest first.
    pub columns: Vec<usize>,
    /// Power-law exponent.
    pub alpha: f64,
    /// Log-law roughness length, m; NaN unless the speed grows with height.
    pub roughness_m: f64,
}

/// The displays of [`shear_view`].
#[derive(Debug, Clone, Serialize)]
pub struct ShearView {
    /// Each column's height, m, in the order given.
    pub heights: Vec<f64>,
    /// One per division, in the order given.
    pub axes: Vec<DivisionAxis>,
    /// One per group, the first division outermost (row-major).
    pub cells: Vec<ShearCell>,
    /// Undivided speed profile by an aggregate-profile method: the fits
    /// through every height, through the lowest and the highest, and through
    /// each pair of neighbouring heights. Empty otherwise.
    pub fits: Vec<HeightFit>,
}

/// The parameter step by step, binned.
#[derive(Debug, Clone, Serialize)]
pub struct ShearFrequency {
    /// Steps with a parameter.
    pub samples: usize,
    /// Mean parameter.
    pub mean: f64,
    /// Bin `i` is `[start + i·width, start + (i+1)·width)`.
    pub start: f64,
    /// Bin width: of α, or °/100 m.
    pub width: f64,
    /// Share of the steps per bin.
    pub frequency: Vec<f64>,
}

/// Log-law roughness length through one speed profile, m: `exp(−b/a)` of
/// `U = a·ln z + b`; NaN unless the speed grows with height.
pub fn roughness_length(heights: &[f64], speeds: &[f64]) -> f64 {
    let log_heights: Vec<f64> = heights.iter().map(|height| libm::log(*height)).collect();
    match linfit(&log_heights, speeds) {
        Some((slope, intercept)) if slope > 0.0 => libm::exp(-intercept / slope),
        _ => f64::NAN,
    }
}

/// Veer rate through one direction profile, °/100 m: the directions
/// relative to the first, wrapped to [−180°, 180°), on height. NaN without
/// two heights.
pub fn veer_rate(heights: &[f64], directions: &[f64]) -> f64 {
    let Some(&first) = directions.first() else { return f64::NAN };
    let relative: Vec<f64> = directions.iter().map(|direction| (direction - first + HALF_TURN).rem_euclid(DEGREES_PER_TURN) - HALF_TURN).collect();
    linfit(heights, &relative).map_or(f64::NAN, |(slope, _)| slope * VEER_PER_METRES)
}

/// The steps `options` uses and the heights of its columns.
struct Prepared {
    heights: Vec<f64>,
    steps: Vec<usize>,
}

fn prepare(dataset: &Dataset, view: &FilterView, options: &ShearViewOptions) -> CoreResult<Prepared> {
    if options.columns.iter().chain(options.magnitudes.iter().flatten()).any(|&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column: dataset.columns.len() });
    }
    let heights: Vec<f64> = options
        .columns
        .iter()
        .map(|&column| {
            let data = &dataset.columns[column];
            data.height_m.filter(|height| *height > 0.0).ok_or_else(|| CoreError::ColumnWithoutHeight { column: data.id.clone() })
        })
        .collect::<CoreResult<_>>()?;
    if heights.iter().all(|height| *height == heights[0]) {
        return Err(CoreError::ShearNeedsTwoHeights);
    }
    if options.divisions.len() > MAX_DIVISIONS {
        return Err(CoreError::TooManyDivisions { divisions: options.divisions.len(), max: MAX_DIVISIONS });
    }
    let mut needed = options.columns.clone();
    if options.quantity == ShearQuantity::Direction {
        needed.extend(options.magnitudes.iter().flatten());
    }
    for division in &options.divisions {
        match *division {
            Division::Sector { direction, sectors } => {
                check_sectors(sectors)?;
                needed.push(direction);
            }
            Division::Bins { column, width } => {
                if !(width > 0.0) {
                    return Err(CoreError::BinWidthNotPositive);
                }
                needed.push(column);
            }
            Division::Hour | Division::Month | Division::MonthOfRecord => {}
        }
    }
    if let Some(&column) = needed.iter().find(|&&column| column >= dataset.columns.len()) {
        return Err(CoreError::NoSuchColumn { column });
    }
    let values: Vec<&[f64]> = needed.iter().map(|&column| &dataset.columns[column].values[..]).collect();
    let steps = view
        .valid_runs(&needed, 0..dataset.len)
        .into_iter()
        .flatten()
        .filter(|&step| values.iter().all(|column| column[step].is_finite()))
        .collect();
    Ok(Prepared { heights, steps })
}

/// Each step's group along `division`, and the axis.
fn classify(dataset: &Dataset, steps: &[usize], record_months: &[usize], months: &[time::Month], division: Division) -> CoreResult<(Vec<usize>, DivisionAxis)> {
    let axis = |groups| DivisionAxis { groups, start: 0.0, width: 0.0, months: Vec::new() };
    Ok(match division {
        Division::Hour => (steps.iter().map(|&step| hour_of(dataset, step)).collect(), axis(HOURS_PER_DAY)),
        Division::Month => (record_months.iter().map(|&month| months[month].index_in_year()).collect(), axis(MONTHS_PER_YEAR)),
        Division::MonthOfRecord => (
            record_months.to_vec(),
            DivisionAxis { months: months.iter().map(|month| (month.year, month.month)).collect(), ..axis(months.len()) },
        ),
        Division::Sector { direction, sectors } => {
            let values = &dataset.columns[direction].values;
            (steps.iter().map(|&step| sector_of(values[step], sectors)).collect(), axis(sectors))
        }
        Division::Bins { column, width } => {
            let values = &dataset.columns[column].values;
            let (low, high) = steps.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &step| (low.min(values[step]), high.max(values[step])));
            if steps.is_empty() {
                return Ok((Vec::new(), DivisionAxis { width, ..axis(0) }));
            }
            let start = (low / width).floor() * width;
            let bins = ((high - start) / width).floor() + 1.0;
            if bins > MAX_SHEAR_BINS as f64 {
                return Err(CoreError::TooManyBins { bins, low, high, max: MAX_SHEAR_BINS });
            }
            let last = bins as usize - 1;
            let groups = steps.iter().map(|&step| (((values[step] - start) / width).floor() as usize).min(last)).collect();
            (groups, DivisionAxis { groups: last + 1, start, width, months: Vec::new() })
        }
    })
}

/// The index into `months` of each step (steps increasing).
fn record_months_of(steps: &[usize], months: &[time::Month]) -> Vec<usize> {
    let mut month = 0;
    steps
        .iter()
        .map(|&step| {
            while month + 1 < months.len() && months[month].step_end <= step {
                month += 1;
            }
            month
        })
        .collect()
}

/// One step's parameter (and roughness, speed only) through `values` at
/// effective heights `heights`; NaN when it cannot be fitted.
fn step_fit(quantity: ShearQuantity, heights: &[f64], values: &[f64]) -> (f64, f64) {
    match quantity {
        ShearQuantity::Speed => (power_law(heights, values), roughness_length(heights, values)),
        ShearQuantity::SpeedSd => (power_law(heights, values), f64::NAN),
        ShearQuantity::Direction => (veer_rate(heights, values), f64::NAN),
    }
}

/// Mean, or median, of the finite `values`; NaN without any.
fn time_series_value(values: &mut Vec<f64>, aggregation: ShearAggregation) -> f64 {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return f64::NAN;
    }
    if aggregation == ShearAggregation::TsMedian {
        values.sort_by(f64::total_cmp);
        let middle = values.len() / 2;
        return if values.len() % 2 == 1 { values[middle] } else { (values[middle - 1] + values[middle]) / 2.0 };
    }
    let mut sum = Sum::ZERO;
    values.iter().for_each(|value| sum.add(*value));
    sum.value() / values.len() as f64
}

/// The shear of `options`, per group of its divisions. One pass over the
/// concurrent steps by group and calendar month (the aggregate methods),
/// plus one fit per step for the time-series methods. O(steps × columns).
///
/// # Errors
/// [`CoreError::ColumnWithoutHeight`], [`CoreError::ShearNeedsTwoHeights`],
/// [`CoreError::TooManyDivisions`], [`CoreError::SectorsOutOfRange`],
/// [`CoreError::BinWidthNotPositive`], [`CoreError::TooManyBins`] (beyond
/// [`MAX_SHEAR_BINS`]), [`CoreError::NoSuchColumn`],
/// [`CoreError::NoConcurrentStep`] or [`CoreError::DisplacementHeight`]
/// (undivided, not below every sensor).
pub fn shear_view(dataset: &Dataset, view: &FilterView, options: &ShearViewOptions) -> CoreResult<ShearView> {
    let Prepared { heights, steps } = prepare(dataset, view, options)?;
    if steps.is_empty() {
        return Err(CoreError::NoConcurrentStep);
    }
    let days = time::days(dataset);
    let months = time::months(dataset, &days);
    let record_months = record_months_of(&steps, &months);
    let mut group_of = vec![0usize; steps.len()];
    let mut axes = Vec::with_capacity(options.divisions.len());
    for &division in &options.divisions {
        let (groups, axis) = classify(dataset, &steps, &record_months, &months, division)?;
        group_of.iter_mut().zip(&groups).for_each(|(group, part)| *group = *group * axis.groups + part);
        axes.push(axis);
    }
    let group_count: usize = axes.iter().map(|axis| axis.groups).product();

    // Tracked per step: the values (speed / SD), or per vane m·sin, m·cos
    // and the magnitude; then the displacement height.
    let count = options.columns.len();
    let direction = options.quantity == ShearQuantity::Direction;
    let tracked = if direction { 3 * count + 1 } else { count + 1 };
    let columns: Vec<&[f64]> = options.columns.iter().map(|&column| &dataset.columns[column].values[..]).collect();
    let magnitudes: Vec<Option<&[f64]>> =
        (0..count).map(|position| options.magnitudes.get(position).copied().flatten().map(|column| &dataset.columns[column].values[..])).collect();
    let displaced = displacement::by_step(dataset);
    let cell = |group: usize, month: usize| group * MONTHS_PER_YEAR + month;
    let mut sums = vec![Sum::ZERO; group_count * MONTHS_PER_YEAR * tracked];
    let mut counts = vec![0.0; group_count * MONTHS_PER_YEAR];
    let time_series = matches!(options.aggregation, ShearAggregation::TsMean | ShearAggregation::TsMedian);
    let mut step_parameters: Vec<Vec<f64>> = vec![Vec::new(); if time_series { group_count } else { 0 }];
    let mut step_roughness: Vec<Vec<f64>> = vec![Vec::new(); if time_series { group_count } else { 0 }];
    let (mut values, mut effective) = (vec![0.0; count], vec![0.0; count]);
    for (position, &step) in steps.iter().enumerate() {
        let group = group_of[position];
        let at = cell(group, months[record_months[position]].index_in_year());
        counts[at] += 1.0;
        let displacement = displaced.as_ref().map_or(0.0, |heights| heights[step]);
        let cell_sums = &mut sums[at * tracked..(at + 1) * tracked];
        for (column, data) in columns.iter().enumerate() {
            values[column] = data[step];
            if direction {
                let magnitude = magnitudes[column].map_or(1.0, |speeds| speeds[step]);
                let radians = data[step].to_radians();
                cell_sums[column].add(magnitude * libm::sin(radians));
                cell_sums[count + column].add(magnitude * libm::cos(radians));
                cell_sums[2 * count + column].add(magnitude);
            } else {
                cell_sums[column].add(data[step]);
            }
        }
        cell_sums[tracked - 1].add(displacement);
        if time_series {
            heights.iter().zip(effective.iter_mut()).for_each(|(height, effective)| *effective = height - displacement);
            if effective.iter().all(|height| *height > 0.0) {
                let (parameter, roughness) = step_fit(options.quantity, &effective, &values);
                step_parameters[group].push(parameter);
                step_roughness[group].push(roughness);
            }
        }
    }

    // MoMM weights: with completeness when undivided, each month's mean days inside a subdivision.
    let steps_per_day = SECONDS_PER_DAY as f64 / dataset.step_s as f64;
    let weights = if options.divisions.is_empty() {
        let mut month_counts = [0.0; MONTHS_PER_YEAR];
        (0..MONTHS_PER_YEAR).for_each(|month| month_counts[month] = counts[cell(0, month)]);
        momm_weights(&month_counts, steps_per_day)
    } else {
        MEAN_DAYS_PER_MONTH
    };
    let mut cells = Vec::with_capacity(group_count);
    for group in 0..group_count {
        let month_counts = &counts[cell(group, 0)..cell(group + 1, 0)];
        let samples = month_counts.iter().sum::<f64>() as usize;
        let profile = aggregate_profile(tracked, options.aggregation, &weights, |month, column| sums[cell(group, month) * tracked + column].value(), |month| month_counts[month]);
        let displacement_m = if samples > 0 { profile[tracked - 1] } else { f64::NAN };
        let (means, cell_magnitudes) = if direction {
            let means = (0..count).map(|column| libm::atan2(profile[column], profile[count + column]).to_degrees().rem_euclid(DEGREES_PER_TURN)).collect();
            let speeds = (0..count).map(|column| if magnitudes[column].is_some() { profile[2 * count + column] } else { f64::NAN }).collect();
            (means, speeds)
        } else {
            (profile[..count].to_vec(), Vec::new())
        };
        let (parameter, roughness_m) = if samples == 0 {
            (f64::NAN, f64::NAN)
        } else if time_series {
            (time_series_value(&mut step_parameters[group], options.aggregation), time_series_value(&mut step_roughness[group], options.aggregation))
        } else {
            match effective_heights(&heights, displacement_m) {
                Some(effective) => step_fit(options.quantity, &effective, &means),
                None if options.divisions.is_empty() => return Err(CoreError::DisplacementHeight { height_m: displacement_m }),
                None => (f64::NAN, f64::NAN),
            }
        };
        cells.push(ShearCell { samples, means, magnitudes: cell_magnitudes, parameter, roughness_m, displacement_m });
    }
    let fits = if options.divisions.is_empty() && options.quantity == ShearQuantity::Speed && !time_series {
        height_fits(&heights, &cells[0])
    } else {
        Vec::new()
    };
    Ok(ShearView { heights, axes, cells, fits })
}

/// The fits through every height, the lowest and highest, and each pair of
/// neighbouring heights, at effective heights. Sensors sharing a height
/// (a colocated pair) each pair with every sensor of the other height.
fn height_fits(heights: &[f64], cell: &ShearCell) -> Vec<HeightFit> {
    let mut order: Vec<usize> = (0..heights.len()).collect();
    order.sort_by(|a, b| heights[*a].total_cmp(&heights[*b]));
    let levels: Vec<&[usize]> = order.chunk_by(|a, b| heights[*a] == heights[*b]).collect();
    let fit = |kind: HeightFitKind, columns: Vec<usize>| {
        let effective: Vec<f64> = columns.iter().map(|&column| heights[column] - cell.displacement_m).collect();
        let speeds: Vec<f64> = columns.iter().map(|&column| cell.means[column]).collect();
        let usable = effective.iter().all(|height| *height > 0.0);
        HeightFit {
            kind,
            alpha: if usable { power_law(&effective, &speeds) } else { f64::NAN },
            roughness_m: if usable { roughness_length(&effective, &speeds) } else { f64::NAN },
            columns,
        }
    };
    let pairs = |low: &[usize], high: &[usize]| -> Vec<Vec<usize>> { low.iter().flat_map(|&a| high.iter().map(move |&b| vec![a, b])).collect() };
    let mut fits = vec![fit(HeightFitKind::All, order.clone())];
    fits.extend(pairs(levels[0], levels[levels.len() - 1]).into_iter().map(|pair| fit(HeightFitKind::Extremes, pair)));
    for neighbours in levels.windows(2) {
        fits.extend(pairs(neighbours[0], neighbours[1]).into_iter().map(|pair| fit(HeightFitKind::Neighbours, pair)));
    }
    fits
}

/// The parameter of every concurrent step (each with its displacement
/// height), in bins of `width` from a multiple of it. O(steps × columns).
///
/// # Errors
/// Those of [`shear_view`] (divisions are ignored), [`CoreError::BinWidthNotPositive`],
/// [`CoreError::NoDataAfterFilter`] when no step has a parameter, or
/// [`CoreError::TooManyBins`] (beyond [`MAX_HISTOGRAM_BINS`]).
pub fn shear_frequency(dataset: &Dataset, view: &FilterView, options: &ShearViewOptions, width: f64) -> CoreResult<ShearFrequency> {
    if !(width > 0.0) {
        return Err(CoreError::BinWidthNotPositive);
    }
    let undivided = ShearViewOptions { divisions: Vec::new(), ..options.clone() };
    let Prepared { heights, steps } = prepare(dataset, view, &undivided)?;
    let columns: Vec<&[f64]> = options.columns.iter().map(|&column| &dataset.columns[column].values[..]).collect();
    let displaced = displacement::by_step(dataset);
    let (mut values, mut effective) = (vec![0.0; columns.len()], vec![0.0; columns.len()]);
    let parameters: Vec<f64> = steps
        .iter()
        .filter_map(|&step| {
            let displacement = displaced.as_ref().map_or(0.0, |heights| heights[step]);
            heights.iter().zip(effective.iter_mut()).for_each(|(height, effective)| *effective = height - displacement);
            columns.iter().zip(values.iter_mut()).for_each(|(column, value)| *value = column[step]);
            let usable = effective.iter().all(|height| *height > 0.0);
            usable.then(|| step_fit(options.quantity, &effective, &values).0).filter(|parameter| parameter.is_finite())
        })
        .collect();
    if parameters.is_empty() {
        return Err(CoreError::NoDataAfterFilter);
    }
    let (low, high) = parameters.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(*value), high.max(*value)));
    let start = (low / width).floor() * width;
    let bins = ((high - start) / width).floor() + 1.0;
    if bins > MAX_HISTOGRAM_BINS as f64 {
        return Err(CoreError::TooManyBins { bins, low, high, max: MAX_HISTOGRAM_BINS });
    }
    let last = bins as usize - 1;
    let mut counts = vec![0usize; last + 1];
    let mut sum = Sum::ZERO;
    for parameter in &parameters {
        counts[(((parameter - start) / width).floor() as usize).min(last)] += 1;
        sum.add(*parameter);
    }
    let samples = parameters.len();
    Ok(ShearFrequency {
        samples,
        mean: sum.value() / samples as f64,
        start,
        width,
        frequency: counts.iter().map(|count| *count as f64 / samples as f64).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_text, ImportConfig};

    const ALPHA: f64 = 0.2;

    /// Two speeds following a power law of [`ALPHA`], their SDs, and two
    /// vanes 10° apart over 50 m, hourly over two months.
    fn profile_dataset() -> Dataset {
        let mut csv = String::from("t,WS 50m,WS 50m SD,WS 100m,WS 100m SD,WD 50m,WD 100m,Temp 2m\n");
        for hour in 0..(24 * 59) {
            let (day, of_day) = (hour / 24, hour % 24);
            let speed = 4.0 + (hour % 7) as f64;
            let high = speed * 2f64.powf(ALPHA);
            let month = if day < 31 { 1 } else { 2 };
            let date = if month == 1 { day + 1 } else { day - 30 };
            csv.push_str(&format!(
                "2020-{month:02}-{date:02} {of_day:02}:00,{speed},{},{high},{},355,5,{}\n",
                speed * 0.2,
                speed * 0.2 * 2f64.powf(-0.1),
                10 + of_day
            ));
        }
        import_text("profile.csv", &csv, &ImportConfig::default()).unwrap().0
    }

    fn options(dataset: &Dataset, quantity: ShearQuantity, ids: &[&str], divisions: Vec<Division>, aggregation: ShearAggregation) -> ShearViewOptions {
        let index = |id: &str| dataset.columns.iter().position(|column| column.id == id).unwrap();
        ShearViewOptions { quantity, columns: ids.iter().map(|id| index(id)).collect(), magnitudes: Vec::new(), divisions, aggregation }
    }

    #[test]
    fn a_power_law_profile_gives_its_exponent_in_every_group_and_method() {
        let dataset = profile_dataset();
        let view = FilterView::new(&dataset);
        for aggregation in [ShearAggregation::ApMean, ShearAggregation::ApMomm, ShearAggregation::TsMean, ShearAggregation::TsMedian] {
            let shear = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 100m"], vec![Division::Month, Division::Hour], aggregation)).unwrap();
            assert_eq!(shear.cells.len(), MONTHS_PER_YEAR * HOURS_PER_DAY);
            let filled: Vec<&ShearCell> = shear.cells.iter().filter(|cell| cell.samples > 0).collect();
            assert_eq!(filled.len(), 2 * HOURS_PER_DAY, "January and February");
            assert!(filled.iter().all(|cell| (cell.parameter - ALPHA).abs() < 1e-9), "{aggregation:?}");
        }
    }

    #[test]
    fn sd_shear_and_veer_follow_their_profiles() {
        let dataset = profile_dataset();
        let view = FilterView::new(&dataset);
        let sd = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::SpeedSd, &["WS 50m SD", "WS 100m SD"], Vec::new(), ShearAggregation::ApMean)).unwrap();
        assert!((sd.cells[0].parameter + 0.1).abs() < 1e-9, "a negative SD exponent: {}", sd.cells[0].parameter);
        assert!(sd.fits.is_empty(), "the pair fits are the speed's");
        let veer = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::Direction, &["WD 50m", "WD 100m"], Vec::new(), ShearAggregation::ApMomm)).unwrap();
        assert!((veer.cells[0].parameter - 20.0).abs() < 1e-9, "355° to 5° over 50 m is +20°/100 m, through north: {}", veer.cells[0].parameter);
        assert!((veer.cells[0].means[0] - 355.0).abs() < 1e-9);
        let per_step = shear_frequency(&dataset, &view, &options(&dataset, ShearQuantity::Direction, &["WD 50m", "WD 100m"], Vec::new(), ShearAggregation::TsMean), 1.0).unwrap();
        assert_eq!(per_step.frequency.iter().filter(|share| **share > 0.0).count(), 1);
        assert!((per_step.mean - 20.0).abs() < 1e-9);
    }

    #[test]
    fn bins_of_a_column_and_months_of_the_record() {
        let dataset = profile_dataset();
        let view = FilterView::new(&dataset);
        let temperature = dataset.columns.iter().position(|column| column.id == "Temp 2m").unwrap();
        let by_bin = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 100m"], vec![Division::Bins { column: temperature, width: 5.0 }], ShearAggregation::ApMean)).unwrap();
        assert_eq!((by_bin.axes[0].start, by_bin.axes[0].groups), (10.0, 5), "10 to 33 °C in bins of 5");
        assert_eq!(by_bin.cells[0].samples, 5 * 59);
        let by_month = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 100m"], vec![Division::MonthOfRecord], ShearAggregation::ApMean)).unwrap();
        assert_eq!(by_month.axes[0].months, [(2020, 1), (2020, 2)]);
        assert_eq!(by_month.cells.iter().map(|cell| cell.samples).collect::<Vec<_>>(), [31 * 24, 28 * 24]);
    }

    #[test]
    fn the_pair_fits_and_the_refusals() {
        let dataset = profile_dataset();
        let view = FilterView::new(&dataset);
        let shear = shear_view(&dataset, &view, &options(&dataset, ShearQuantity::Speed, &["WS 100m", "WS 50m"], Vec::new(), ShearAggregation::ApMomm)).unwrap();
        assert_eq!(shear.fits.len(), 3, "all, lowest and highest, one neighbouring pair");
        assert_eq!(shear.fits.iter().map(|fit| fit.kind).collect::<Vec<_>>(), [HeightFitKind::All, HeightFitKind::Extremes, HeightFitKind::Neighbours]);
        assert_eq!(shear.fits[0].columns, [1, 0], "lowest first");
        assert!(shear.fits.iter().all(|fit| (fit.alpha - ALPHA).abs() < 1e-9));
        let same_height = options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 50m SD"], Vec::new(), ShearAggregation::ApMean);
        assert!(matches!(shear_view(&dataset, &view, &same_height), Err(CoreError::ShearNeedsTwoHeights)));
        let three = options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 100m"], vec![Division::Hour; 3], ShearAggregation::ApMean);
        assert!(matches!(shear_view(&dataset, &view, &three), Err(CoreError::TooManyDivisions { .. })));
        let wide = options(&dataset, ShearQuantity::Speed, &["WS 50m", "WS 100m"], vec![Division::Bins { column: 0, width: 0.0 }], ShearAggregation::ApMean);
        assert!(matches!(shear_view(&dataset, &view, &wide), Err(CoreError::BinWidthNotPositive)));
    }

    #[test]
    fn the_overall_shear_by_a_time_series_method() {
        let dataset = profile_dataset();
        let shear = super::super::shear(&dataset, &FilterView::new(&dataset), &[0, 2], Some(4), 4, ShearAggregation::TsMedian).unwrap();
        assert!((shear.alpha - ALPHA).abs() < 1e-9);
        assert!(shear.alpha_interval.is_none());
        assert_eq!((shear.alpha_by_hour.len(), shear.alpha_by_sector.len()), (HOURS_PER_DAY, 4));
        assert!(shear.alpha_by_hour.iter().all(|alpha| (alpha - ALPHA).abs() < 1e-9));
    }

    #[test]
    fn colocated_sensors_pair_with_the_next_height() {
        let cell = ShearCell { samples: 1, means: vec![5.0, 6.0, 6.1], magnitudes: Vec::new(), parameter: 0.0, roughness_m: 0.0, displacement_m: 0.0 };
        let fits = height_fits(&[20.0, 30.0, 30.0], &cell);
        let columns: Vec<(HeightFitKind, Vec<usize>)> = fits.into_iter().map(|fit| (fit.kind, fit.columns)).collect();
        assert_eq!(
            columns,
            [
                (HeightFitKind::All, vec![0, 1, 2]),
                (HeightFitKind::Extremes, vec![0, 1]),
                (HeightFitKind::Extremes, vec![0, 2]),
                (HeightFitKind::Neighbours, vec![0, 1]),
                (HeightFitKind::Neighbours, vec![0, 2]),
            ]
        );
    }

    #[test]
    fn veer_wraps_relative_to_the_first_vane() {
        assert!((veer_rate(&[10.0, 60.0], &[350.0, 10.0]) - 40.0).abs() < 1e-12);
        assert!((veer_rate(&[10.0, 60.0], &[10.0, 350.0]) + 40.0).abs() < 1e-12);
        assert!(veer_rate(&[10.0], &[10.0]).is_nan());
    }
}

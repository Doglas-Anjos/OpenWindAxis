//! Wind shear: power and log law through an aggregate profile, with its interval.
use crate::acc::{Acc, Sum};
use crate::bootstrap::{self, Interval};
use crate::displacement;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time::{self, hour_of, HOURS_PER_DAY, MONTHS_PER_YEAR, SECONDS_PER_DAY};
use crate::weibull::linfit;
use serde::{Deserialize, Serialize};
use super::{check_sectors, sector_of, shear_view, Division, ShearQuantity, ShearView, ShearViewOptions};

/// Mean number of days in each calendar month, January first, as the mean of
/// monthly means weighs them (manual def-momm.htm; February averages leap
/// years, 365.24 days in all).
pub const MEAN_DAYS_PER_MONTH: [f64; MONTHS_PER_YEAR] = [31.0, 28.24, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
/// How a shear profile averages each height's speed over time (manual
/// def-shear-aggregation-methods.htm): one curve through the aggregate
/// profile, or one per step and then their mean or median.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShearAggregation {
    /// Simple mean of the concurrent steps: simplest, but a record with more
    /// of one season leans toward it.
    ApMean,
    /// Mean of monthly means (`MoMM = Σ λᵢ·Yᵢ·mᵢ / Σ λᵢ·Yᵢ` over the twelve
    /// calendar months, `λᵢ` the month's completeness and `Yᵢ` its mean
    /// days): corrects for seasonal bias. The reference product's default.
    #[default]
    ApMomm,
    /// Mean of the parameters fitted step by step: suits a changing
    /// displacement height, but the high values weigh in.
    TsMean,
    /// Median of the parameters fitted step by step: less sensitive to the
    /// high values.
    TsMedian,
}

/// Power-law and log-law shear of a set of speed columns.
#[derive(Debug, Clone, Serialize)]
pub struct Shear {
    /// Height of each column, m, in the order given.
    pub heights: Vec<f64>,
    /// Each column's speed over the steps where every column has data,
    /// averaged by `aggregation`: the profile the curves are fitted to.
    pub means: Vec<f64>,
    /// Steps where every column has data.
    #[serde(rename = "n")]
    pub samples: usize,
    /// Power-law exponent fitted to the mean profile.
    pub alpha: f64,
    /// Log-law roughness length, m (NaN unless speed grows with height).
    pub z0: f64,
    /// Exponent per hour of day.
    pub alpha_by_hour: Vec<f64>,
    /// Exponent per direction sector; empty without a direction column.
    pub alpha_by_sector: Vec<f64>,
    /// How each height's speed was averaged.
    pub aggregation: ShearAggregation,
    /// 95 % interval of `alpha` by day-block bootstrap ([`crate::bootstrap`]);
    /// `None` with fewer than two days of concurrent data, or by a
    /// time-series method.
    pub alpha_interval: Option<Interval>,
    /// The displacement height over the concurrent steps, averaged like the
    /// speeds, m: the fits use `heights` minus it. 0 without one.
    pub displacement_m: f64,
}

/// Aggregate-profile shear: each height's speed averaged over the concurrent
/// steps by `aggregation`, then one fit, overall and per hour of day /
/// direction sector. By a time-series method, the same figures from
/// [`super::shear_view`] (the step fits' mean or median), without interval. The fits use effective heights, z minus the displacement
/// height ([`crate::displacement`]), whose value per profile is averaged over
/// the same steps with the same weights as the speeds. One pass, by group and
/// calendar month; the overall profile is the in-order merge of the 24 hourly
/// ones.
///
/// # Errors
/// [`CoreError::ColumnWithoutHeight`], [`CoreError::ShearNeedsTwoHeights`],
/// [`CoreError::DisplacementHeight`] (not below every sensor),
/// [`CoreError::SectorsOutOfRange`] (with a direction column) or
/// [`CoreError::NoConcurrentStep`].
pub fn shear(
    dataset: &Dataset,
    view: &FilterView,
    columns: &[usize],
    direction: Option<usize>,
    sectors: usize,
    aggregation: ShearAggregation,
) -> CoreResult<Shear> {
    let heights: Vec<f64> = columns
        .iter()
        .map(|&column| {
            let data = &dataset.columns[column];
            data.height_m.filter(|height| *height > 0.0).ok_or_else(|| CoreError::ColumnWithoutHeight { column: data.id.clone() })
        })
        .collect::<CoreResult<_>>()?;
    if heights.iter().all(|height| *height == heights[0]) {
        return Err(CoreError::ShearNeedsTwoHeights);
    }
    if direction.is_some() {
        check_sectors(sectors)?;
    }
    if matches!(aggregation, ShearAggregation::TsMean | ShearAggregation::TsMedian) {
        return time_series_shear(dataset, view, columns, direction, sectors, aggregation, heights);
    }
    let column_count = columns.len();
    // Each cell also sums the displacement height, as one more column after the speeds.
    let tracked = column_count + 1;
    let displaced = displacement::by_step(dataset);
    let displacement_at = |step: usize| displaced.as_ref().map_or(0.0, |heights| heights[step]);
    let speeds: Vec<&[f64]> = columns.iter().map(|&column| &dataset.columns[column].values[..]).collect();
    let runs = view.valid_runs(columns, 0..dataset.len);
    let mut direction_data = direction.map(|column| (view.cursor(column), &dataset.columns[column].values[..]));
    // Groups 0..24 are hours of day, then one per sector; each splits by
    // calendar month, so either aggregation can be taken at the end.
    let first_sector_group = HOURS_PER_DAY;
    let groups = first_sector_group + if direction.is_some() { sectors } else { 0 };
    let cell = |group: usize, month: usize| group * MONTHS_PER_YEAR + month;
    let mut sums = vec![Sum::ZERO; groups * MONTHS_PER_YEAR * tracked];
    let mut counts = vec![0usize; groups * MONTHS_PER_YEAR];
    let days = time::days(dataset);
    let months = time::months(dataset, &days);
    let mut month_index = 0;
    // Per day with concurrent data, for the α interval: its month, step
    // count and per-column totals (`day_totals[day * tracked + column]`).
    let mut day_index = 0;
    let mut day_months: Vec<usize> = Vec::new();
    let mut day_counts: Vec<f64> = Vec::new();
    let mut day_totals: Vec<f64> = Vec::new();
    let mut open_day: Option<usize> = None;
    for run in &runs {
        for step in run.clone() {
            if speeds.iter().any(|column| column[step].is_nan()) {
                continue;
            }
            while month_index + 1 < months.len() && months[month_index].step_end <= step {
                month_index += 1;
            }
            let month = months[month_index].month as usize - 1;
            while day_index + 1 < days.len() && days[day_index].end <= step {
                day_index += 1;
            }
            if open_day != Some(day_index) {
                open_day = Some(day_index);
                day_months.push(month);
                day_counts.push(0.0);
                day_totals.extend(std::iter::repeat(0.0).take(tracked));
            }
            *day_counts.last_mut().expect("a day was just opened") += 1.0;
            let today = day_totals.len() - tracked;
            let displacement = displacement_at(step);
            day_totals[today..].iter_mut().zip(&speeds).for_each(|(total, column)| *total += column[step]);
            day_totals[today + column_count] += displacement;
            let mut add = |group: usize| {
                let at = cell(group, month);
                counts[at] += 1;
                let cell_sums = &mut sums[at * tracked..(at + 1) * tracked];
                for (sum, column) in cell_sums.iter_mut().zip(&speeds) {
                    sum.add(column[step]);
                }
                cell_sums[column_count].add(displacement);
            };
            add(hour_of(dataset, step));
            if let Some((excluded, directions)) = direction_data.as_mut() {
                if !excluded.excludes(step) && !directions[step].is_nan() {
                    add(first_sector_group + sector_of(directions[step], sectors));
                }
            }
        }
    }
    // The overall profile merges the 24 hours, month by month, in order.
    let mut overall_sums = vec![Sum::ZERO; MONTHS_PER_YEAR * tracked];
    let mut overall_counts = [0usize; MONTHS_PER_YEAR];
    for hour in 0..HOURS_PER_DAY {
        for month in 0..MONTHS_PER_YEAR {
            let at = cell(hour, month);
            overall_counts[month] += counts[at];
            let hourly = &sums[at * tracked..(at + 1) * tracked];
            overall_sums[month * tracked..(month + 1) * tracked].iter_mut().zip(hourly).for_each(|(total, part)| total.merge(part));
        }
    }
    let samples: usize = overall_counts.iter().sum();
    if samples == 0 {
        return Err(CoreError::NoConcurrentStep);
    }
    let steps_per_day = SECONDS_PER_DAY as f64 / dataset.step_s as f64;
    let overall_weights = momm_weights(&overall_counts.map(|count| count as f64), steps_per_day);
    // α of a profile whose last entry is its displacement height.
    let fit = |profile: &[f64]| effective_heights(&heights, profile[column_count]).map_or(f64::NAN, |effective| power_law(&effective, &profile[..column_count]));
    let alpha_of = |group: usize| {
        let month_counts = &counts[cell(group, 0)..cell(group + 1, 0)];
        if month_counts.iter().all(|&count| count == 0) {
            return f64::NAN;
        }
        let total = |month: usize, column: usize| sums[cell(group, month) * tracked + column].value();
        let count = |month: usize| month_counts[month] as f64;
        fit(&aggregate_profile(tracked, aggregation, &overall_weights, total, count))
    };
    let mut overall = aggregate_profile(
        tracked,
        aggregation,
        &overall_weights,
        |month, column| overall_sums[month * tracked + column].value(),
        |month| overall_counts[month] as f64,
    );
    let displacement_m = overall[column_count];
    let effective = effective_heights(&heights, displacement_m).ok_or(CoreError::DisplacementHeight { height_m: displacement_m })?;
    // α of each replicate: the drawn days' totals by month, then the same
    // profile and fit. O(days × columns) per replicate.
    let mut replicate_alphas = Vec::with_capacity(bootstrap::RESAMPLES);
    let (mut month_totals, mut month_counts) = (vec![0.0; MONTHS_PER_YEAR * tracked], [0.0; MONTHS_PER_YEAR]);
    if day_counts.len() >= 2 {
        bootstrap::each_replicate(day_counts.len(), |draws| {
            month_totals.iter_mut().for_each(|total| *total = 0.0);
            month_counts = [0.0; MONTHS_PER_YEAR];
            for (day, &times) in draws.iter().enumerate().filter(|(_, &times)| times > 0) {
                let (month, times) = (day_months[day], f64::from(times));
                month_counts[month] += times * day_counts[day];
                for column in 0..tracked {
                    month_totals[month * tracked + column] += times * day_totals[day * tracked + column];
                }
            }
            let weights = momm_weights(&month_counts, steps_per_day);
            let profile = aggregate_profile(
                tracked,
                aggregation,
                &weights,
                |month, column| month_totals[month * tracked + column],
                |month| month_counts[month],
            );
            replicate_alphas.push(fit(&profile));
        });
    }
    overall.truncate(column_count);
    let log_heights: Vec<f64> = effective.iter().map(|height| libm::log(*height)).collect();
    let roughness_length = match linfit(&log_heights, &overall) {
        Some((slope, intercept)) if slope > 0.0 => libm::exp(-intercept / slope),
        _ => f64::NAN,
    };
    Ok(Shear {
        alpha: power_law(&effective, &overall),
        z0: roughness_length,
        samples,
        alpha_by_hour: (0..HOURS_PER_DAY).map(alpha_of).collect(),
        alpha_by_sector: (first_sector_group..groups).map(alpha_of).collect(),
        heights,
        means: overall,
        aggregation,
        alpha_interval: bootstrap::percentile_interval(&replicate_alphas),
        displacement_m,
    })
}

/// [`shear`] by a time-series method: three [`super::shear_view`]s, overall,
/// by hour and by sector.
fn time_series_shear(
    dataset: &Dataset,
    view: &FilterView,
    columns: &[usize],
    direction: Option<usize>,
    sectors: usize,
    aggregation: ShearAggregation,
    heights: Vec<f64>,
) -> CoreResult<Shear> {
    let options = |divisions| ShearViewOptions { quantity: ShearQuantity::Speed, columns: columns.to_vec(), magnitudes: Vec::new(), divisions, aggregation };
    let parameters = |view: ShearView| view.cells.iter().map(|cell| cell.parameter).collect::<Vec<f64>>();
    let overall = shear_view(dataset, view, &options(Vec::new()))?;
    let by_hour = parameters(shear_view(dataset, view, &options(vec![Division::Hour]))?);
    let by_sector = match direction {
        Some(direction) => parameters(shear_view(dataset, view, &options(vec![Division::Sector { direction, sectors }]))?),
        None => Vec::new(),
    };
    let cell = &overall.cells[0];
    Ok(Shear {
        heights,
        means: cell.means.clone(),
        samples: cell.samples,
        alpha: cell.parameter,
        z0: cell.roughness_m,
        alpha_by_hour: by_hour,
        alpha_by_sector: by_sector,
        aggregation,
        alpha_interval: None,
        displacement_m: cell.displacement_m,
    })
}

/// `heights` less the displacement height, or `None` when one is not above it.
pub(super) fn effective_heights(heights: &[f64], displacement_m: f64) -> Option<Vec<f64>> {
    let effective: Vec<f64> = heights.iter().map(|height| height - displacement_m).collect();
    effective.iter().all(|height| *height > 0.0).then_some(effective)
}

/// λᵢ·Yᵢ per calendar month: a month weighs its mean days, scaled down while
/// incomplete (`λᵢ = min(1, Nᵢ / (n·Yᵢ))` for `Nᵢ` concurrent steps and `n`
/// steps per day; manual def-momm.htm). Computed from all concurrent steps of
/// the month, so every hour and sector gets the same seasonal correction.
pub(super) fn momm_weights(month_counts: &[f64; MONTHS_PER_YEAR], steps_per_day: f64) -> [f64; MONTHS_PER_YEAR] {
    std::array::from_fn(|month| {
        let days = MEAN_DAYS_PER_MONTH[month];
        (month_counts[month] / (steps_per_day * days)).min(1.0) * days
    })
}

/// Each column's speed from per-month totals and counts, averaged by
/// `aggregation`: the simple mean (also for the time-series methods, whose
/// profile is only shown), or the mean of monthly means weighted by
/// `weights` ([`momm_weights`]) over the months with data.
pub(super) fn aggregate_profile(
    column_count: usize,
    aggregation: ShearAggregation,
    weights: &[f64; MONTHS_PER_YEAR],
    total: impl Fn(usize, usize) -> f64,
    count: impl Fn(usize) -> f64,
) -> Vec<f64> {
    (0..column_count)
        .map(|column| match aggregation {
            ShearAggregation::ApMean | ShearAggregation::TsMean | ShearAggregation::TsMedian => {
                let (mut sum, mut steps) = (Sum::ZERO, 0.0);
                for month in 0..MONTHS_PER_YEAR {
                    sum.add(total(month, column));
                    steps += count(month);
                }
                sum.value() / steps
            }
            ShearAggregation::ApMomm => {
                let (mut weighted, mut weight) = (Sum::ZERO, Sum::ZERO);
                for month in (0..MONTHS_PER_YEAR).filter(|&month| count(month) > 0.0) {
                    weighted.add(weights[month] * total(month, column) / count(month));
                    weight.add(weights[month]);
                }
                weighted.value() / weight.value()
            }
        })
        .collect()
}

/// Slope of ln U on ln z; NaN if any mean speed is not positive.
pub(super) fn power_law(heights: &[f64], mean_speeds: &[f64]) -> f64 {
    if mean_speeds.iter().any(|speed| !(*speed > 0.0)) {
        return f64::NAN;
    }
    let log_heights: Vec<f64> = heights.iter().map(|height| libm::log(*height)).collect();
    let log_speeds: Vec<f64> = mean_speeds.iter().map(|speed| libm::log(*speed)).collect();
    linfit(&log_heights, &log_speeds).map_or(f64::NAN, |(slope, _)| slope)
}

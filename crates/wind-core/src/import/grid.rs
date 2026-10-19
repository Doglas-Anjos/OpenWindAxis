//! Stage 4: the regular time grid.
use crate::error::{CoreError, CoreResult};
use std::collections::HashMap;

// ---- Grid ----

/// Timestamps sampled to find the grid's phase.
pub(super) const PHASE_SAMPLE: usize = 1_000;
/// Sanity limit: the timestamps may span at most this many steps per row,
/// plus [`GRID_SLACK_STEPS`]; more means a wrong format, step or outlier.
pub(super) const MAX_STEPS_PER_ROW: u128 = 10;
pub(super) const GRID_SLACK_STEPS: u128 = 10_000;
/// Marks a row that lost its grid step to an earlier duplicate.
pub(super) const NOT_PLACED: usize = usize::MAX;

// ---- Stage 4: grid ----

/// The rows placed on a regular time grid.
pub(super) struct Grid {
    /// Start of the first step (before the timestamp-meaning shift), seconds.
    pub(super) start: i64,
    /// Time step, seconds.
    pub(super) step: i64,
    /// Number of steps.
    pub(super) len: usize,
    /// `[column][step]`, NaN where no row landed.
    pub(super) values: Vec<Vec<f64>>,
    pub(super) duplicates: usize,
    pub(super) off_grid: usize,
    pub(super) missing_steps: usize,
}

/// Places each row on the nearest step of a regular grid. Rows arrive in any
/// order; the first of two rows on one step wins.
pub(super) fn place_on_grid(times: Vec<i64>, values: Vec<Vec<f64>>, configured_step: Option<i64>) -> CoreResult<Grid> {
    let sorted = times.windows(2).all(|pair| pair[0] <= pair[1]);
    let mut order: Vec<usize> = (0..times.len()).collect();
    if !sorted {
        order.sort_by_key(|&row| times[row]);
    }
    let step = match configured_step {
        Some(step) if step > 0 => step,
        Some(step) => return Err(CoreError::StepNotPositive { step_s: step }),
        None => infer_step(&times, &order).ok_or(CoreError::IdenticalTimestamps)?,
    };
    let earliest = times[order[0]];
    if sorted && times.windows(2).all(|pair| pair[1] - pair[0] == step) {
        // Already a gapless regular grid: no copy.
        return Ok(Grid { start: earliest, step, len: times.len(), values, duplicates: 0, off_grid: 0, missing_steps: 0 });
    }
    // Anchor the grid on the most common phase, so one stray off-phase row
    // (a logger restart at 00:03 in a 10-min file) cannot shift the record.
    let phase = modal_phase(&times, step);
    let half_step = step / 2;
    let phase_point_before = earliest - (earliest - phase).rem_euclid(step);
    let start = phase_point_before + (earliest - phase_point_before + half_step).div_euclid(step) * step;
    let nearest_step = |time: i64| (time - start + half_step).div_euclid(step);
    let last_step = nearest_step(times[order[order.len() - 1]]);
    if last_step as u128 > MAX_STEPS_PER_ROW * times.len() as u128 + GRID_SLACK_STEPS {
        return Err(CoreError::TimestampsSpanTooMany { steps: last_step + 1, step_s: step, rows: times.len() });
    }
    let len = last_step as usize + 1;
    let mut step_of_row = vec![NOT_PLACED; times.len()];
    let mut filled = vec![false; len];
    let (mut duplicates, mut off_grid) = (0usize, 0usize);
    for &row in &order {
        if (times[row] - start).rem_euclid(step) != 0 {
            off_grid += 1;
        }
        let grid_step = nearest_step(times[row]) as usize;
        if filled[grid_step] {
            duplicates += 1;
        } else {
            filled[grid_step] = true;
            step_of_row[row] = grid_step;
        }
    }
    let missing_steps = filled.iter().filter(|filled| !**filled).count();
    // Column by column, so peak memory is the raw rows plus one grid column.
    let values = values
        .into_iter()
        .map(|raw| {
            let mut gridded = vec![f64::NAN; len];
            for (row, &grid_step) in step_of_row.iter().enumerate() {
                if grid_step != NOT_PLACED {
                    gridded[grid_step] = raw[row];
                }
            }
            gridded
        })
        .collect();
    Ok(Grid { start, step, len, values, duplicates, off_grid, missing_steps })
}

/// Most common `time mod step` over up to [`PHASE_SAMPLE`] timestamps spread
/// through the file (O(1) in the file size); ties go to the smaller phase.
pub(super) fn modal_phase(times: &[i64], step: i64) -> i64 {
    let stride = (times.len() / PHASE_SAMPLE).max(1);
    let mut stamps_by_phase: HashMap<i64, usize> = HashMap::new();
    for time in times.iter().step_by(stride) {
        *stamps_by_phase.entry(time.rem_euclid(step)).or_default() += 1;
    }
    stamps_by_phase
        .into_iter()
        .max_by_key(|&(phase, stamps)| (stamps, std::cmp::Reverse(phase)))
        .map_or(0, |(phase, _)| phase)
}

/// Most frequent positive interval; ties go to the shorter one.
pub(super) fn infer_step(times: &[i64], order: &[usize]) -> Option<i64> {
    let mut gaps_by_length: HashMap<i64, usize> = HashMap::new();
    for pair in order.windows(2) {
        let gap = times[pair[1]] - times[pair[0]];
        if gap > 0 {
            *gaps_by_length.entry(gap).or_default() += 1;
        }
    }
    gaps_by_length
        .into_iter()
        .max_by_key(|&(gap, count)| (count, std::cmp::Reverse(gap)))
        .map(|(gap, _)| gap)
}

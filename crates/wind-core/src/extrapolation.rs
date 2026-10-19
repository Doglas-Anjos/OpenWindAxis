//! Revisar › Extrapolação vertical (manual dlg-revise-vertical-extrapolation.htm;
//! ARQUITETURA 2026-10-18): speed, speed SD, direction and temperature
//! synthesized at another height from the profile each step measures, as
//! derived columns ([`crate::derived`]).
//!
//! Each step fits a line through the sensors of the quantity:
//! - speed and speed SD: the power law, ln v on ln z at effective heights
//!   (less the step's displacement height); a speed counts from
//!   [`SHEAR_MIN_SPEED_M_S`], an SD above 0. The speed's exponent can be
//!   held inside a range: beyond it, the limit (the original's "restrict",
//!   [`DEFAULT_ALPHA_RANGE`] by default);
//! - direction: the veer, each direction relative to the first sensor's in
//!   [−180°, 180°), on height (as [`crate::analysis::veer_rate`]);
//! - temperature: the gradient, temperature on height.
//!
//! Where a step has no fit (fewer than two sensors with a value at two
//! heights), or always with `table_only`, the slope is the median of the
//! step fits of its calendar month and hour over the record (the lookup
//! table; the record's median for a cell without any). The value at the
//! target height is then carried along the slope from a sensor (the one
//! nearest the height, or one chosen), or read on the step's line of best
//! fit (carried from the nearest sensor where the step has no fit).
//!
//! Values never depend on flags, the derived columns' rule: a flagged sensor
//! still counts in the fits, and the synthesized column inherits the flags of
//! every sensor it reads.
use crate::derived::{Derived, SHEAR_MIN_SPEED_M_S};
use crate::displacement;
use crate::model::{ColumnType, DataColumn, Dataset, ExtrapolateFrom, Extrapolation, Subtype};
use crate::time::{self, hour_of, HOURS_PER_DAY, MONTHS_PER_YEAR};

/// The original's default limits of a step's power-law exponent.
pub const DEFAULT_ALPHA_RANGE: (f64, f64) = (-0.25, 1.25);
/// Degrees in a turn and in half of one.
const DEGREES_PER_TURN: f64 = 360.0;
const HALF_TURN: f64 = DEGREES_PER_TURN / 2.0;

/// What a synthesized column carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// Mean speed, by the power law.
    Speed,
    /// Speed standard deviation, by the power law.
    SpeedSd,
    /// Mean direction, by the veer.
    Direction,
    /// Mean temperature, by the gradient.
    Temperature,
}

/// Where the synthesized profile passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Carry {
    /// Through this sensor's value.
    Sensor(usize),
    /// Along the step's line of best fit; through `fallback` where the step has none.
    BestFit {
        /// The sensor nearest the target height.
        fallback: usize,
    },
}

/// A synthesized column, resolved to column indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Synthesized {
    /// What it carries.
    pub quantity: Quantity,
    /// The sensors each step's fit goes through, at two or more heights.
    pub fitted: Vec<usize>,
    /// Where the profile passes.
    pub carry: Carry,
    /// The target height, m.
    pub height_m: f64,
    /// Only the month × hour table of slopes, never the step's own.
    pub table_only: bool,
    /// Speed only: the limits of a step's own exponent.
    pub range: Option<(f64, f64)>,
}

impl Synthesized {
    /// The columns it reads: the fitted sensors and the carried one.
    pub fn sources(&self) -> Vec<usize> {
        let carried = match self.carry {
            Carry::Sensor(column) | Carry::BestFit { fallback: column } => column,
        };
        let mut sources = self.fitted.clone();
        if !sources.contains(&carried) {
            sources.push(carried);
        }
        sources
    }
}

/// The columns one [`Extrapolation`] synthesizes.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The speed, always.
    pub speed: Synthesized,
    /// The speed SD, when two or more of the speeds have one at two heights.
    pub deviation: Option<Synthesized>,
    /// The direction, when asked for and its vanes stand at two heights.
    pub direction: Option<Synthesized>,
    /// The temperature, when asked for and its thermometers stand at two heights.
    pub temperature: Option<Synthesized>,
}

/// Index of the measured column `id` of `kind` and `subtype` with a positive height.
fn measured(dataset: &Dataset, id: &str, kind: ColumnType, subtype: Subtype) -> Option<usize> {
    dataset.columns.iter().position(|column| {
        column.id == id && column.kind == kind && column.subtype == subtype && column.derived.is_none() && column.height_m.is_some_and(|height| height > 0.0)
    })
}

fn height_of(dataset: &Dataset, column: usize) -> f64 {
    dataset.columns[column].height_m.unwrap_or(f64::NAN)
}

/// Whether `columns` stand at two or more heights.
fn two_heights(dataset: &Dataset, columns: &[usize]) -> bool {
    columns.iter().any(|&column| height_of(dataset, column) != height_of(dataset, columns[0]))
}

/// The sensor nearest `height_m`, the higher on a tie.
fn nearest(dataset: &Dataset, columns: &[usize], height_m: f64) -> usize {
    let mut best = columns[0];
    for &column in &columns[1..] {
        let (distance, best_distance) = ((height_of(dataset, column) - height_m).abs(), (height_of(dataset, best) - height_m).abs());
        if distance < best_distance || (distance == best_distance && height_of(dataset, column) > height_of(dataset, best)) {
            best = column;
        }
    }
    best
}

/// The measured columns of `ids` of `kind`, without repeats, when they stand at two heights.
fn sensors(dataset: &Dataset, ids: &[String], kind: ColumnType) -> Option<Vec<usize>> {
    let mut columns: Vec<usize> = ids.iter().filter_map(|id| measured(dataset, id, kind, Subtype::Mean)).collect();
    columns.dedup();
    (columns.len() >= 2 && two_heights(dataset, &columns)).then_some(columns)
}

/// The columns `spec` synthesizes, or `None` when its speeds are not two or
/// more measured mean speeds at two heights, or its height is not positive.
/// A direction or temperature whose sensors do not qualify is left out.
pub fn plan(dataset: &Dataset, spec: &Extrapolation) -> Option<Plan> {
    if !(spec.height_m > 0.0 && spec.height_m.is_finite()) {
        return None;
    }
    let ids = if spec.sensors.is_empty() { vec![spec.lower.clone(), spec.upper.clone()] } else { spec.sensors.clone() };
    let speeds = sensors(dataset, &ids, ColumnType::WindSpeed)?;
    let height_m = spec.height_m;
    let best_fit = spec.from == ExtrapolateFrom::BestFit;
    let carry = |columns: &[usize], chosen: Option<usize>| {
        let fallback = nearest(dataset, columns, height_m);
        match chosen {
            Some(column) => Carry::Sensor(column),
            None if best_fit => Carry::BestFit { fallback },
            None => Carry::Sensor(fallback),
        }
    };
    let chosen_speed = match &spec.from {
        ExtrapolateFrom::Sensor(id) => measured(dataset, id, ColumnType::WindSpeed, Subtype::Mean),
        ExtrapolateFrom::Nearest | ExtrapolateFrom::BestFit => None,
    };
    let synthesized = |quantity, fitted: Vec<usize>, carry, range| Synthesized { quantity, fitted, carry, height_m, table_only: spec.table_only, range };
    let deviation_of = |speed: usize| {
        dataset.columns.iter().position(|column| {
            column.parent == Some(speed) && column.kind == ColumnType::WindSpeed && column.subtype == Subtype::StdDev && column.derived.is_none()
        })
    };
    let deviations: Vec<usize> = speeds.iter().filter_map(|&speed| deviation_of(speed)).filter(|&sd| height_of(dataset, sd) > 0.0).collect();
    let deviation = (deviations.len() >= 2 && two_heights(dataset, &deviations)).then(|| {
        let carried = carry(&deviations, chosen_speed.and_then(deviation_of));
        synthesized(Quantity::SpeedSd, deviations.clone(), carried, None)
    });
    let other = |ids: &[String], kind, quantity| sensors(dataset, ids, kind).map(|columns| synthesized(quantity, columns.clone(), carry(&columns, None), None));
    Some(Plan {
        speed: synthesized(Quantity::Speed, speeds.clone(), carry(&speeds, chosen_speed), spec.alpha_range),
        deviation,
        direction: other(&spec.vanes, ColumnType::WindDirection, Quantity::Direction),
        temperature: other(&spec.thermometers, ColumnType::Temperature, Quantity::Temperature),
    })
}

/// The derived column of `synthesized`, named after its height
/// ("Vel 120 m extrapolada", "… DP", "Dir …", "Temp …"), with the units of
/// its sensors. `parent` is the synthesized speed, for its SD.
pub fn column(dataset: &Dataset, synthesized: Synthesized, parent: Option<usize>) -> DataColumn {
    let height_m = synthesized.height_m;
    let (id, kind, subtype) = match synthesized.quantity {
        Quantity::Speed => (format!("Vel {height_m} m extrapolada"), ColumnType::WindSpeed, Subtype::Mean),
        Quantity::SpeedSd => (format!("Vel {height_m} m extrapolada DP"), ColumnType::WindSpeed, Subtype::StdDev),
        Quantity::Direction => (format!("Dir {height_m} m extrapolada"), ColumnType::WindDirection, Subtype::Mean),
        Quantity::Temperature => (format!("Temp {height_m} m extrapolada"), ColumnType::Temperature, Subtype::Mean),
    };
    DataColumn {
        id,
        kind,
        subtype,
        units: dataset.columns[synthesized.fitted[0]].units.clone(),
        height_m: Some(height_m),
        parent,
        values: values(dataset, &synthesized).into(),
        derived: Some(Derived::Extrapolated(Box::new(synthesized))),
    }
}

/// A step's line of best fit: its slope through the centroid of its points;
/// for a direction, the points are relative to `origin`.
#[derive(Debug, Clone, Copy)]
struct Line {
    slope: f64,
    x: f64,
    y: f64,
    origin: f64,
}

/// The line through step `step`'s sensors, or `None` without two of them
/// with a value at two heights. `points` is scratch space.
fn fit(dataset: &Dataset, synthesized: &Synthesized, heights: &[f64], step: usize, displacement_m: f64, points: &mut Vec<(f64, f64)>) -> Option<Line> {
    points.clear();
    let mut origin = f64::NAN;
    for (&column, &height) in synthesized.fitted.iter().zip(heights) {
        let value = dataset.columns[column].values[step];
        match synthesized.quantity {
            Quantity::Speed | Quantity::SpeedSd => {
                let counts = if synthesized.quantity == Quantity::Speed { value >= SHEAR_MIN_SPEED_M_S } else { value > 0.0 };
                if counts && height - displacement_m > 0.0 {
                    points.push((libm::log(height - displacement_m), libm::log(value)));
                }
            }
            Quantity::Direction if value.is_finite() => {
                if origin.is_nan() {
                    origin = value;
                }
                points.push((height, (value - origin + HALF_TURN).rem_euclid(DEGREES_PER_TURN) - HALF_TURN));
            }
            Quantity::Temperature if value.is_finite() => points.push((height, value)),
            Quantity::Direction | Quantity::Temperature => {}
        }
    }
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let (x, y) = points.iter().fold((0.0, 0.0), |(x, y), point| (x + point.0, y + point.1));
    let (x, y) = (x / count, y / count);
    let (spread, covariance) = points.iter().fold((0.0, 0.0), |(spread, covariance), point| (spread + (point.0 - x).powi(2), covariance + (point.0 - x) * (point.1 - y)));
    (spread > 0.0).then(|| Line { slope: covariance / spread, x, y, origin: if origin.is_nan() { 0.0 } else { origin } })
}

/// The value at `height_m` on `line` with slope `slope`.
fn on_line(quantity: Quantity, line: Line, slope: f64, height_m: f64, displacement_m: f64) -> f64 {
    match quantity {
        Quantity::Speed | Quantity::SpeedSd if height_m - displacement_m > 0.0 => {
            libm::exp(line.y + slope * (libm::log(height_m - displacement_m) - line.x))
        }
        Quantity::Speed | Quantity::SpeedSd => f64::NAN,
        Quantity::Direction => (line.origin + line.y + slope * (height_m - line.x)).rem_euclid(DEGREES_PER_TURN),
        Quantity::Temperature => line.y + slope * (height_m - line.x),
    }
}

/// Sensor `column`'s value at step `step` carried to `height_m` along `slope`.
fn carried(dataset: &Dataset, quantity: Quantity, column: usize, slope: f64, height_m: f64, step: usize, displacement_m: f64) -> f64 {
    let (value, height) = (dataset.columns[column].values[step], height_of(dataset, column));
    match quantity {
        Quantity::Speed | Quantity::SpeedSd => {
            let (to, from) = (height_m - displacement_m, height - displacement_m);
            if to > 0.0 && from > 0.0 {
                value * (to / from).powf(slope)
            } else {
                f64::NAN
            }
        }
        Quantity::Direction => (value + slope * (height_m - height)).rem_euclid(DEGREES_PER_TURN),
        Quantity::Temperature => value + slope * (height_m - height),
    }
}

/// The median of `values`, NaN when empty.
fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_unstable_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// The values of `synthesized`: one pass for the month × hour table of
/// slopes, one for the values. O(steps × sensors).
pub(crate) fn values(dataset: &Dataset, synthesized: &Synthesized) -> Vec<f64> {
    let heights: Vec<f64> = synthesized.fitted.iter().map(|&column| height_of(dataset, column)).collect();
    let by_power_law = matches!(synthesized.quantity, Quantity::Speed | Quantity::SpeedSd);
    let displaced = if by_power_law { displacement::by_step(dataset) } else { None };
    let displacement_at = |step: usize| displaced.as_ref().map_or(0.0, |heights| heights[step]);
    let days = time::days(dataset);
    let mut month_of = vec![0usize; dataset.len];
    for month in time::months(dataset, &days) {
        month_of[month.steps()].fill(month.index_in_year());
    }
    let cell = |step: usize| month_of[step] * HOURS_PER_DAY + hour_of(dataset, step);
    let mut points = Vec::with_capacity(heights.len());
    let mut cells: Vec<Vec<f64>> = vec![Vec::new(); MONTHS_PER_YEAR * HOURS_PER_DAY];
    for step in 0..dataset.len {
        if let Some(line) = fit(dataset, synthesized, &heights, step, displacement_at(step), &mut points) {
            cells[cell(step)].push(line.slope);
        }
    }
    let overall = median(cells.iter().flatten().copied().collect());
    let table: Vec<f64> = cells.into_iter().map(|slopes| if slopes.is_empty() { overall } else { median(slopes) }).collect();
    let limited = |slope: f64| synthesized.range.map_or(slope, |(low, high)| slope.clamp(low, high));
    (0..dataset.len)
        .map(|step| {
            let displacement_m = displacement_at(step);
            let own = if synthesized.table_only { None } else { fit(dataset, synthesized, &heights, step, displacement_m, &mut points) };
            let slope = own.map_or_else(|| table[cell(step)], |line| limited(line.slope));
            match (synthesized.carry, own) {
                (Carry::BestFit { .. }, Some(line)) => on_line(synthesized.quantity, line, slope, synthesized.height_m, displacement_m),
                (Carry::BestFit { fallback: column }, None) | (Carry::Sensor(column), _) => {
                    carried(dataset, synthesized.quantity, column, slope, synthesized.height_m, step, displacement_m)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_text, ImportConfig};

    const ALPHA: f64 = 0.2;

    /// Speeds at 20, 40 and 80 m on a power law of [`ALPHA`] (the 80 m one
    /// 5 % fast), their SDs, two vanes veering 10° over 60 m and two
    /// thermometers cooling 0.5 °C over 60 m; hourly, a week.
    fn tower() -> Dataset {
        let mut csv = String::from("t,ws20,ws20 SD,ws40,ws40 SD,ws80,ws80 SD,wd20,wd80,t20,t80\n");
        for hour in 0..(24 * 7) {
            let speed = 5.0 + (hour % 5) as f64;
            let at = |height: f64| speed * (height / 20.0f64).powf(ALPHA);
            csv.push_str(&format!(
                "2020-01-{:02} {:02}:00,{},{},{},{},{},{},355,5,20,19.5\n",
                1 + hour / 24,
                hour % 24,
                at(20.0),
                0.1 * at(20.0),
                at(40.0),
                0.1 * at(40.0),
                1.05 * at(80.0),
                0.1 * at(80.0),
            ));
        }
        let mut dataset = import_text("torre.csv", &csv, &ImportConfig::default()).unwrap().0;
        let kinds = [("wd20", ColumnType::WindDirection, 20.0), ("wd80", ColumnType::WindDirection, 80.0), ("t20", ColumnType::Temperature, 20.0), ("t80", ColumnType::Temperature, 80.0)];
        for (id, kind, height) in kinds {
            let column = dataset.columns.iter_mut().find(|column| column.id == id).unwrap();
            (column.kind, column.subtype, column.height_m) = (kind, Subtype::Mean, Some(height));
        }
        dataset
    }

    fn spec(sensors: &[&str], height_m: f64, from: ExtrapolateFrom) -> Extrapolation {
        Extrapolation { sensors: sensors.iter().map(|id| id.to_string()).collect(), height_m, from, ..Extrapolation::default() }
    }

    fn index(dataset: &Dataset, id: &str) -> usize {
        dataset.columns.iter().position(|column| column.id == id).unwrap()
    }

    #[test]
    fn from_a_sensor_along_the_step_exponent() {
        let dataset = tower();
        let plan = plan(&dataset, &spec(&["ws20", "ws40"], 120.0, ExtrapolateFrom::Nearest)).unwrap();
        assert_eq!(plan.speed.carry, Carry::Sensor(index(&dataset, "ws40")), "the nearer of the fitted sensors");
        let speeds = values(&dataset, &plan.speed);
        let expected = dataset.columns[index(&dataset, "ws40")].values[3] * 3f64.powf(ALPHA);
        assert!((speeds[3] - expected).abs() < 1e-9, "{} vs {expected}", speeds[3]);
        let deviation = plan.deviation.expect("both speeds have an SD");
        assert!((values(&dataset, &deviation)[3] - 0.1 * expected).abs() < 1e-9);
    }

    #[test]
    fn the_best_fit_line_and_a_chosen_sensor() {
        let dataset = tower();
        let all = ["ws20", "ws40", "ws80"];
        let best = plan(&dataset, &spec(&all, 120.0, ExtrapolateFrom::BestFit)).unwrap();
        let top = plan(&dataset, &spec(&all, 120.0, ExtrapolateFrom::Sensor("ws80".into()))).unwrap();
        let (on_line, from_top) = (values(&dataset, &best.speed)[0], values(&dataset, &top.speed)[0]);
        assert!(from_top > on_line, "the fast top sensor lifts the curve it carries: {from_top} vs {on_line}");
        let restricted = Extrapolation { alpha_range: Some((0.0, 0.1)), ..spec(&all, 120.0, ExtrapolateFrom::Nearest) };
        let limited = values(&dataset, &plan(&dataset, &restricted).unwrap().speed)[0];
        let ws80 = dataset.columns[index(&dataset, "ws80")].values[0];
        assert!((limited - ws80 * 1.5f64.powf(0.1)).abs() < 1e-9, "the exponent held at its upper limit");
    }

    #[test]
    fn direction_and_temperature_by_veer_and_gradient() {
        let dataset = tower();
        let with_more = Extrapolation { vanes: vec!["wd20".into(), "wd80".into()], thermometers: vec!["t20".into(), "t80".into()], ..spec(&["ws20", "ws40"], 140.0, ExtrapolateFrom::Nearest) };
        let plan = plan(&dataset, &with_more).unwrap();
        let direction = values(&dataset, &plan.direction.unwrap());
        assert!((direction[0] - 15.0).abs() < 1e-9, "10° per 60 m, 60 m above the 80 m vane: {}", direction[0]);
        let temperature = values(&dataset, &plan.temperature.unwrap());
        assert!((temperature[0] - 19.0).abs() < 1e-9, "{}", temperature[0]);
    }

    #[test]
    fn the_synthesized_columns_take_a_flag_of_their_own() {
        let mut dataset = tower();
        dataset.extrapolations = vec![spec(&["ws20", "ws40", "ws80"], 100.0, ExtrapolateFrom::BestFit)];
        crate::derived::append(&mut dataset);
        let speed = index(&dataset, "Vel 100 m extrapolada");
        assert_eq!(dataset.columns[index(&dataset, "Vel 100 m extrapolada DP")].parent, Some(speed));
        let steps = dataset.len;
        assert!(crate::flags::flag(&mut dataset, &[speed], 0, 0..steps).is_ok(), "the original flags its synthesized columns");
        let turbulence = index(&dataset, "Vel 100 m extrapolada TI");
        assert!(crate::flags::flag(&mut dataset, &[turbulence], 0, 0..1).is_err(), "a TI only inherits");
        let view = crate::filter::FilterView::new(&dataset);
        assert_eq!(view.excluded(speed), [0..steps]);
    }

    #[test]
    fn the_table_fills_steps_without_a_fit_and_bad_specs_are_refused() {
        let mut dataset = tower();
        let ws20 = index(&dataset, "ws20");
        let mut values_20 = dataset.columns[ws20].values.to_vec();
        values_20[5] = 1.0; // below the shear threshold: no fit of its own
        dataset.columns[ws20].values = values_20.into();
        let plan = plan(&dataset, &spec(&["ws20", "ws40"], 60.0, ExtrapolateFrom::Nearest)).unwrap();
        let speeds = values(&dataset, &plan.speed);
        let expected = dataset.columns[index(&dataset, "ws40")].values[5] * 1.5f64.powf(ALPHA);
        assert!((speeds[5] - expected).abs() < 1e-9, "the median of the hour's exponents is the law's");
        assert!(super::plan(&dataset, &spec(&["ws20", "ws20 SD"], 60.0, ExtrapolateFrom::Nearest)).is_none(), "an SD is not a speed");
        assert!(super::plan(&dataset, &spec(&["ws20", "ws40"], 0.0, ExtrapolateFrom::Nearest)).is_none());
        assert!(super::plan(&dataset, &spec(&["ws20"], 60.0, ExtrapolateFrom::Nearest)).is_none());
    }
}

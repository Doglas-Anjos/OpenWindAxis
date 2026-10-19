//! Interaction-path timings on 30 columns × 1 M ten-minute steps (~19 years),
//! the size the targets in docs/ARQUITETURA.md §4.4 are stated for.
//! Run: `cargo bench -p wind-core --bench interaction` (native, one thread).
use std::hint::black_box;
use std::time::Instant;
use wind_core::analysis;
use wind_core::derived;
use wind_core::filter::FilterView;
use wind_core::flags;
use wind_core::model::{ColumnType, DataColumn, Dataset, Subtype};
use wind_core::series::DayStats;
use wind_core::weibull::Method;

/// Steps in the benchmark record.
/// The rose without a calm threshold, as before it existed.
const NO_CALMS: f64 = 0.0;
const STEPS: usize = 1_000_000;
/// Ten-minute steps.
const TEN_MINUTES: i64 = 600;
/// 2014-01-01T00:00Z.
const START_2014: i64 = 1_388_534_400;
/// Share of values left missing.
const GAP_SHARE: f64 = 0.01;
/// Mast layout: speeds (each with an SD child) from 40 m every 10 m, then
/// directions and temperatures.
const SPEED_HEIGHTS: usize = 10;
const DIRECTION_SENSORS: usize = 5;
const TEMPERATURE_SENSORS: usize = 5;
/// Flagged segments laid down before timing, as after a QC pass.
const EXISTING_SEGMENTS: usize = 200;
const SEGMENT_SPACING: usize = 4_000;
const SEGMENT_LENGTH: usize = 500;
/// One day of ten-minute steps: the stretch flagged in the timed edit.
const DAY_OF_STEPS: usize = 144;
/// Column indices used by the timed analyses.
const TOP_SPEED: usize = 18;
const FIRST_DIRECTION: usize = 2 * SPEED_HEIGHTS;
const ALL_SPEED_MEANS: [usize; SPEED_HEIGHTS] = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18];
const ROSE_SECTORS: usize = 16;
const ROSE_SPEED_EDGES: [f64; 5] = [3.0, 6.0, 9.0, 12.0, 15.0];
const HISTOGRAM_BIN_WIDTH: f64 = 0.5;
/// Site elevation, m, so the temperatures yield an air density column.
const SITE_ELEVATION_M: f64 = 300.0;
/// Repetitions per measurement; the best one is reported.
const REPETITIONS: usize = 5;
const SLOW_REPETITIONS: usize = 3;

/// xorshift64*: deterministic, dependency-free noise in [0, 1).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// How one synthetic sensor is laid out.
struct Sensor {
    id: String,
    kind: ColumnType,
    subtype: Subtype,
    height_m: f64,
    parent: Option<usize>,
    sample: fn(&mut Rng) -> f64,
}

fn synthetic_column(rng: &mut Rng, sensor: Sensor) -> DataColumn {
    let values = (0..STEPS).map(|_| if rng.next() < GAP_SHARE { f64::NAN } else { (sensor.sample)(rng) }).collect();
    DataColumn {
        id: sensor.id,
        kind: sensor.kind,
        subtype: sensor.subtype,
        units: String::new(),
        height_m: Some(sensor.height_m),
        parent: sensor.parent,
        values: values.into(),
        derived: None,
    }
}

/// Rayleigh speeds (Weibull k = 2, A = 8 m/s).
fn rayleigh_speed(rng: &mut Rng) -> f64 {
    8.0 * (-(1.0 - rng.next()).ln()).sqrt()
}

/// Directions to 0.1°, as loggers write them.
fn logged_direction(rng: &mut Rng) -> f64 {
    (3600.0 * rng.next()).floor() / 10.0
}

fn turbulence_sd(rng: &mut Rng) -> f64 {
    rng.next()
}

fn air_temperature(rng: &mut Rng) -> f64 {
    25.0 + 5.0 * rng.next()
}

fn dataset() -> Dataset {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut columns = Vec::new();
    for level in 0..SPEED_HEIGHTS {
        let height_m = 40.0 + 10.0 * level as f64;
        let mean = Sensor { id: format!("ws{height_m}"), kind: ColumnType::WindSpeed, subtype: Subtype::Mean, height_m, parent: None, sample: rayleigh_speed };
        columns.push(synthetic_column(&mut rng, mean));
        let parent = Some(columns.len() - 1);
        let deviation = Sensor { id: format!("sd{height_m}"), kind: ColumnType::WindSpeed, subtype: Subtype::StdDev, height_m, parent, sample: turbulence_sd };
        columns.push(synthetic_column(&mut rng, deviation));
    }
    for index in 0..DIRECTION_SENSORS {
        let vane = Sensor { id: format!("dir{index}"), kind: ColumnType::WindDirection, subtype: Subtype::Mean, height_m: 100.0, parent: None, sample: logged_direction };
        columns.push(synthetic_column(&mut rng, vane));
    }
    for index in 0..TEMPERATURE_SENSORS {
        let thermometer = Sensor { id: format!("t{index}"), kind: ColumnType::Temperature, subtype: Subtype::Mean, height_m: 2.0, parent: None, sample: air_temperature };
        columns.push(synthetic_column(&mut rng, thermometer));
    }
    Dataset {
        name: "bench".into(),
        start: START_2014,
        step_s: TEN_MINUTES,
        len: STEPS,
        timestamp_meaning: Default::default(),
        utc_offset_min: None,
        latitude: None,
        longitude: None,
        elevation_m: None,
        columns,
        flags: flags::default_flags(),
        segments: Vec::new(),
        evidence: Default::default(),
        edits: Vec::new(),
        extrapolations: Vec::new(),
        calculated: Vec::new(),
        ti_cutoff_m_s: None,
        primary: Vec::new(),
        displacement: None,
    }
}

/// Best of `repetitions` runs, in ms.
fn time<T>(label: &str, repetitions: usize, mut run: impl FnMut(usize) -> T) {
    let mut best = f64::INFINITY;
    for repetition in 0..repetitions {
        let started = Instant::now();
        black_box(run(repetition));
        best = best.min(started.elapsed().as_secs_f64() * 1e3);
    }
    println!("{label:<44} {best:>9.2} ms");
}

fn main() {
    let mut dataset = dataset();
    let measured = dataset.columns.len();
    dataset.elevation_m = Some(SITE_ELEVATION_M);
    for thermometer in dataset.columns.iter_mut().filter(|column| column.kind == ColumnType::Temperature) {
        thermometer.units = derived::CELSIUS.into();
    }
    // Density + 10 TI + 10 WPD; dropped again so the timings below stay on 30 columns.
    time("derived columns at import (21 × 1 M steps)", SLOW_REPETITIONS, |_| {
        derived::append(&mut dataset);
        let added = dataset.columns.len() - measured;
        dataset.columns.truncate(measured);
        added
    });
    let flag_count = dataset.flags.len();
    for segment in 0..EXISTING_SEGMENTS {
        let start = segment * SEGMENT_SPACING;
        flags::flag(&mut dataset, &[segment % (2 * SPEED_HEIGHTS)], segment % flag_count, start..start + SEGMENT_LENGTH).unwrap();
    }
    let mut view = FilterView::new(&dataset);
    let mut stats = DayStats::new(&dataset, &view);
    time("day stats, full build (import time)", SLOW_REPETITIONS, |_| DayStats::new(&dataset, &view));
    time("flag 1 column + summaries of 30", REPETITIONS, |repetition| {
        let start = 900_000 + repetition * 1_000;
        let edited = start..start + DAY_OF_STEPS;
        flags::flag(&mut dataset, &[0], 0, edited.clone()).unwrap();
        view = FilterView::new(&dataset);
        stats.refresh(&dataset, &view, &[0], edited);
        stats.summaries()
    });
    time("coverage, 30 columns", REPETITIONS, |_| analysis::coverage(&stats));
    time("wind rose, 16 sectors", REPETITIONS, |_| {
        analysis::wind_rose(&dataset, &view, FIRST_DIRECTION, Some(TOP_SPEED), ROSE_SECTORS, &ROSE_SPEED_EDGES, NO_CALMS).unwrap()
    });
    time("diurnal profile", REPETITIONS, |_| analysis::diurnal(&dataset, &view, stats.months(), TOP_SPEED));
    time("histogram, no fit", REPETITIONS, |_| analysis::histogram(&dataset, &view, TOP_SPEED, HISTOGRAM_BIN_WIDTH, None).unwrap());
    time("histogram + Weibull MLE", SLOW_REPETITIONS, |_| {
        analysis::histogram(&dataset, &view, TOP_SPEED, HISTOGRAM_BIN_WIDTH, Some(Method::MaxLikelihood)).unwrap()
    });
    time("shear, 10 heights + 16 sectors", REPETITIONS, |_| {
        analysis::shear(&dataset, &view, &ALL_SPEED_MEANS, Some(FIRST_DIRECTION), ROSE_SECTORS, Default::default()).unwrap()
    });
}

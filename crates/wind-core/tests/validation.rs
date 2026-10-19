//! Validation harness: known inputs with hand-checked expected outputs, plus
//! property tests on random data. CSV texts and expected numbers are test
//! fixtures; everything else is named.
use std::ops::Range;
use wind_core::analysis;
use wind_core::derived::{self, Derived};
use wind_core::primary;
use wind_core::error::CoreError;
use wind_core::export;
use wind_core::filter::{valid_values, FilterView};
use wind_core::flags;
use wind_core::history::{History, MAX_REVISIONS};
use wind_core::import::{import_text, ColumnOverride, ImportConfig};
use wind_core::model::{ColumnType, DataColumn, Dataset, Segment, Subtype, TimeStampMeaning};
use wind_core::ops::Op;
use wind_core::series::{self, ColumnSummary, DayStats};
use wind_core::time::{self, SECONDS_PER_DAY, SECONDS_PER_HOUR};
use wind_core::weibull::{self, Method};

/// 2023-01-01T00:00Z.
const START_2023: i64 = 1_672_531_200;
/// 2014-01-01T00:00Z.
const START_2014: i64 = 1_388_534_400;
/// Ten-minute steps, the usual logger interval.
const TEN_MINUTES: i64 = 600;
/// Default exclusion flag indices from `flags::DEFAULT_FLAGS`.
const INVALID_FLAG: usize = 0;
/// A rose without a calm threshold.
const NO_CALMS: f64 = 0.0;
const TOWER_SHADOW_FLAG: usize = 1;

fn column(id: &str, kind: ColumnType, height_m: Option<f64>, values: Vec<f64>) -> DataColumn {
    DataColumn { id: id.into(), kind, subtype: Subtype::Mean, units: String::new(), height_m, parent: None, values: values.into(), derived: None }
}

fn make_dataset(start: i64, step_s: i64, columns: Vec<DataColumn>) -> Dataset {
    Dataset {
        name: "test".into(),
        start,
        step_s,
        len: columns[0].values.len(),
        timestamp_meaning: TimeStampMeaning::Start,
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

/// Exact Weibull quantiles: a sample whose parameters are known.
fn weibull_sample(shape: f64, scale: f64, count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| scale * (-(1.0 - (index as f64 + 0.5) / count as f64).ln()).powf(1.0 / shape))
        .collect()
}

fn close(value: f64, want: f64, relative_tolerance: f64) -> bool {
    (value - want).abs() <= relative_tolerance * want.abs()
}

fn defaults() -> ImportConfig {
    ImportConfig::default()
}

fn view(dataset: &Dataset) -> FilterView {
    FilterView::new(dataset)
}

fn months(dataset: &Dataset) -> Vec<time::Month> {
    time::months(dataset, &time::days(dataset))
}

fn summaries(dataset: &Dataset) -> Vec<ColumnSummary> {
    DayStats::new(dataset, &view(dataset)).summaries()
}

#[test]
fn comma_csv_infers_columns_heights_and_associations() {
    let csv = "Timestamp,WS80_Avg,WS80_SD,Dir78,Temp 2m\n\
               2014-01-01 00:00:00,5.0,0.5,350,20.1\n\
               2014-01-01 00:10:00,6.0,0.6,10,20.2\n\
               2014-01-01 00:20:00,7.0,0.7,20,20.3\n";
    let (dataset, report) = import_text("t", csv, &defaults()).unwrap();
    assert_eq!(report.delimiter, ',');
    assert_eq!((dataset.start, dataset.step_s, dataset.len), (START_2014, TEN_MINUTES, 3));
    let columns = &dataset.columns;
    assert_eq!(columns.len(), 4);
    assert_eq!((columns[0].kind, columns[0].subtype, columns[0].height_m), (ColumnType::WindSpeed, Subtype::Mean, Some(80.0)));
    assert_eq!((columns[1].kind, columns[1].subtype, columns[1].parent), (ColumnType::WindSpeed, Subtype::StdDev, Some(0)));
    assert_eq!((columns[2].kind, columns[2].height_m), (ColumnType::WindDirection, Some(78.0)));
    assert_eq!((columns[3].kind, columns[3].height_m), (ColumnType::Temperature, Some(2.0)));
    assert_eq!(columns[0].values[..], [5.0, 6.0, 7.0]);
}

#[test]
fn semicolon_decimal_comma_day_first_with_logger_preamble() {
    let csv = "Logger: XYZ\nSite: Teste\n\
               Data/Hora;Velocidade 100m;Direção 98m;Rajada 100m\n\
               13/01/2014 00:00;5,5;180;9,1\n\
               13/01/2014 00:10;6,5;190;10,2\n\
               13/01/2014 00:20;-999;200;11,0\n";
    let (dataset, report) = import_text("br", csv, &defaults()).unwrap();
    assert_eq!(report.delimiter, ';');
    assert!(report.decimal_comma);
    assert_eq!(report.header_row, Some(2));
    assert_eq!(report.time_format, "%d/%m/%Y %H:%M");
    assert_eq!(dataset.columns[0].kind, ColumnType::WindSpeed);
    assert_eq!(dataset.columns[1].kind, ColumnType::WindDirection);
    assert_eq!((dataset.columns[2].subtype, dataset.columns[2].parent), (Subtype::Gust, Some(0)));
    assert_eq!(dataset.columns[0].values[0], 5.5);
    assert!(dataset.columns[0].values[2].is_nan(), "-999 is a missing-value sentinel");
}

#[test]
fn gaps_duplicates_and_end_of_interval_timestamps() {
    let csv = "time,ws\n\
               2020-05-01 00:10,1\n\
               2020-05-01 00:20,2\n\
               2020-05-01 00:20,99\n\
               2020-05-01 00:50,NaN\n\
               2020-05-01 01:00,\n\
               2020-05-01 01:10,7\n";
    let mut config = defaults();
    config.timestamp_meaning = TimeStampMeaning::End;
    let (dataset, report) = import_text("g", csv, &config).unwrap();
    let may_first_2020 = 1_588_291_200;
    assert_eq!((dataset.start, dataset.step_s, dataset.len), (may_first_2020, TEN_MINUTES, 7), "first step starts 00:00");
    assert_eq!((report.duplicates, report.missing_steps, report.off_grid), (1, 2, 0));
    let values = &dataset.columns[0].values;
    assert_eq!((values[0], values[1], values[6]), (1.0, 2.0, 7.0), "the first of two duplicate rows is kept");
    assert!(values[2..6].iter().all(|value| value.is_nan()));
    assert_eq!(report.unparsed, vec![0], "NaN and empty are missing, not unparsed");
}

#[test]
fn separate_date_and_time_columns() {
    let csv = "Date\tTime\tSpeed 60m\n2019-03-01\t23:50\t4.2\n2019-03-02\t00:00\t4.4\n";
    let (dataset, report) = import_text("dt", csv, &defaults()).unwrap();
    assert_eq!(report.delimiter, '\t');
    assert_eq!(report.time_columns, vec![0, 1]);
    assert_eq!(dataset.columns.len(), 1);
    assert_eq!(dataset.columns[0].height_m, Some(60.0));
    assert_eq!(dataset.step_s, TEN_MINUTES);
}

#[test]
fn unrecognized_timestamps_are_an_error() {
    let error = import_text("x", "a,b\nfoo,1\nbar,2\n", &defaults()).unwrap_err();
    assert!(matches!(error, CoreError::NoTimestampInHead { .. }), "{error}");
}

#[test]
fn outlier_timestamp_is_rejected_instead_of_allocating_decades() {
    let csv = "t,ws\n2014-01-01 00:00,1\n2014-01-01 00:10,2\n2099-01-01 00:00,3\n";
    let error = import_text("o", csv, &defaults()).unwrap_err();
    assert!(matches!(error, CoreError::TimestampsSpanTooMany { .. }), "{error}");
}

#[test]
fn overrides_win_over_guesses() {
    let mut config = defaults();
    config.columns.push(serde_json::from_str(r#"{"index":1,"ctype":"temperature","height_m":10}"#).unwrap());
    config.columns.push(serde_json::from_str(r#"{"index":2,"skip":true}"#).unwrap());
    let csv = "t,WS80,junk\n2014-01-01 00:00,1,x\n2014-01-01 00:10,2,y\n";
    let (dataset, _) = import_text("ov", csv, &config).unwrap();
    assert_eq!(dataset.columns.len(), 1);
    assert_eq!((dataset.columns[0].kind, dataset.columns[0].height_m), (ColumnType::Temperature, Some(10.0)));
    assert_eq!(dataset.columns[0].units, "°C");
}

#[test]
fn summary_uses_circular_mean_for_direction() {
    let csv = "t,dir,ws\n2014-01-01 00:00,350,1\n2014-01-01 00:10,10,3\n2014-01-01 00:30,,5\n";
    let (dataset, _) = import_text("s", csv, &defaults()).unwrap();
    let all = summaries(&dataset);
    let direction = &all[0];
    let mean = direction.mean.rem_euclid(360.0);
    assert!(mean < 1e-9 || mean > 360.0 - 1e-9, "mean of 350° and 10° is 0°, got {}", direction.mean);
    assert_eq!((direction.valid, direction.recovery), (2, 0.5));
    let speed = &all[1];
    assert_eq!((speed.valid, speed.mean, speed.min, speed.max), (3, 3.0, 1.0, 5.0));
}

#[test]
fn decimation_keeps_extremes_and_breaks_on_gaps() {
    let mut values: Vec<f64> = (0..1000).map(|step| (step % 10) as f64).collect();
    values[500] = 100.0;
    values[501] = -100.0;
    values[900..].iter_mut().for_each(|value| *value = f64::NAN);
    let buckets = 10;
    let points = series::decimate(&values, 0..1000, buckets);
    assert!(points.len() <= 2 * buckets);
    assert!(points.contains(&(500, 100.0)) && points.contains(&(501, -100.0)));
    assert!(points.last().unwrap().1.is_nan(), "an all-gap bucket breaks the line");
    assert!(points.windows(2).all(|pair| pair[0].0 <= pair[1].0), "points stay in time order");
}

#[test]
fn csv_export_round_trips() {
    let csv = "Data;WS 80m;Dir 78m\n01/02/2014 00:00;5,25;180\n01/02/2014 00:10;;190\n01/02/2014 00:20;6,75;200\n";
    let (original, _) = import_text("a", csv, &defaults()).unwrap();
    let exported = export::to_csv(&original, &Default::default()).unwrap();
    let (reimported, _) = import_text("b", &exported, &defaults()).unwrap();
    assert_eq!((original.start, original.step_s, original.len), (reimported.start, reimported.step_s, reimported.len));
    for (before, after) in original.columns.iter().zip(&reimported.columns) {
        assert_eq!(before.id, after.id);
        let same = |(a, b): (&f64, &f64)| a == b || (a.is_nan() && b.is_nan());
        assert!(before.values.iter().zip(after.values.iter()).all(same));
    }
}

#[test]
fn weibull_methods_recover_known_parameters() {
    let speeds = weibull_sample(2.0, 8.0, 200_000);
    let tolerances = [(Method::MaxLikelihood, 0.002), (Method::Wasp, 0.01), (Method::Openwind, 0.002), (Method::LeastSquares, 0.02)];
    for (method, tolerance) in tolerances {
        let fitted = weibull::fit(&speeds, method).unwrap();
        assert!(close(fitted.k, 2.0, tolerance) && close(fitted.a, 8.0, tolerance), "{method:?}: k={} A={}", fitted.k, fitted.a);
    }
    let fitted = weibull::fit(&weibull_sample(3.5, 11.0, 50_000), Method::MaxLikelihood).unwrap();
    assert!(close(fitted.k, 3.5, 0.003) && close(fitted.a, 11.0, 0.003), "k={} A={}", fitted.k, fitted.a);
    assert!(weibull::fit(&[5.0, 5.0, 5.0], Method::MaxLikelihood).is_err(), "constant speeds have no Weibull");
}

#[test]
fn histogram_bins_and_weibull_curve() {
    let mut speeds = weibull_sample(2.0, 8.0, 100_000);
    speeds.push(f64::NAN);
    let dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds)]);
    let histogram = analysis::histogram(&dataset, &view(&dataset), 0, 1.0, Some(Method::MaxLikelihood)).unwrap();
    assert_eq!((histogram.samples, histogram.start, histogram.width), (100_000, 0.0, 1.0));
    assert!(close(histogram.frequency.iter().sum::<f64>(), 1.0, 1e-12));
    let curve = histogram.weibull.unwrap();
    // Bin [0,1) of Weibull(2, 8): 1 - exp(-(1/8)^2).
    assert!(close(curve.frequency[0], 1.0 - (-(1.0f64 / 64.0)).exp(), 1e-2));
    let gamma_three_halves = 0.886_226_925_452_758;
    assert!(close(histogram.mean, 8.0 * gamma_three_halves, 1e-3), "mean = A·Γ(1.5), got {}", histogram.mean);
    let epf_of_rayleigh = 6.0 / std::f64::consts::PI;
    assert!(
        close(histogram.energy_pattern_factor, epf_of_rayleigh, 2e-3),
        "EPF of k=2 is Γ(2.5)/Γ(1.5)³ = 6/π, got {}",
        histogram.energy_pattern_factor
    );
}

#[test]
fn flags_exclude_data_cover_children_and_split_on_unflag() {
    let mut deviation = column("sd", ColumnType::WindSpeed, Some(80.0), vec![0.5; 10]);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let speeds = column("ws", ColumnType::WindSpeed, Some(80.0), (0..10).map(f64::from).collect());
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![speeds, deviation]);

    flags::flag(&mut dataset, &[0], INVALID_FLAG, 2..6).unwrap();
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 2..6).unwrap();
    assert_eq!(dataset.segments.len(), 1, "re-flagging the same range does not duplicate");
    assert_eq!(view(&dataset).excluded(0), &[2..6]);
    assert_eq!(view(&dataset).excluded(1), &[2..6], "the SD child follows its mean column");
    assert_eq!(summaries(&dataset)[0].recovery, 0.6);

    flags::unflag(&mut dataset, &[0], None, 4..5).unwrap();
    assert_eq!(view(&dataset).excluded(0), &[2..4, 5..6], "unflagging the middle splits the segment");

    dataset.flags[INVALID_FLAG].include = true;
    assert!(view(&dataset).excluded(0).is_empty(), "an included flag does not filter");
    dataset.flags[INVALID_FLAG].include = false;

    // Re-import with the columns swapped keeps the segments on the right column.
    let swapped = vec![dataset.columns[1].clone(), dataset.columns[0].clone()];
    let mut reimported = make_dataset(START_2023, TEN_MINUTES, swapped);
    flags::carry_over(&dataset, &mut reimported);
    assert!(reimported.segments.iter().all(|segment| segment.column == 1));
    assert_eq!(view(&reimported).excluded(1), &[2..4, 5..6]);
}

#[test]
fn coverage_by_calendar_month_after_filtering() {
    // Hourly from 2023-01-31 22:00: two steps in January, three in February.
    let start = START_2023 + 30 * SECONDS_PER_DAY + 22 * SECONDS_PER_HOUR;
    let speeds = column("ws", ColumnType::WindSpeed, None, vec![1.0, 2.0, 3.0, f64::NAN, 5.0]);
    let mut dataset = make_dataset(start, SECONDS_PER_HOUR, vec![speeds]);
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 0..1).unwrap();
    let coverage = analysis::coverage(&DayStats::new(&dataset, &view(&dataset)));
    let month_spans: Vec<_> = coverage.months.iter().map(|month| (month.year, month.month, month.steps())).collect();
    assert_eq!(month_spans, vec![(2023, 1, 0..2), (2023, 2, 2..5)]);
    assert_eq!(coverage.recovery[0], vec![0.5, 2.0 / 3.0]);
    assert_eq!(coverage.total[0], 0.6);
}

#[test]
fn wind_rose_sectors_speed_classes_and_energy() {
    let directions = column("dir", ColumnType::WindDirection, Some(78.0), vec![359.0, 1.0, 90.0, 180.0, 270.0, 44.0]);
    let speeds = column("ws", ColumnType::WindSpeed, Some(80.0), vec![2.0, 4.0, 5.0, 10.0, f64::NAN, 1.0]);
    let dataset = make_dataset(START_2023, TEN_MINUTES, vec![directions, speeds]);
    let quadrants = 4;
    let rose = analysis::wind_rose(&dataset, &view(&dataset), 0, Some(1), quadrants, &[3.0, 6.0], NO_CALMS).unwrap();
    assert_eq!(rose.samples, 5, "the step without speed is left out");
    assert_eq!(rose.frequency, vec![0.6, 0.2, 0.2, 0.0], "359°, 1° and 44° fall in the north sector");
    assert_eq!(rose.mean_speed[0], 7.0 / 3.0);
    assert!(rose.mean_speed[3].is_nan());
    assert!(close(rose.energy[2], 1000.0 / 1198.0, 1e-12));
    assert_eq!(rose.frequency_by_speed[0][0], 0.4, "2 and 1 m/s are below 3 m/s");
    assert_eq!(rose.frequency_by_speed[2][2], 0.2, "10 m/s is in the open-ended class");

    let directions_only = analysis::wind_rose(&dataset, &view(&dataset), 0, None, quadrants, &[3.0], NO_CALMS).unwrap();
    assert_eq!(directions_only.samples, 6);
    assert!(directions_only.frequency_by_speed.is_empty() && directions_only.mean_speed[0].is_nan());
}

#[test]
fn diurnal_profile_by_hour_and_month() {
    let steps_per_hour = 6;
    let two_days = 48 * steps_per_hour;
    let hour_values: Vec<f64> = (0..two_days).map(|step| ((step / steps_per_hour) % 24) as f64).collect();
    let alternating: Vec<f64> = (0..two_days).map(|step| if step % 2 == 0 { 350.0 } else { 10.0 }).collect();
    let dataset = make_dataset(
        START_2023,
        TEN_MINUTES,
        vec![column("ws", ColumnType::WindSpeed, None, hour_values), column("dir", ColumnType::WindDirection, None, alternating)],
    );
    let profile = analysis::diurnal(&dataset, &view(&dataset), &months(&dataset), 0);
    assert_eq!(profile.all, (0..24).map(f64::from).collect::<Vec<_>>());
    assert_eq!(profile.by_month[0], profile.all);
    assert!(profile.by_month[1].iter().all(|mean| mean.is_nan()));
    let direction_at_5h = analysis::diurnal(&dataset, &view(&dataset), &months(&dataset), 1).all[5];
    assert!(direction_at_5h < 1e-9 || direction_at_5h > 360.0 - 1e-9, "direction averages as a vector, got {direction_at_5h}");
}

#[test]
fn shear_recovers_power_and_log_law() {
    let heights = [40.0, 60.0, 80.0, 100.0];
    let alpha = 0.2;
    let steps = 24 * 6;
    let scale: Vec<f64> = (0..steps).map(|step| 4.0 + (step % 7) as f64).collect();
    let mut columns: Vec<DataColumn> = heights
        .iter()
        .map(|&height| {
            let speeds = scale.iter().map(|base| base * (height / 100.0f64).powf(alpha)).collect();
            column(&format!("ws{height}"), ColumnType::WindSpeed, Some(height), speeds)
        })
        .collect();
    columns.push(column("dir", ColumnType::WindDirection, Some(98.0), (0..steps).map(|step| (step % 4) as f64 * 90.0).collect()));
    std::sync::Arc::get_mut(&mut columns[0].values).expect("a column just built")[3] = f64::NAN;
    let dataset = make_dataset(START_2023, TEN_MINUTES, columns);
    let shear = analysis::shear(&dataset, &view(&dataset), &[0, 1, 2, 3], Some(4), 4, analysis::ShearAggregation::ApMean).unwrap();
    assert_eq!(shear.samples, steps - 1, "only steps where every height has data");
    assert!(close(shear.alpha, alpha, 1e-12), "alpha = {}", shear.alpha);
    assert!(shear.alpha_by_hour.iter().chain(&shear.alpha_by_sector).all(|value| close(*value, alpha, 1e-12)));
    assert_eq!(shear.alpha_by_sector.len(), 4);

    // Log law U = (u*/κ)·ln(z/z0) with z0 = 0.03 m.
    let roughness = 0.03;
    let log_law = heights
        .iter()
        .map(|&height| column("u", ColumnType::WindSpeed, Some(height), vec![2.5 * (height / roughness).ln(); 3]))
        .collect();
    let log_dataset = make_dataset(START_2023, TEN_MINUTES, log_law);
    let shear = analysis::shear(&log_dataset, &view(&log_dataset), &[0, 1, 2, 3], None, 12, Default::default()).unwrap();
    assert!(close(shear.z0, roughness, 1e-9), "z0 = {}", shear.z0);
    assert!(shear.alpha_by_sector.is_empty());

    let same_height = vec![
        column("a", ColumnType::WindSpeed, Some(80.0), vec![1.0]),
        column("b", ColumnType::WindSpeed, Some(80.0), vec![1.0]),
    ];
    let one_height = make_dataset(START_2023, TEN_MINUTES, same_height);
    let error = analysis::shear(&one_height, &view(&one_height), &[0, 1], None, 12, Default::default()).unwrap_err();
    assert_eq!(error, CoreError::ShearNeedsTwoHeights);
}

// ---- Property tests ----

/// xorshift64*: deterministic noise in [0, 1) for property tests.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, limit: usize) -> usize {
        (self.next() * limit as f64) as usize
    }
    /// `scale · U[0,1)`, or NaN 3 % of the time.
    fn value_or_gap(&mut self, scale: f64) -> f64 {
        let value = scale * self.next();
        if self.next() < 0.03 {
            f64::NAN
        } else {
            value
        }
    }
}

fn bit_identical(left: &ColumnSummary, right: &ColumnSummary) -> bool {
    let bits = |summary: &ColumnSummary| {
        let ColumnSummary { valid, recovery, mean, min, max } = summary;
        (*valid, recovery.to_bits(), mean.to_bits(), min.to_bits(), max.to_bits())
    };
    bits(left) == bits(right)
}

#[test]
fn incremental_refresh_equals_full_rebuild_bit_for_bit() {
    // 40-min steps from 13:20, so days are partial at both ends; a mean speed
    // with its SD child and a direction column; 3 % gaps.
    let mut rng = Rng(42);
    let steps = 2_000;
    let forty_minutes = 2_400;
    let speeds: Vec<f64> = (0..steps).map(|_| rng.value_or_gap(12.0)).collect();
    let deviations: Vec<f64> = (0..steps).map(|_| rng.value_or_gap(1.0)).collect();
    let directions: Vec<f64> = (0..steps).map(|_| rng.value_or_gap(360.0)).collect();
    let mut deviation = column("sd", ColumnType::WindSpeed, Some(80.0), deviations);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let columns = vec![
        column("ws", ColumnType::WindSpeed, Some(80.0), speeds),
        deviation,
        column("dir", ColumnType::WindDirection, Some(78.0), directions),
    ];
    let afternoon = 13 * SECONDS_PER_HOUR + 1200;
    let mut dataset = make_dataset(START_2023 + afternoon, forty_minutes, columns);
    let mut stats = DayStats::new(&dataset, &view(&dataset));
    let edits = 300;
    for edit in 0..edits {
        let first = rng.below(steps);
        let edited = first..(first + 1 + rng.below(200)).min(steps);
        let targets = [[0].as_slice(), &[2], &[0, 2]][rng.below(3)];
        if rng.next() < 0.6 {
            flags::flag(&mut dataset, targets, rng.below(3), edited.clone()).unwrap();
        } else {
            flags::unflag(&mut dataset, targets, None, edited.clone()).unwrap();
        }
        let current = view(&dataset);
        stats.refresh(&dataset, &current, targets, edited);
        let rebuilt = DayStats::new(&dataset, &current);
        for (index, (incremental, full)) in stats.summaries().iter().zip(rebuilt.summaries().iter()).enumerate() {
            assert!(bit_identical(incremental, full), "edit {edit}, column {index}: {incremental:?} vs {full:?}");
        }
    }
    // The merge of the days also agrees with one direct pass over the data.
    let current = view(&dataset);
    let direct: Vec<f64> = valid_values(&dataset.columns[0].values, &current.valid_runs(&[0], 0..dataset.len)).collect();
    let mean = direct.iter().sum::<f64>() / direct.len() as f64;
    let summary = &stats.summaries()[0];
    assert_eq!(summary.valid, direct.len());
    assert!(close(summary.mean, mean, 1e-12), "{} vs {mean}", summary.mean);
}

#[test]
fn nasa_power_files_read_with_their_site_and_recognised_parameters() {
    // The header block as the POWER API answers (hourly, point, CSV, UTC),
    // with the first rows it gave for this site; the second speed made missing.
    let power = "-BEGIN HEADER-
                 NASA/POWER Source Native Resolution Hourly Data 
                 Dates (month/day/year): 01/01/2023 through 01/01/2023 in UTC
                 Location: Latitude  -5.1234   Longitude -36.4567 
                 Elevation from MERRA-2: Average for 0.5 x 0.625 degree lat/lon region = 30.1 meters
                 The value for missing source data that cannot be computed or is outside of the sources availability range: -999 
                 Parameter(s): 
                 WS50M     MERRA-2 Wind Speed at 50 Meters (m/s) 
                 WD50M     MERRA-2 Wind Direction at 50 Meters (Degrees) 
                 T2M       MERRA-2 Temperature at 2 Meters (C) 
                 PS        MERRA-2 Surface Pressure (kPa) 
                 -END HEADER-
                 YEAR,MO,DY,HR,WS50M,WD50M,T2M,PS
                 2023,1,1,0,8.77,111.0,26.85,100.97
                 2023,1,1,1,-999,112.8,26.61,100.93
                 2023,1,1,2,7.84,114.2,26.37,100.86
                 2023,1,1,3,7.52,115.0,26.16,100.78
";
    let (dataset, report) = import_text("POWER_Point_Hourly.csv", power, &defaults()).unwrap();
    let described: Vec<(&str, ColumnType, Option<f64>)> = dataset.columns.iter().map(|column| (column.id.as_str(), column.kind, column.height_m)).collect();
    assert_eq!(
        described,
        [
            ("Vel 50m (WS50M)", ColumnType::WindSpeed, Some(50.0)),
            ("Dir 50m (WD50M)", ColumnType::WindDirection, Some(50.0)),
            ("Temp 2m (T2M)", ColumnType::Temperature, Some(2.0)),
            ("Pres (PS)", ColumnType::Pressure, None),
        ]
    );
    assert_eq!((dataset.step_s, dataset.len, report.rows_bad_time), (3600, 4, 0), "hourly, every row's time read");
    assert!(dataset.columns[0].values[1].is_nan(), "-999 is missing");
    assert_eq!((dataset.latitude, dataset.longitude), (Some(-5.1234), Some(-36.4567)), "the site from the header block");
    assert_eq!(dataset.columns[3].units, derived::KILOPASCAL, "pressure in kPa, told by its magnitude");
    assert_eq!(report.format.as_deref(), Some(wind_core::import::POWER_FORMAT));
    assert_eq!(dataset.utc_offset_min, Some(0), "asked in UTC, the file says so");
}

#[test]
fn ammonit_meteo40_files_read_with_heights_from_their_ini_part() {
    let meteo40 = "Date/time,Thies80;wind_speed;Avg,Thies80;wind_speed;StdDev,Thies80;wind_speed;Count,Vane78;wind_direction;Avg,T2;temperature;Avg,addr
                   2014-07-17 15:50:00,6.21,0.62,600,95.4,27.1,1024
                   2014-07-17 16:00:00,6.50,0.65,600,93.0,27.0,1088
                   2014-07-17 16:10:00,6.62,0.66,600,88.7,26.8,1152
                   
                   [System]
                   timezone=UTC-03:00
                   latitude=-5.1234
                   longitude=-36.4567
                   [Thies80;wind_speed]
                   sensor_height=80
                   [Vane78;wind_direction]
                   sensor_height=78
                   [T2;temperature]
                   sensor_height=2
";
    let (dataset, report) = import_text("D401234_20140717.csv", meteo40, &defaults()).unwrap();
    let described: Vec<(ColumnType, Subtype, Option<f64>)> = dataset.columns.iter().map(|column| (column.kind, column.subtype, column.height_m)).collect();
    assert_eq!(
        described,
        [
            (ColumnType::WindSpeed, Subtype::Mean, Some(80.0)),
            (ColumnType::WindSpeed, Subtype::StdDev, Some(80.0)),
            (ColumnType::WindDirection, Subtype::Mean, Some(78.0)),
            (ColumnType::Temperature, Subtype::Mean, Some(2.0)),
        ],
        "the count and the address left out, the heights from the ini part"
    );
    assert_eq!(dataset.columns[1].parent, Some(0));
    assert_eq!((dataset.len, dataset.step_s), (3, 600), "the ini part is not data");
    assert_eq!((dataset.latitude, dataset.longitude, dataset.utc_offset_min), (Some(-5.1234), Some(-36.4567), Some(-180)));
    assert_eq!(report.format.as_deref(), Some(wind_core::import::AMMONIT_FORMAT));
}

#[test]
fn nrg_symphoniepro_exports_read_past_their_long_header() {
    // Two hundred header lines of sensor history before the table: more than
    // the importer searches for data on its own.
    let mut sympro = String::from("SymphoniePRO Logger data file ----- Measurements

Site Properties
Latitude:	-5.1234
Longitude:	-36.4567
Time Zone:	UTC-03:00
Sensor History
");
    for channel in 1..=20 {
        let (description, height) = if channel == 20 { ("NRG T60 Temperature", "2.00") } else { ("NRG Class1 Anem", "80.00") };
        for line in [format!("Channel:	{channel}"), "Type:	Analog".into(), format!("Description:	{description}"), format!("Height:	{height}")] {
            sympro.push_str(&line);
            sympro.push('\n');
        }
        for _ in 0..6 {
            sympro.push_str("Scale Factor:	1.0
");
        }
    }
    sympro.push_str("Data
Timestamp	Ch1_Anem_80.00m_N_Avg_m/s	Ch1_Anem_80.00m_N_SD_m/s	Ch20_Analog_2.00m_N_Avg_C
");
    for step in 0..3 {
        sympro.push_str(&format!("2023-01-01 00:{:02}:00	6.{step}	0.6	27.{step}
", step * 10));
    }
    let (dataset, report) = import_text("001234_2023-01-01_meas.txt", &sympro, &defaults()).unwrap();
    let described: Vec<(ColumnType, Subtype, Option<f64>)> = dataset.columns.iter().map(|column| (column.kind, column.subtype, column.height_m)).collect();
    assert_eq!(
        described,
        [(ColumnType::WindSpeed, Subtype::Mean, Some(80.0)), (ColumnType::WindSpeed, Subtype::StdDev, Some(80.0)), (ColumnType::Temperature, Subtype::Mean, Some(2.0))],
        "the analog channel read as the temperature its sensor history says"
    );
    assert_eq!((dataset.len, dataset.latitude, dataset.utc_offset_min), (3, Some(-5.1234), Some(-180)));
    assert_eq!(report.format.as_deref(), Some(wind_core::import::SYMPRO_FORMAT));
}

#[test]
fn campbell_toa5_files_read_with_their_statistics_and_without_the_record() {
    let toa5 = "\"TOA5\",\"Torre_RN\",\"CR1000X\",\"12345\",\"CR1000X.Std.05\",\"CPU:torre.CR1X\",\"4321\",\"Dez_min\"\n\
                \"TIMESTAMP\",\"RECORD\",\"WS_80m_Avg\",\"WS80\",\"WS_80m_Max\",\"WD_78m\",\"AirTC_Avg\"\n\
                \"TS\",\"RN\",\"m/s\",\"m/s\",\"m/s\",\"Deg\",\"Deg C\"\n\
                \"\",\"\",\"Avg\",\"Std\",\"Max\",\"Smp\",\"Avg\"\n\
                \"2023-01-01 00:10:00\",0,6.21,0.62,8.1,95.4,27.1\n\
                \"2023-01-01 00:20:00\",1,\"NAN\",0.60,7.9,93.0,27.0\n\
                \"2023-01-01 00:30:00\",2,6.50,0.65,8.4,90.1,26.9\n\
                \"2023-01-01 00:40:00\",3,6.62,0.66,8.6,88.7,26.8\n";
    let (dataset, report) = import_text("Torre_RN_Dez_min.dat", toa5, &defaults()).unwrap();
    let described: Vec<(&str, ColumnType, Subtype, Option<f64>)> =
        dataset.columns.iter().map(|column| (column.id.as_str(), column.kind, column.subtype, column.height_m)).collect();
    assert_eq!(
        described,
        [
            ("WS_80m_Avg", ColumnType::WindSpeed, Subtype::Mean, Some(80.0)),
            ("WS80 Std", ColumnType::WindSpeed, Subtype::StdDev, Some(80.0)),
            ("WS_80m_Max", ColumnType::WindSpeed, Subtype::Max, Some(80.0)),
            ("WD_78m", ColumnType::WindDirection, Subtype::Mean, Some(78.0)),
            ("AirTC_Avg", ColumnType::Temperature, Subtype::Mean, None),
        ],
        "the RECORD counter is not a column; the processing line names the SD"
    );
    assert_eq!(dataset.columns[1].parent, Some(0), "the SD belongs to the mean at 80 m");
    assert!(dataset.columns[0].values[1].is_nan(), "NAN is missing");
    assert_eq!((dataset.step_s, dataset.len), (600, 4));
    assert_eq!(report.format.as_deref(), Some(wind_core::import::TOA5_FORMAT), "so the user can set the end-of-interval stamps");
}

#[test]
fn wind_components_become_a_speed_and_the_direction_it_comes_from() {
    // ERA5 names (u100, v100) and MERRA-2 names (U50M, V50M). A westerly
    // (u > 0), a northerly (v < 0), a south-westerly and a calm.
    let csv = "valid_time,u100,v100,U50M,V50M\n\
               2023-01-01 00:00,4,0,3,0\n\
               2023-01-01 01:00,0,-5,0,-4\n\
               2023-01-01 02:00,3,3,2,2\n\
               2023-01-01 03:00,0,0,0,0\n";
    let (mut dataset, _) = import_text("era5.csv", csv, &defaults()).unwrap();
    derived::append(&mut dataset);
    let named = |id: &str| dataset.columns.iter().position(|column| column.id == id).unwrap_or_else(|| panic!("no column {id}"));
    let (speed, direction) = (named("Vel 100m (u,v)"), named("Dir 100m (u,v)"));
    assert_eq!(dataset.columns[speed].values[..3], [4.0, 5.0, 18f64.sqrt()]);
    let degrees = &dataset.columns[direction].values;
    assert!(close(degrees[0], 270.0, 1e-12) && close(degrees[2], 225.0, 1e-12), "{degrees:?}");
    assert!(degrees[1] == 0.0 && degrees[1].is_sign_positive(), "a northerly is 0°, not −0°: {}", degrees[1]);
    assert!(degrees[3].is_nan(), "a calm has no direction");
    assert_eq!(dataset.columns[direction].kind, ColumnType::WindDirection);
    named("Vel 50m (u,v)");
    named("Vel 100m (u,v) WPD");
    assert_eq!(primary::primary(&dataset, ColumnType::WindSpeed), Some(speed), "the components' speed is the dataset's wind");
    assert_eq!(primary::primary(&dataset, ColumnType::WindDirection), Some(direction), "and its direction, at the same height");
    // An edit of a component carries to the speed.
    wind_core::edits::Edit::ScaleOffset { columns: vec![0], steps: 0..1, scale: 2.0, offset: 0.0 }.apply(&mut dataset).unwrap();
    assert_eq!(dataset.columns[speed].values[0], 8.0);
}

#[test]
fn a_logger_file_reads_through_names_associated_columns_and_its_clock() {
    // A logger export's layout: a row index before
    // the time stamp, channels named by the logger's codes, the SDs far from
    // their means (DEVP5 is nearer AVGP9 than AVGP5), a clock in UTC−3.
    let csv = "index;Timestamp;AVGP5;MAXP5;AVGP9;MAXP9;DEVP5;DEVP9\n\
               0;2019-08-22 20:20:00;4.31;7.31;4.29;6.73;1.02;1.04\n\
               1;2019-08-22 20:30:00;4.50;7.10;4.40;6.90;1.00;1.01\n\
               2;2019-08-22 20:40:00;4.62;7.20;4.55;7.00;0.98;0.99\n";
    let speed = |index: usize, name: &str, subtype: Subtype, parent: Option<usize>| ColumnOverride {
        index,
        kind: Some(ColumnType::WindSpeed),
        subtype: Some(subtype),
        height_m: Some(Some(30.0)),
        name: Some(name.into()),
        parent,
        ..ColumnOverride::default()
    };
    let config = ImportConfig {
        time_columns: Some(vec![1]),
        utc_offset_min: Some(-180),
        columns: vec![
            ColumnOverride { index: 0, skip: true, ..ColumnOverride::default() },
            speed(2, "Vel 30m A", Subtype::Mean, None),
            speed(3, "Vel 30m A Máx", Subtype::Max, Some(2)),
            speed(4, "Vel 30m B", Subtype::Mean, None),
            speed(5, "Vel 30m B Máx", Subtype::Max, Some(4)),
            speed(6, "Vel 30m A DP", Subtype::StdDev, Some(2)),
            speed(7, "Vel 30m B DP", Subtype::StdDev, Some(4)),
        ],
        ..defaults()
    };
    let (dataset, _) = import_text("COMPOSE_Torre01.csv", csv, &config).unwrap();
    let parents: Vec<(&str, Option<&str>)> =
        dataset.columns.iter().map(|column| (column.id.as_str(), column.parent.map(|parent| dataset.columns[parent].id.as_str()))).collect();
    assert_eq!(
        parents,
        [
            ("Vel 30m A", None),
            ("Vel 30m A Máx", Some("Vel 30m A")),
            ("Vel 30m B", None),
            ("Vel 30m B Máx", Some("Vel 30m B")),
            ("Vel 30m A DP", Some("Vel 30m A")),
            ("Vel 30m B DP", Some("Vel 30m B")),
        ],
        "the named parents win over the nearest mean"
    );
    assert_eq!((dataset.len, dataset.utc_offset_min), (3, Some(-180)));
    let wrong = ImportConfig { utc_offset_min: Some(900), ..defaults() };
    assert_eq!(import_text("x.csv", csv, &wrong).unwrap_err(), CoreError::UtcOffsetOutOfRange { minutes: 900 });
}

#[test]
fn summaries_follow_value_edits_of_columns_derived_far_away() {
    use wind_core::edits::Edit;
    use wind_core::model::Extrapolation;
    // A month of ten-minute speeds at 40 and 80 m, every 7th step too slow
    // for its own shear exponent: those take the month × hour table, which
    // an edit anywhere in the month moves. Scaling the first 20 days changes
    // the extrapolation on the last 11 too.
    let steps = 31 * 144;
    let lower: Vec<f64> = (0..steps).map(|step| if step % 7 == 0 { 2.0 } else { 5.0 + (step % 5) as f64 }).collect();
    let upper: Vec<f64> = lower.iter().enumerate().map(|(step, speed)| speed * if step % 3 == 0 { 1.1 } else { 1.2 }).collect();
    let speeds = vec![column("ws40", ColumnType::WindSpeed, Some(40.0), lower), column("ws80", ColumnType::WindSpeed, Some(80.0), upper)];
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, speeds);
    dataset.extrapolations = vec![Extrapolation { lower: "ws40".into(), upper: "ws80".into(), height_m: 120.0, table_only: false, ..Extrapolation::default() }];
    derived::append(&mut dataset);
    let mut stats = DayStats::new(&dataset, &view(&dataset));
    let mut history = History::default();
    let same_as_rebuilt = |stats: &DayStats, dataset: &Dataset, when: &str| {
        let rebuilt = DayStats::new(dataset, &view(dataset));
        for (index, (incremental, full)) in stats.summaries().iter().zip(rebuilt.summaries().iter()).enumerate() {
            assert!(bit_identical(incremental, full), "{when}, column {index}: {incremental:?} vs {full:?}");
        }
    };
    let scaled = Op::Edit(Edit::ScaleOffset { columns: vec![0], steps: 0..20 * 144, scale: 1.5, offset: 0.0 });
    history.apply(&mut dataset, scaled.clone()).unwrap();
    stats.follow(&dataset, &view(&dataset), &scaled);
    same_as_rebuilt(&stats, &dataset, "after the edit");
    let undone = history.undo(&mut dataset).unwrap();
    stats.follow(&dataset, &view(&dataset), &undone);
    same_as_rebuilt(&stats, &dataset, "after the undo");
    let flagged = flag_op(&[0], 0, 10..20);
    history.apply(&mut dataset, flagged.clone()).unwrap();
    stats.follow(&dataset, &view(&dataset), &flagged);
    same_as_rebuilt(&stats, &dataset, "after a flag edit, refreshed in place");
}

#[test]
fn valid_runs_match_a_step_by_step_check() {
    let mut rng = Rng(7);
    let steps = 500;
    let columns = (0..3).map(|index| column(&format!("c{index}"), ColumnType::WindSpeed, Some(10.0), vec![1.0; steps])).collect();
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, columns);
    for _ in 0..40 {
        let first = rng.below(steps);
        flags::flag(&mut dataset, &[rng.below(3)], rng.below(3), first..first + rng.below(30)).unwrap();
    }
    let current = view(&dataset);
    for selection in [vec![0], vec![1, 2], vec![0, 1, 2]] {
        let within = 37..461;
        let mut cursors: Vec<_> = selection.iter().map(|&index| current.cursor(index)).collect();
        let want: Vec<usize> = within
            .clone()
            .filter(|&step| !cursors.iter_mut().fold(false, |any, cursor| cursor.excludes(step) | any))
            .collect();
        let got: Vec<usize> = current.valid_runs(&selection, within).into_iter().flatten().collect();
        assert_eq!(got, want, "columns {selection:?}");
    }
}

#[test]
fn day_blocks_tile_the_record() {
    // 10-min steps from 13:20: the first day holds 64 steps.
    let afternoon = 13 * SECONDS_PER_HOUR + 1200;
    let dataset = make_dataset(START_2023 + afternoon, TEN_MINUTES, vec![column("a", ColumnType::Other, None, vec![0.0; 500])]);
    let days = time::days(&dataset);
    assert_eq!(days[0], 0..64);
    assert_eq!(days[1], 64..208);
    assert!(days.windows(2).all(|pair| pair[0].end == pair[1].start) && days.last().unwrap().end == 500);
    // A 2-day step leaves every other day empty but keeps day d at d days.
    let two_days = 2 * SECONDS_PER_DAY;
    let slow = make_dataset(START_2023, two_days, vec![column("a", ColumnType::Other, None, vec![0.0; 4])]);
    assert_eq!(time::days(&slow), vec![0..1, 1..1, 1..2, 2..2, 2..3, 3..3, 3..4]);
}

// ---- Regressions from the 2026-10-13 code review ----

#[test]
fn huge_outliers_are_an_error_not_a_crash() {
    // A logger error code such as 9.9e37 parses as a finite number.
    let logger_error_code = 9.9e37;
    let speeds = column("ws", ColumnType::WindSpeed, Some(80.0), vec![5.0, 6.0, 7.0, logger_error_code]);
    let dataset = make_dataset(START_2023, TEN_MINUTES, vec![speeds]);
    let error = analysis::histogram(&dataset, &view(&dataset), 0, 0.5, None).unwrap_err();
    assert!(matches!(error, CoreError::TooManyBins { .. }), "{error}");
    // Least squares puts each speed on the line, with no bins to allocate.
    assert!(weibull::fit(&[5.0, 6.0, logger_error_code], Method::LeastSquares).unwrap().k.is_finite());
}

#[test]
fn duplicate_headers_never_collide_with_real_ones() {
    let csv = "t,A,A,A_2\n2014-01-01 00:00,1,2,3\n2014-01-01 00:10,1,2,3\n";
    let (dataset, _) = import_text("dup", csv, &defaults()).unwrap();
    let ids: Vec<&str> = dataset.columns.iter().map(|column| column.id.as_str()).collect();
    assert_eq!(ids, ["A", "A_3", "A_2"]);
}

#[test]
fn a_stray_off_phase_first_row_does_not_shift_the_grid() {
    // A logger restart at 00:03 in a 10-min file.
    let csv = "t,ws\n2014-01-01 00:03,1\n2014-01-01 00:10,2\n2014-01-01 00:20,3\n2014-01-01 00:30,4\n";
    let (dataset, report) = import_text("p", csv, &defaults()).unwrap();
    assert_eq!((dataset.start, dataset.step_s, dataset.len), (START_2014, TEN_MINUTES, 4), "grid on :00, not :03");
    assert_eq!((report.off_grid, report.duplicates), (1, 0));
    assert_eq!(dataset.columns[0].values[..], [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn carried_flags_follow_the_file_timestamps() {
    let rows: String = (0..6).map(|minute_tens| format!("2014-01-01 00:{minute_tens}0,{minute_tens}\n")).collect();
    let csv = format!("t,ws\n{rows}");
    let (mut old, _) = import_text("a", &csv, &defaults()).unwrap();
    flags::flag(&mut old, &[0], INVALID_FLAG, 2..4).unwrap();
    let flagged_steps = |dataset: &Dataset| dataset.segments[0].steps();

    // Same file, timestamps now read as interval ends: same rows stay flagged.
    let mut interval_ends = defaults();
    interval_ends.timestamp_meaning = TimeStampMeaning::End;
    let (mut new, _) = import_text("a", &csv, &interval_ends).unwrap();
    assert_eq!(new.start, old.start - TEN_MINUTES);
    flags::carry_over(&old, &mut new);
    assert_eq!(flagged_steps(&new), 2..4);

    // Two earlier rows added: the flag stays on the same times.
    let longer = format!("t,ws\n2013-12-31 23:40,8\n2013-12-31 23:50,9\n{rows}");
    let (mut new, _) = import_text("a", &longer, &defaults()).unwrap();
    flags::carry_over(&old, &mut new);
    assert_eq!(flagged_steps(&new), 4..6);

    // A coarser step keeps the flag on every step it touches.
    let mut hourly = defaults();
    hourly.step_s = Some(SECONDS_PER_HOUR);
    let (mut new, _) = import_text("a", &csv, &hourly).unwrap();
    flags::carry_over(&old, &mut new);
    assert_eq!(flagged_steps(&new), 0..1);
}

#[test]
fn unflag_with_a_reversed_range_is_a_no_op() {
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 100])]);
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 0..100).unwrap();
    #[allow(clippy::reversed_empty_ranges)]
    flags::unflag(&mut dataset, &[0], None, 60..40).unwrap();
    assert_eq!(dataset.segments.len(), 1);
    assert_eq!(dataset.segments[0].steps(), 0..100);
}

// ---- Regressions from the second code review ----

#[test]
fn export_reports_formats_a_plain_date_time_cannot_render() {
    let (dataset, _) = import_text("z", "t,ws\n2014-01-01 00:00,1\n2014-01-01 00:10,2\n", &defaults()).unwrap();
    let options = export::CsvExportOptions { time_format: "%Y-%m-%d %H:%M %z".into(), ..Default::default() };
    let error = export::to_csv(&dataset, &options).unwrap_err();
    assert!(matches!(error, CoreError::InvalidTimeFormat { .. }), "{error}");
}

#[test]
fn export_writes_decimal_commas_and_source_timestamps_by_default() {
    let csv = "t;ws\n2014-01-01 00:10;5,25\n2014-01-01 00:20;6\n";
    let mut interval_ends = defaults();
    interval_ends.timestamp_meaning = TimeStampMeaning::End;
    let (dataset, _) = import_text("e", csv, &interval_ends).unwrap();
    let options = export::CsvExportOptions { delimiter: ';', decimal_comma: true, ..Default::default() };
    let exported = export::to_csv(&dataset, &options).unwrap();
    let first_row = exported.lines().nth(1).unwrap();
    assert_eq!(first_row, "2014-01-01 00:10:00;5,25", "stamps as in the source file, decimal comma");
    let as_starts = export::CsvExportOptions { timestamp_meaning: Some(TimeStampMeaning::Start), ..options };
    let first_row = export::to_csv(&dataset, &as_starts).unwrap().lines().nth(1).unwrap().to_string();
    assert!(first_row.starts_with("2014-01-01 00:00:00"), "{first_row}");
}

#[test]
fn windows_1252_files_keep_their_accents() {
    // "Vel 80m Máx" and "Direção" written by a Latin-1 logger.
    let mut bytes = b"t;Vel 80m M\xe1x;Dire\xe7\xe3o 78m\n".to_vec();
    bytes.extend_from_slice(b"2014-01-01 00:00;5;180\n2014-01-01 00:10;6;190\n");
    let text = wind_core::import::decode_text(&bytes);
    let (dataset, _) = import_text("latin", &text, &defaults()).unwrap();
    assert_eq!(dataset.columns[0].id, "Vel 80m Máx");
    assert_eq!((dataset.columns[0].kind, dataset.columns[0].subtype), (ColumnType::WindSpeed, Subtype::Max));
    assert_eq!(dataset.columns[1].kind, ColumnType::WindDirection);
    let utf8 = "já é UTF-8".as_bytes();
    assert!(matches!(wind_core::import::decode_text(utf8), std::borrow::Cow::Borrowed(_)), "valid UTF-8 is not copied");
}

#[test]
fn a_height_correction_can_clear_the_guess() {
    let csv = "t,WS80\n2014-01-01 00:00,1\n2014-01-01 00:10,2\n";
    let corrected = |correction: &str| {
        let mut config = defaults();
        config.columns.push(serde_json::from_str(correction).unwrap());
        import_text("h", csv, &config).unwrap().0.columns[0].height_m
    };
    assert_eq!(corrected(r#"{"index":1}"#), Some(80.0), "absent: keep the guess");
    assert_eq!(corrected(r#"{"index":1,"height_m":null}"#), None, "null: no height");
    assert_eq!(corrected(r#"{"index":1,"height_m":60}"#), Some(60.0), "number: that height");
}

#[test]
fn unflag_rejects_a_column_that_does_not_exist() {
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 10])]);
    let error = flags::unflag(&mut dataset, &[7], None, 0..5).unwrap_err();
    assert_eq!(error, CoreError::NoSuchColumn { column: 7 });
}

// ---- Step 6: ops and the undo history ----

fn flag_op(columns: &[usize], flag: usize, steps: std::ops::Range<usize>) -> Op {
    Op::Flag { columns: columns.to_vec(), flag, steps }
}

#[test]
fn flagging_keeps_one_canonical_segment_list() {
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 20])]);
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 2..6).unwrap();
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 4..10).unwrap();
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 10..12).unwrap();
    let whole = Segment { column: 0, flag: INVALID_FLAG, start: 2, end: 12 };
    assert_eq!(dataset.segments, [whole], "overlapping and touching segments merge");
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 5..7).unwrap();
    assert_eq!(dataset.segments, [whole], "flagging inside a segment does not split it");
}

#[test]
fn undo_and_redo_restore_every_state_exactly() {
    let mut deviation = column("sd", ColumnType::WindSpeed, Some(80.0), vec![0.5; 300]);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let speeds = column("ws", ColumnType::WindSpeed, Some(80.0), (0..300).map(f64::from).collect());
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![speeds, deviation]);
    let mut stats = DayStats::new(&dataset, &view(&dataset));
    let mut history = History::default();
    let ops = [
        flag_op(&[0], INVALID_FLAG, 10..200),
        flag_op(&[0], TOWER_SHADOW_FLAG, 150..250),
        Op::Unflag { columns: vec![0], flag: None, steps: 100..160 },
    ];
    let mut states = vec![dataset.segments.clone()];
    for op in &ops {
        history.apply(&mut dataset, op.clone()).unwrap();
        states.push(dataset.segments.clone());
    }
    let refreshed = |dataset: &Dataset, stats: &mut DayStats, op: &Op| {
        let (columns, steps) = op.reach();
        let current = view(dataset);
        stats.refresh(dataset, &current, columns, steps);
        let rebuilt = DayStats::new(dataset, &current);
        stats.summaries().iter().zip(rebuilt.summaries().iter()).all(|(incremental, full)| bit_identical(incremental, full))
    };
    for want in states.iter().rev().skip(1) {
        let op = history.undo(&mut dataset).expect("something to undo");
        assert_eq!(&dataset.segments, want);
        assert!(refreshed(&dataset, &mut stats, &op), "summaries after undoing {op:?}");
    }
    assert!(!history.can_undo() && history.undo(&mut dataset).is_none());
    for want in states.iter().skip(1) {
        let op = history.redo(&mut dataset).expect("something to redo");
        assert_eq!(&dataset.segments, want);
        assert!(refreshed(&dataset, &mut stats, &op), "summaries after redoing {op:?}");
    }
    assert!(!history.can_redo() && history.redo(&mut dataset).is_none());
}

#[test]
fn a_new_edit_clears_redo_and_no_ops_are_not_recorded() {
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 50])]);
    let mut history = History::default();
    history.apply(&mut dataset, flag_op(&[0], INVALID_FLAG, 0..10)).unwrap();
    history.apply(&mut dataset, flag_op(&[0], INVALID_FLAG, 0..10)).unwrap();
    history.apply(&mut dataset, Op::Unflag { columns: vec![0], flag: None, steps: 30..40 }).unwrap();
    history.undo(&mut dataset).unwrap();
    assert!(!history.can_undo(), "re-flagging and unflagging nothing recorded no step");
    assert!(dataset.segments.is_empty());

    history.redo(&mut dataset).unwrap();
    history.undo(&mut dataset).unwrap();
    history.apply(&mut dataset, flag_op(&[0], INVALID_FLAG, 20..30)).unwrap();
    assert!(!history.can_redo(), "a new edit drops what was undone");

    let error = history.apply(&mut dataset, flag_op(&[9], INVALID_FLAG, 0..5)).unwrap_err();
    assert_eq!(error, CoreError::NoSuchColumn { column: 9 });
    history.undo(&mut dataset).unwrap();
    assert!(!history.can_undo(), "a failed edit records nothing");
}

#[test]
fn history_keeps_only_the_latest_revisions() {
    let extra = 5;
    let edits = MAX_REVISIONS + extra;
    // A gap between flagged steps, so every edit adds its own segment.
    let spacing = 2;
    let mut dataset =
        make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; edits * spacing])]);
    let mut history = History::default();
    for edit in 0..edits {
        let step = edit * spacing;
        history.apply(&mut dataset, flag_op(&[0], INVALID_FLAG, step..step + 1)).unwrap();
    }
    let mut undone = 0;
    while history.undo(&mut dataset).is_some() {
        undone += 1;
    }
    assert_eq!(undone, MAX_REVISIONS);
    assert_eq!(dataset.segments.len(), extra, "the oldest edits stay applied");
}

// ---- Step 7: projects ----

#[test]
fn restoring_saved_flags_validates_before_changing_anything() {
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 20])]);
    let saved = |start, end| vec![Segment { column: 0, flag: INVALID_FLAG, start, end }];
    let past_the_end = flags::restore(&mut dataset, flags::default_flags(), saved(15, 21)).unwrap_err();
    assert_eq!(past_the_end, CoreError::SegmentOutOfRange { start: 15, end: 21, len: 20 });
    let empty = flags::restore(&mut dataset, flags::default_flags(), saved(5, 5)).unwrap_err();
    assert!(matches!(empty, CoreError::SegmentOutOfRange { .. }));
    let no_such_flag = flags::restore(&mut dataset, Vec::new(), saved(0, 5)).unwrap_err();
    assert_eq!(no_such_flag, CoreError::NoSuchFlag { flag: INVALID_FLAG });
    assert!(dataset.segments.is_empty(), "a rejected restore changes nothing");

    let touching = [saved(0, 5), saved(5, 8)].concat();
    flags::restore(&mut dataset, flags::default_flags(), touching).unwrap();
    assert_eq!(dataset.segments, saved(0, 8), "restored segments are normalized");
}

// ---- Item 12: derived columns ----

/// Site elevation used by the derived-column tests, m.
const SITE_ELEVATION_M: f64 = 500.0;
/// Pascals in a hectopascal.
const PASCALS_PER_HECTOPASCAL: f64 = 100.0;
/// A met mast file: speed with its SD at 80 m, temperature at 80 m (°C) and
/// pressure at 2 m (hPa), with gaps in each input.
const TOWER_WITH_PRESSURE: &str = "t,WS80,WS80_SD,Temp 80m,Pres 2m\n\
    2014-01-01 00:00,8.0,0.8,15.0,1000.0\n\
    2014-01-01 00:10,3.0,0.6,20.0,\n\
    2014-01-01 00:20,,0.5,25.0,990.0\n\
    2014-01-01 00:30,10.0,1.0,,995.0\n";

/// IEC 61400-12-1 moist air, written out: (1/T)·(p/R_d − φ·P_w·(1/R_d − 1/R_w)),
/// P_w = 2.05·10⁻⁵·exp(0.0631846·T) Pa, φ the relative humidity as a share.
fn moist_air(pressure_pa: f64, celsius: f64, humidity_percent: f64) -> f64 {
    let kelvin = celsius + derived::CELSIUS_ZERO_K;
    let vapour_pa = 2.05e-5 * (0.063_184_6 * kelvin).exp();
    let (dry, vapour) = (derived::DRY_AIR_GAS_CONSTANT_J_KG_K, derived::WATER_VAPOUR_GAS_CONSTANT_J_KG_K);
    (pressure_pa / dry - humidity_percent / 100.0 * vapour_pa * (1.0 / dry - 1.0 / vapour)) / kelvin
}

/// [`moist_air`] at the humidity assumed without a sensor.
fn default_air(pressure_pa: f64, celsius: f64) -> f64 {
    moist_air(pressure_pa, celsius, derived::DEFAULT_RELATIVE_HUMIDITY_PERCENT)
}

fn imported_with_derived(csv: &str, elevation_m: Option<f64>) -> Dataset {
    let config = ImportConfig { elevation_m, ..defaults() };
    let (mut dataset, _) = import_text("derived", csv, &config).unwrap();
    derived::append(&mut dataset);
    dataset
}

fn column_index(dataset: &Dataset, id: &str) -> usize {
    dataset.columns.iter().position(|column| column.id == id).unwrap_or_else(|| panic!("no column {id}"))
}

/// Equal to rounding, or both NaN.
fn same_value(value: f64, want: f64) -> bool {
    (value.is_nan() && want.is_nan()) || close(value, want, 1e-12)
}

#[test]
fn derived_columns_follow_their_formulas() {
    let dataset = imported_with_derived(TOWER_WITH_PRESSURE, Some(SITE_ELEVATION_M));
    let ids: Vec<&str> = dataset.columns.iter().map(|column| column.id.as_str()).collect();
    assert_eq!(ids, ["WS80", "WS80_SD", "Temp 80m", "Pres 2m", derived::AIR_DENSITY_ID, "WS80 TI", "WS80 WPD"]);
    let density = column_index(&dataset, derived::AIR_DENSITY_ID);
    let (turbulence, power) = (density + 1, density + 2);
    assert_eq!(dataset.columns[density].derived, Some(Derived::AirDensity { temperature: 2, pressure: Some(3), humidity: None }));
    assert_eq!(dataset.columns[turbulence].derived, Some(Derived::TurbulenceIntensity { speed: 0, deviation: 1 }));
    assert_eq!(dataset.columns[power].derived, Some(Derived::PowerDensity { speed: 0, density: Some(density) }));
    assert_eq!((dataset.columns[density].units.as_str(), dataset.columns[density].height_m), ("kg/m³", Some(80.0)));
    assert_eq!((dataset.columns[power].units.as_str(), dataset.columns[power].kind), ("W/m²", ColumnType::PowerDensity));

    // Pressure read at 2 m, moved to the 80 m temperature sensor by the standard atmosphere.
    let adjustment_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 80.0) - derived::standard_pressure_pa(SITE_ELEVATION_M + 2.0);
    let gap_fill_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 80.0);
    let want_density = [
        default_air(1000.0 * PASCALS_PER_HECTOPASCAL + adjustment_pa, 15.0),
        default_air(gap_fill_pa, 20.0),
        default_air(990.0 * PASCALS_PER_HECTOPASCAL + adjustment_pa, 25.0),
        f64::NAN,
    ];
    // As the original: σ/U at any speed above 0.01 m/s (3 m/s here gives 0.2).
    let want_turbulence = [0.1, 0.6 / 3.0, f64::NAN, 0.1];
    // ½·ρ·U³.
    let want_power = [0.5 * want_density[0] * 512.0, 0.5 * want_density[1] * 27.0, f64::NAN, f64::NAN];
    for (column, want) in [(density, want_density), (turbulence, want_turbulence), (power, want_power)] {
        let values = &dataset.columns[column].values;
        assert!(values.iter().zip(want).all(|(&value, want)| same_value(value, want)), "{}: {values:?} vs {want:?}", dataset.columns[column].id);
    }
    assert!(close(dataset.columns[density].values[0], 1.2, 0.01), "a plausible density: {}", dataset.columns[density].values[0]);
}

#[test]
fn air_density_reads_the_humidity_where_there_is_one() {
    let csv = "t,WS80,Temp 2m,UR 2m\n2014-01-01 00:00,8,20,80\n2014-01-01 00:10,9,20,\n2014-01-01 00:20,9,20,0\n";
    let mut dataset = imported_with_derived(csv, Some(SITE_ELEVATION_M));
    let density = derived::air_density_column(&dataset).unwrap();
    assert_eq!(dataset.columns[density].derived, Some(Derived::AirDensity { temperature: 1, pressure: None, humidity: Some(2) }));
    let standard_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 2.0);
    let want = [moist_air(standard_pa, 20.0, 80.0), default_air(standard_pa, 20.0), moist_air(standard_pa, 20.0, 0.0)];
    assert!(dataset.columns[density].values.iter().zip(want).all(|(&value, want)| close(value, want, 1e-12)), "{:?}", dataset.columns[density].values);
    // The original's own expression at 100 kPa, 20 °C and 80 %: it takes R_d = 287.058.
    let original = (100.0 / 0.287_058 - 2.05e-5 * (0.063_184_6f64 * 293.15).exp() / 1000.0 * 0.8 * 1.316_746) / 293.15;
    assert!(close(moist_air(100_000.0, 20.0, 80.0), original, 1e-4), "{original}");
    // Humid air is lighter: 80 % at 20 °C takes about 0.7 % off.
    assert!(want[0] < want[1] && want[1] < want[2]);
    // A flag on the humidity leaves the density valid (the original falls back to 50 %).
    dataset.segments = vec![Segment { column: 2, flag: INVALID_FLAG, start: 0, end: 3 }];
    assert!(FilterView::new(&dataset).excluded(density).is_empty());
    // An edit of the humidity computes the density again.
    dataset.columns[2].values = vec![0.0; 3].into();
    derived::recompute(&mut dataset, &[2]);
    assert!(close(dataset.columns[density].values[0], want[2], 1e-12));
}

#[test]
fn air_density_needs_a_pressure_column_or_the_site_elevation() {
    let csv = "t,WS80,Temp 2m\n2014-01-01 00:00,8,15\n2014-01-01 00:10,9,16\n";
    let without_site = imported_with_derived(csv, None);
    assert!(derived::air_density_column(&without_site).is_none());
    let power = column_index(&without_site, "WS80 WPD");
    assert_eq!(without_site.columns[power].derived, Some(Derived::PowerDensity { speed: 0, density: None }));
    // ½·ρ·U³ at the standard density.
    assert_eq!(without_site.columns[power].values[0], 0.5 * analysis::RHO_STD * 512.0);

    let with_site = imported_with_derived(csv, Some(SITE_ELEVATION_M));
    let density = derived::air_density_column(&with_site).expect("temperature plus site elevation");
    let standard_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 2.0);
    assert!(close(with_site.columns[density].values[0], default_air(standard_pa, 15.0), 1e-12));
    assert!(with_site.columns.iter().all(|column| column.kind != ColumnType::TurbulenceIntensity), "TI needs an SD column");
}

#[test]
fn import_tells_temperature_and_pressure_units_by_magnitude() {
    let csv = "t,Temp 2m,Pres 2m,Baro 3m,Pressure 4m\n2014-01-01 00:00,288.1,95.2,1001,101300\n2014-01-01 00:10,290.4,95.3,1002,101250\n";
    let (dataset, _) = import_text("units", csv, &defaults()).unwrap();
    let units: Vec<&str> = dataset.columns.iter().map(|column| column.units.as_str()).collect();
    assert_eq!(units, [derived::KELVIN, derived::KILOPASCAL, derived::HECTOPASCAL, derived::PASCAL]);
    let mut config = defaults();
    config.columns.push(serde_json::from_str(r#"{"index":1,"units":"°F"}"#).unwrap());
    let (corrected, _) = import_text("units", csv, &config).unwrap();
    assert_eq!(corrected.columns[0].units, "°F", "a user's units win");
    let at_sea_level = imported_with_derived(csv, Some(0.0));
    let density = derived::air_density_column(&at_sea_level).unwrap();
    // The primary pressure: 3 and 4 m are colocated (within the 1 m threshold)
    // and equally complete, so the first, 3 m in hPa; the only thermometer
    // (2 m, K): about 100.1 kPa at 288 K, 1.21 kg/m³, never a tenth or ten times that.
    assert!(close(at_sea_level.columns[density].values[0], 1.21, 0.01), "{}", at_sea_level.columns[density].values[0]);
}

#[test]
fn a_site_elevation_outside_the_standard_atmosphere_is_refused() {
    let config = ImportConfig { elevation_m: Some(20_000.0), ..defaults() };
    let error = import_text("high", "t,ws\n2014-01-01 00:00,1\n2014-01-01 00:10,2\n", &config).unwrap_err();
    assert!(matches!(error, CoreError::ElevationOutOfRange { .. }), "{error}");
}

#[test]
fn derived_columns_inherit_flags_and_take_none() {
    let mut dataset = imported_with_derived(TOWER_WITH_PRESSURE, Some(SITE_ELEVATION_M));
    let (speed, deviation, temperature) = (0, 1, 2);
    let density = derived::air_density_column(&dataset).unwrap();
    let (turbulence, power) = (density + 1, density + 2);
    let error = flags::flag(&mut dataset, &[power], INVALID_FLAG, 0..2).unwrap_err();
    assert_eq!(error, CoreError::DerivedColumnNotFlaggable { column: "WS80 WPD".into() });
    assert!(flags::unflag(&mut dataset, &[turbulence], None, 0..2).is_err());
    let derived_segment = vec![Segment { column: density, flag: INVALID_FLAG, start: 0, end: 1 }];
    assert!(flags::restore(&mut dataset, flags::default_flags(), derived_segment).is_err(), "a saved flag on a derived column");

    flags::flag(&mut dataset, &[temperature], INVALID_FLAG, 0..1).unwrap();
    flags::flag(&mut dataset, &[deviation], INVALID_FLAG, 1..2).unwrap();
    flags::flag(&mut dataset, &[speed], INVALID_FLAG, 3..4).unwrap();
    let current = view(&dataset);
    assert_eq!(current.excluded(density), &[0..1], "density: temperature and pressure");
    assert_eq!(current.excluded(turbulence), &[1..2, 3..4], "TI: speed and its SD");
    assert_eq!(current.excluded(power), &[0..1, 3..4], "WPD: speed and, through the density, temperature");
    assert_eq!(derived::measured_sources(&dataset, dataset.columns[power].derived.as_ref().unwrap()), [0, 2, 3]);
}

#[test]
fn derived_ids_never_collide_and_csv_export_leaves_them_out_by_default() {
    let csv = "t,WS80,WS80 WPD\n2014-01-01 00:00,8,1\n2014-01-01 00:10,9,2\n";
    let dataset = imported_with_derived(csv, None);
    let ids: Vec<&str> = dataset.columns.iter().map(|column| column.id.as_str()).collect();
    assert_eq!(ids, ["WS80", "WS80 WPD", "WS80 WPD_2"]);
    let header = |options: &export::CsvExportOptions| export::to_csv(&dataset, options).unwrap().lines().next().unwrap().to_string();
    assert_eq!(header(&Default::default()), "Timestamp,WS80,WS80 WPD");
    let with_derived = export::CsvExportOptions { include_derived: true, ..Default::default() };
    assert_eq!(header(&with_derived), "Timestamp,WS80,WS80 WPD,WS80 WPD_2");
}

/// A random mast with every derived column: speed + SD, temperature and
/// pressure, 3 % gaps, 40-min steps from mid-afternoon (partial days).
fn random_mast(rng: &mut Rng, steps: usize) -> Dataset {
    let forty_minutes = 2_400;
    let speeds: Vec<f64> = (0..steps).map(|_| rng.value_or_gap(15.0)).collect();
    let deviations: Vec<f64> = (0..steps).map(|_| rng.value_or_gap(1.5)).collect();
    let temperatures: Vec<f64> = (0..steps).map(|_| 10.0 + rng.value_or_gap(20.0)).collect();
    let pressures: Vec<f64> = (0..steps).map(|_| 950.0 + rng.value_or_gap(60.0)).collect();
    let mut deviation = column("sd", ColumnType::WindSpeed, Some(80.0), deviations);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let mut temperature = column("temp", ColumnType::Temperature, Some(78.0), temperatures);
    temperature.units = derived::CELSIUS.into();
    let mut pressure = column("pres", ColumnType::Pressure, Some(2.0), pressures);
    pressure.units = derived::HECTOPASCAL.into();
    let columns = vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds), deviation, temperature, pressure];
    let afternoon = 13 * SECONDS_PER_HOUR + 1200;
    let mut dataset = make_dataset(START_2023 + afternoon, forty_minutes, columns);
    dataset.elevation_m = Some(SITE_ELEVATION_M);
    derived::append(&mut dataset);
    dataset
}

#[test]
fn derived_exclusions_are_the_union_of_their_sources_and_summaries_stay_incremental() {
    let mut rng = Rng(12);
    let steps = 1_500;
    let mut dataset = random_mast(&mut rng, steps);
    let measured = 4;
    assert_eq!(dataset.columns.len(), measured + 3, "density, TI and WPD");
    let mut stats = DayStats::new(&dataset, &view(&dataset));
    let edits = 200;
    for edit in 0..edits {
        let first = rng.below(steps);
        let edited = first..(first + 1 + rng.below(150)).min(steps);
        let targets = [[0].as_slice(), &[1], &[2], &[3], &[0, 2]][rng.below(5)];
        if rng.next() < 0.6 {
            flags::flag(&mut dataset, targets, rng.below(3), edited.clone()).unwrap();
        } else {
            flags::unflag(&mut dataset, targets, None, edited.clone()).unwrap();
        }
        let current = view(&dataset);
        for derived_column in measured..dataset.columns.len() {
            let sources = derived::measured_sources(&dataset, dataset.columns[derived_column].derived.as_ref().unwrap());
            let mut cursors: Vec<_> = sources.iter().map(|&source| current.cursor(source)).collect();
            let want: Vec<usize> =
                (0..steps).filter(|&step| cursors.iter_mut().fold(false, |any, cursor| cursor.excludes(step) | any)).collect();
            let got: Vec<usize> = current.excluded(derived_column).iter().cloned().flatten().collect();
            assert_eq!(got, want, "edit {edit}, column {}", dataset.columns[derived_column].id);
        }
        stats.refresh(&dataset, &current, targets, edited);
        let rebuilt = DayStats::new(&dataset, &current);
        for (index, (incremental, full)) in stats.summaries().iter().zip(rebuilt.summaries().iter()).enumerate() {
            assert!(bit_identical(incremental, full), "edit {edit}, column {index}: {incremental:?} vs {full:?}");
        }
    }
}

#[test]
fn derived_values_match_the_per_step_formulas_on_random_data() {
    let mut rng = Rng(99);
    let dataset = random_mast(&mut rng, 3_000);
    let value = |column: usize, step: usize| dataset.columns[column].values[step];
    let density = derived::air_density_column(&dataset).unwrap();
    let adjustment_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 78.0) - derived::standard_pressure_pa(SITE_ELEVATION_M + 2.0);
    let fill_pa = derived::standard_pressure_pa(SITE_ELEVATION_M + 78.0);
    for step in 0..dataset.len {
        let (speed, deviation, celsius, hectopascals) = (value(0, step), value(1, step), value(2, step), value(3, step));
        let pressure_pa = if hectopascals.is_nan() { fill_pa } else { hectopascals * PASCALS_PER_HECTOPASCAL + adjustment_pa };
        let want_density = default_air(pressure_pa, celsius);
        let want_turbulence = if speed.is_nan() || deviation.is_nan() {
            f64::NAN
        } else if speed <= derived::TI_ORIGINAL_MIN_SPEED_M_S {
            0.0
        } else {
            deviation / speed
        };
        // ½·ρ·U³.
        let want_power = 0.5 * want_density * speed * speed * speed;
        for (column, want) in [(density, want_density), (density + 1, want_turbulence), (density + 2, want_power)] {
            assert!(same_value(value(column, step), want), "step {step}, {}: {} vs {want}", dataset.columns[column].id, value(column, step));
        }
    }
}

// ---- Integration of the parallel fronts ----

#[test]
fn rules_refuse_derived_columns_like_manual_flags() {
    let mut dataset = imported_with_derived(TOWER_WITH_PRESSURE, Some(SITE_ELEVATION_M));
    let turbulence = column_index(&dataset, "WS80 TI");
    let rule = wind_core::rules::FlagRule {
        criterion: wind_core::rules::Criterion::ChangeAbove { limit: 0.0 },
        min_steps: 1,
        columns: vec![turbulence],
        flag: INVALID_FLAG,
    };
    let run = wind_core::rules::RuleRun::new(vec![rule], 0..dataset.len).unwrap();
    let error = History::default().apply(&mut dataset, Op::FlagByRules(run)).unwrap_err();
    assert_eq!(error, CoreError::DerivedColumnNotFlaggable { column: "WS80 TI".into() });
    assert!(dataset.segments.is_empty(), "a refused run flags nothing");
}

// ---- Step 10: "Filtrar por" ----

/// Hourly steps over 400 days from an afternoon, so months, hours and the
/// year's turn are all crossed.
const CRITERIA_STEPS: usize = 9_600;
const CRITERIA_TRIALS: usize = 60;
const DEGREES_PER_TURN: f64 = 360.0;
const MAX_SPEED_M_S: f64 = 20.0;
const MONTHS: u32 = 12;
const HOURS: u32 = 24;

/// The direction sector of `direction`, written again here (independent of
/// the core's) the way the wind rose defines it: sector 0 centred on north.
fn sector_by_hand(direction: f64, sectors: usize) -> usize {
    let width = DEGREES_PER_TURN / sectors as f64;
    ((direction.rem_euclid(DEGREES_PER_TURN) + width / 2.0) / width) as usize % sectors
}

/// Whether `step` of `column` is kept, checked step by step from the
/// definitions in ESPEC-INTERFACE §3.2, with the calendar from chrono.
fn kept_by_hand(dataset: &Dataset, criteria: &wind_core::criteria::Criteria, column: usize, step: usize) -> bool {
    use chrono::{DateTime, Datelike, Timelike};
    let owner = dataset.columns[column].parent.unwrap_or(column);
    let flags_here: Vec<usize> = dataset
        .segments
        .iter()
        .filter(|segment| segment.column == owner && segment.steps().contains(&step))
        .map(|segment| segment.flag)
        .collect();
    let flag_kept = match &criteria.flags {
        None => flags_here.iter().all(|&flag| dataset.flags[flag].include),
        Some(selection) => {
            flags_here.iter().all(|&flag| selection.keep[flag]) && (selection.keep_unflagged || !flags_here.is_empty())
        }
    };
    let instant = DateTime::from_timestamp(dataset.time_at(step), 0).unwrap();
    let month_kept = criteria.months.map_or(true, |(first, last)| {
        let month = instant.month();
        if first <= last { (first..=last).contains(&month) } else { month >= first || month <= last }
    });
    let hours_kept = criteria.hours.map_or(true, |(start, end)| {
        let hour = instant.hour();
        if start < end { (start..end).contains(&hour) } else { hour >= start || hour < end }
    });
    let sector_kept = criteria.sector.map_or(true, |sector| {
        let direction = dataset.columns[sector.direction].values[step];
        !direction.is_nan() && sector_by_hand(direction, sector.sectors) == sector.sector
    });
    let value_kept = criteria.value.map_or(true, |value| {
        let found = dataset.columns[value.column].values[step];
        value.min <= found && found < value.max
    });
    let date_kept = criteria.steps.as_ref().map_or(true, |steps| steps.contains(&step));
    flag_kept && date_kept && month_kept && hours_kept && sector_kept && value_kept
}

#[test]
fn filter_criteria_match_a_step_by_step_check() {
    use wind_core::criteria::{Criteria, FlagSelection, SectorCriterion, ValueCriterion};
    let mut rng = Rng(2024);
    let afternoon = 15 * SECONDS_PER_HOUR;
    let speeds: Vec<f64> = (0..CRITERIA_STEPS).map(|_| rng.value_or_gap(MAX_SPEED_M_S)).collect();
    let deviations: Vec<f64> = (0..CRITERIA_STEPS).map(|_| rng.value_or_gap(1.0)).collect();
    let directions: Vec<f64> = (0..CRITERIA_STEPS).map(|_| rng.value_or_gap(DEGREES_PER_TURN)).collect();
    let mut deviation = column("sd", ColumnType::WindSpeed, Some(80.0), deviations);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let columns =
        vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds), deviation, column("dir", ColumnType::WindDirection, Some(78.0), directions)];
    let mut dataset = make_dataset(START_2023 + afternoon, SECONDS_PER_HOUR, columns);
    let flaggable = [0, 2];
    for _ in 0..40 {
        let first = rng.below(CRITERIA_STEPS);
        let target = flaggable[rng.below(flaggable.len())];
        let flag = rng.below(dataset.flags.len());
        flags::flag(&mut dataset, &[target], flag, first..first + rng.below(300)).unwrap();
    }
    dataset.flags[TOWER_SHADOW_FLAG].include = true;
    for trial in 0..CRITERIA_TRIALS {
        let on = |rng: &mut Rng| rng.next() < 0.5;
        let mut criteria = Criteria::default();
        if on(&mut rng) {
            let keep = (0..dataset.flags.len()).map(|_| rng.next() < 0.5).collect();
            criteria.flags = Some(FlagSelection { keep, keep_unflagged: rng.next() < 0.7 });
        }
        if on(&mut rng) {
            let first = rng.below(CRITERIA_STEPS);
            criteria.steps = Some(first..first + rng.below(CRITERIA_STEPS));
        }
        if on(&mut rng) {
            criteria.months = Some((1 + rng.below(MONTHS as usize) as u32, 1 + rng.below(MONTHS as usize) as u32));
        }
        if on(&mut rng) {
            criteria.hours = Some((rng.below(HOURS as usize) as u32, 1 + rng.below(HOURS as usize) as u32));
        }
        if on(&mut rng) {
            let sectors = 1 + rng.below(16);
            criteria.sector = Some(SectorCriterion { direction: 2, sectors, sector: rng.below(sectors) });
        }
        if on(&mut rng) {
            let min = rng.next() * MAX_SPEED_M_S;
            criteria.value = Some(ValueCriterion { column: 0, min, max: min + rng.next() * MAX_SPEED_M_S });
        }
        let filtered = FilterView::with_criteria(&dataset, &criteria).unwrap();
        for read in [vec![0], vec![1], vec![0, 2]] {
            let want: Vec<usize> =
                (0..CRITERIA_STEPS).filter(|&step| read.iter().all(|&column| kept_by_hand(&dataset, &criteria, column, step))).collect();
            let got: Vec<usize> = filtered.valid_runs(&read, 0..CRITERIA_STEPS).into_iter().flatten().collect();
            assert_eq!(got, want, "trial {trial}, columns {read:?}, {criteria:?}");
            let mut cursor = filtered.cursor(read[0]);
            let by_cursor: Vec<usize> = (0..CRITERIA_STEPS).filter(|&step| !cursor.excludes(step)).collect();
            let want_first: Vec<usize> = (0..CRITERIA_STEPS).filter(|&step| kept_by_hand(&dataset, &criteria, read[0], step)).collect();
            assert_eq!(by_cursor, want_first, "cursor, trial {trial}");
        }
    }
}

#[test]
fn filter_criteria_are_checked() {
    use wind_core::criteria::{Criteria, FlagSelection, SectorCriterion, ValueCriterion};
    let dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![1.0; 10])]);
    let refused = |criteria: Criteria| FilterView::with_criteria(&dataset, &criteria).unwrap_err();
    let thirteenth_month = 13;
    let past_midnight = 25;
    assert!(matches!(refused(Criteria { months: Some((1, thirteenth_month)), ..Default::default() }), CoreError::FilterCriterionInvalid { .. }));
    assert!(matches!(refused(Criteria { hours: Some((0, past_midnight)), ..Default::default() }), CoreError::FilterCriterionInvalid { .. }));
    let empty_range = ValueCriterion { column: 0, min: 5.0, max: 5.0 };
    assert!(matches!(refused(Criteria { value: Some(empty_range), ..Default::default() }), CoreError::FilterCriterionInvalid { .. }));
    let short_list = FlagSelection { keep: vec![true], keep_unflagged: true };
    assert!(matches!(refused(Criteria { flags: Some(short_list), ..Default::default() }), CoreError::FilterCriterionInvalid { .. }));
    let missing_column = SectorCriterion { direction: 3, sectors: 12, sector: 0 };
    assert_eq!(refused(Criteria { sector: Some(missing_column), ..Default::default() }), CoreError::NoSuchColumn { column: 3 });
    let no_sectors = SectorCriterion { direction: 0, sectors: 0, sector: 0 };
    assert!(matches!(refused(Criteria { sector: Some(no_sectors), ..Default::default() }), CoreError::SectorsOutOfRange { .. }));
}

// ---- Step 13: method corrections ----

/// Days of each month of 2023 (not a leap year).
const DAYS_2023: [usize; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
/// Mean days per month in the mean of monthly means (manual def-momm.htm).
const MEAN_DAYS: [f64; 12] = [31.0, 28.24, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
const HOURS_PER_DAY_STEPS: usize = 24;

#[test]
fn ap_momm_weighs_months_by_days_and_completeness() {
    // Hourly, 2023-01-01 to 2024-02-01: January twice, the second half of
    // December missing at the top. Speeds are constant within a month, so
    // each monthly mean is known.
    let top_speed = |month: usize| 5.0 + month as f64;
    let bottom_speed = |month: usize| 4.0 + 0.5 * month as f64;
    let month_lengths: Vec<usize> = DAYS_2023.iter().chain([&DAYS_2023[0]]).map(|days| days * HOURS_PER_DAY_STEPS).collect();
    let (mut top, mut bottom, mut month_of_step) = (Vec::new(), Vec::new(), Vec::new());
    for (index, &steps) in month_lengths.iter().enumerate() {
        let month = index % 12;
        for step in 0..steps {
            let december_second_half = month == 11 && step >= steps / 2;
            top.push(if december_second_half { f64::NAN } else { top_speed(month) });
            bottom.push(bottom_speed(month));
            month_of_step.push(month);
        }
    }
    let columns = vec![
        column("top", ColumnType::WindSpeed, Some(100.0), top.clone()),
        column("bottom", ColumnType::WindSpeed, Some(40.0), bottom),
    ];
    let dataset = make_dataset(START_2023, SECONDS_PER_HOUR, columns);
    let momm = analysis::shear(&dataset, &view(&dataset), &[0, 1], None, 12, analysis::ShearAggregation::ApMomm).unwrap();
    let mean = analysis::shear(&dataset, &view(&dataset), &[0, 1], None, 12, analysis::ShearAggregation::ApMean).unwrap();

    // Weights λ·Y: concurrent steps per calendar month against a full month.
    let mut concurrent = [0usize; 12];
    for (step, &month) in month_of_step.iter().enumerate() {
        if !top[step].is_nan() {
            concurrent[month] += 1;
        }
    }
    let weight = |month: usize| (concurrent[month] as f64 / (HOURS_PER_DAY_STEPS as f64 * MEAN_DAYS[month])).min(1.0) * MEAN_DAYS[month];
    let weighted = |speed: &dyn Fn(usize) -> f64| (0..12).map(|month| weight(month) * speed(month)).sum::<f64>() / (0..12).map(weight).sum::<f64>();
    assert!(close(momm.means[0], weighted(&top_speed), 1e-12), "{} vs {}", momm.means[0], weighted(&top_speed));
    assert!(close(momm.means[1], weighted(&bottom_speed), 1e-12));
    assert_eq!(momm.aggregation, analysis::ShearAggregation::ApMomm);
    // The simple mean leans to the doubled January; the MoMM does not.
    let simple_top = top.iter().filter(|speed| !speed.is_nan()).sum::<f64>() / top.iter().filter(|speed| !speed.is_nan()).count() as f64;
    assert!(close(mean.means[0], simple_top, 1e-12));
    assert!((momm.means[0] - mean.means[0]).abs() > 0.1, "the two methods differ on a biased record");
    assert_eq!(analysis::ShearAggregation::default(), analysis::ShearAggregation::ApMomm, "the reference product's default");
}

#[test]
fn calms_leave_the_sectors_and_complete_the_total() {
    let speeds = vec![0.2, 0.4, 3.0, 5.0, 0.1, 7.0, f64::NAN, 9.0];
    let directions = vec![0.0, 90.0, 180.0, 270.0, 0.0, 90.0, 180.0, f64::NAN];
    let dataset = make_dataset(
        START_2023,
        TEN_MINUTES,
        vec![column("dir", ColumnType::WindDirection, None, directions), column("ws", ColumnType::WindSpeed, None, speeds)],
    );
    let quadrants = 4;
    let calm_threshold = 0.5;
    let rose = analysis::wind_rose(&dataset, &view(&dataset), 0, Some(1), quadrants, &[3.0], calm_threshold).unwrap();
    // Valid pairs: steps 0–5 (step 6 lacks a speed, step 7 a direction); three are calm.
    assert_eq!(rose.samples, 6);
    assert!(close(rose.calm, 0.5, 1e-15));
    assert!(close(rose.frequency.iter().sum::<f64>() + rose.calm, 1.0, 1e-15), "directions and calms add up to 100 %");
    assert!(close(rose.mean_speed[1], 7.0, 1e-15), "east holds only the 7 m/s step: the 0.4 m/s one was calm");
    let without_speed = analysis::wind_rose(&dataset, &view(&dataset), 0, None, quadrants, &[], calm_threshold).unwrap();
    assert_eq!(without_speed.calm, 0.0, "no speed, no calms");
    let error = analysis::wind_rose(&dataset, &view(&dataset), 0, Some(1), quadrants, &[], -1.0).unwrap_err();
    assert!(matches!(error, CoreError::CalmThresholdInvalid { .. }));
}

#[test]
fn energy_figures_use_the_measured_air_density() {
    let dataset = imported_with_derived(TOWER_WITH_PRESSURE, Some(SITE_ELEVATION_M));
    let speed = column_index(&dataset, "WS80");
    let density = column_index(&dataset, derived::AIR_DENSITY_ID);
    let histogram = analysis::histogram(&dataset, &view(&dataset), speed, 1.0, None).unwrap();
    // By hand: ½·ρ·u³ per step with a speed, ρ from the column or RHO_STD where it has none.
    let steps: Vec<(f64, f64)> = (0..dataset.len)
        .filter(|&step| !dataset.columns[speed].values[step].is_nan())
        .map(|step| {
            let rho = dataset.columns[density].values[step];
            (dataset.columns[speed].values[step], if rho.is_finite() { rho } else { analysis::RHO_STD })
        })
        .collect();
    let power = steps.iter().map(|(speed, rho)| 0.5 * rho * speed.powi(3)).sum::<f64>() / steps.len() as f64;
    let mean_rho = steps.iter().map(|(_, rho)| rho).sum::<f64>() / steps.len() as f64;
    assert!(close(histogram.power_density, power, 1e-12), "{} vs {power}", histogram.power_density);
    assert!(close(histogram.air_density, mean_rho, 1e-12));
    assert!(histogram.density_measured);
    let no_density = make_dataset(START_2023, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, None, vec![2.0, 4.0])]);
    let standard = analysis::histogram(&no_density, &view(&no_density), 0, 1.0, None).unwrap();
    assert_eq!((standard.air_density, standard.density_measured), (analysis::RHO_STD, false));
    assert!(close(standard.power_density, 0.5 * analysis::RHO_STD * (8.0 + 64.0) / 2.0, 1e-15));
}

// ---- Step 14: confidence intervals ----

const TRUE_SHAPE: f64 = 2.0;
const TRUE_SCALE: f64 = 8.0;
/// Hourly steps over 400 days.
const BOOTSTRAP_DAYS: usize = 400;
const HOURS_PER_DAY_COUNT: usize = 24;
const TWO_DECIMALS: f64 = 100.0;

/// A Weibull variate by inversion, rounded to a logger's two decimals.
fn weibull_draw(rng: &mut Rng) -> f64 {
    let speed = TRUE_SCALE * (-(1.0 - rng.next()).ln()).powf(1.0 / TRUE_SHAPE);
    (speed * TWO_DECIMALS).round() / TWO_DECIMALS
}

#[test]
fn splitmix64_matches_the_reference_sequence() {
    // First outputs of Vigna's splitmix64.c seeded with 0.
    let mut random = wind_core::bootstrap::SplitMix64::new(0);
    assert_eq!(random.next_u64(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(random.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    let bound = 7;
    assert!((0..1_000).all(|_| random.below(bound) < bound));
}

#[test]
fn a_histogram_fits_like_its_speeds() {
    let mut rng = Rng(11);
    let speeds: Vec<f64> = (0..5_000).map(|_| weibull_draw(&mut rng)).collect();
    let mut counts = std::collections::BTreeMap::new();
    for &speed in &speeds {
        *counts.entry((speed * TWO_DECIMALS).round() as u64).or_insert(0.0) += 1.0;
    }
    let histogram: Vec<(f64, f64)> = counts.iter().map(|(&class, &count)| (class as f64 / TWO_DECIMALS, count)).collect();
    for method in [Method::MaxLikelihood, Method::LeastSquares, Method::Wasp, Method::Openwind] {
        let (raw, binned) = (weibull::fit(&speeds, method).unwrap(), weibull::fit_weighted(&histogram, method).unwrap());
        assert!(close(binned.k, raw.k, 1e-9) && close(binned.a, raw.a, 1e-9), "{method:?}: {binned:?} vs {raw:?}");
    }
}

/// An hourly record of `BOOTSTRAP_DAYS` days of speeds, either independent
/// every hour or one value held all day (the extreme of autocorrelation).
fn bootstrap_record(held_all_day: bool) -> Dataset {
    let mut rng = Rng(29);
    let mut speeds = Vec::with_capacity(BOOTSTRAP_DAYS * HOURS_PER_DAY_COUNT);
    for _ in 0..BOOTSTRAP_DAYS {
        let daily = weibull_draw(&mut rng);
        for _ in 0..HOURS_PER_DAY_COUNT {
            speeds.push(if held_all_day { daily } else { weibull_draw(&mut rng) });
        }
    }
    make_dataset(START_2023, SECONDS_PER_HOUR, vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds)])
}

#[test]
fn weibull_intervals_hold_the_truth_and_widen_with_autocorrelation() {
    use wind_core::bootstrap::weibull_interval;
    let independent = bootstrap_record(false);
    let interval = weibull_interval(&independent, &view(&independent), 0, Method::MaxLikelihood).unwrap();
    let fit = weibull::fit(&independent.columns[0].values, Method::MaxLikelihood).unwrap();
    assert!(interval.k.low < TRUE_SHAPE && TRUE_SHAPE < interval.k.high, "{interval:?}");
    assert!(interval.a.low < TRUE_SCALE && TRUE_SCALE < interval.a.high, "{interval:?}");
    assert!(interval.k.low < fit.k && fit.k < interval.k.high);
    let again = weibull_interval(&independent, &view(&independent), 0, Method::MaxLikelihood).unwrap();
    assert_eq!(interval, again, "a fixed seed: the same data, the same interval");

    // The same number of steps, but only one value per day: far less
    // information, and the day blocks see it.
    let held = bootstrap_record(true);
    let held_interval = weibull_interval(&held, &view(&held), 0, Method::MaxLikelihood).unwrap();
    let width = |interval: wind_core::bootstrap::Interval| interval.high - interval.low;
    assert!(width(held_interval.k) > 3.0 * width(interval.k), "{held_interval:?} vs {interval:?}");

    let one_day = make_dataset(START_2023, SECONDS_PER_HOUR, vec![column("ws", ColumnType::WindSpeed, None, vec![5.0, 6.0, 7.0])]);
    assert_eq!(weibull_interval(&one_day, &view(&one_day), 0, Method::MaxLikelihood), None, "one day: nothing to resample");
}

#[test]
fn the_shear_exponent_comes_with_its_interval() {
    let mut rng = Rng(5);
    let alpha = 0.2;
    let (low_height, high_height) = (40.0, 100.0);
    let steps = BOOTSTRAP_DAYS * HOURS_PER_DAY_COUNT;
    let mut top = Vec::with_capacity(steps);
    let mut bottom = Vec::with_capacity(steps);
    for _ in 0..steps {
        let speed = weibull_draw(&mut rng);
        top.push(speed);
        // Noise around the power law, so α varies between replicates.
        bottom.push(speed * (low_height / high_height as f64).powf(alpha) * (0.9 + 0.2 * rng.next()));
    }
    let columns =
        vec![column("top", ColumnType::WindSpeed, Some(high_height), top), column("bottom", ColumnType::WindSpeed, Some(low_height), bottom)];
    let dataset = make_dataset(START_2023, SECONDS_PER_HOUR, columns);
    for aggregation in [analysis::ShearAggregation::ApMean, analysis::ShearAggregation::ApMomm] {
        let shear = analysis::shear(&dataset, &view(&dataset), &[0, 1], None, 12, aggregation).unwrap();
        let interval = shear.alpha_interval.expect("400 days of data");
        assert!(interval.low < shear.alpha && shear.alpha < interval.high, "{aggregation:?}: {} in {interval:?}", shear.alpha);
        assert!(interval.high - interval.low < 0.05, "a tight interval for a long, clean record: {interval:?}");
    }
}

#[test]
fn grouped_logger_speeds_fit_like_the_raw_ones() {
    let mut rng = Rng(77);
    let two_decimals: Vec<f64> = (0..20_000).map(|_| weibull_draw(&mut rng)).collect();
    let three_decimals: Vec<f64> = two_decimals.iter().map(|speed| speed + 0.001).collect();
    for speeds in [&two_decimals, &three_decimals] {
        let one_each: Vec<(f64, f64)> = speeds.iter().map(|&speed| (speed, 1.0)).collect();
        for method in [Method::MaxLikelihood, Method::LeastSquares, Method::Wasp, Method::Openwind] {
            let (fast, raw) = (weibull::fit(speeds, method).unwrap(), weibull::fit_weighted(&one_each, method).unwrap());
            assert!(close(fast.k, raw.k, 1e-12) && close(fast.a, raw.a, 1e-12), "{method:?}: {fast:?} vs {raw:?}");
        }
    }
}

// ---- Step 15: what the main window shows ----

#[test]
fn a_logger_preamble_places_the_site_and_the_configuration_wins() {
    let csv = "Logger: X\nLat: -5,1234; Lon: -36,4567\nt;ws\n2014-01-01 00:00;1\n2014-01-01 00:10;2\n";
    let (dataset, _) = import_text("site", csv, &defaults()).unwrap();
    assert_eq!((dataset.latitude, dataset.longitude), (Some(-5.1234), Some(-36.4567)));
    let configured = ImportConfig { latitude: Some(-6.5), ..defaults() };
    let (dataset, _) = import_text("site", csv, &configured).unwrap();
    assert_eq!((dataset.latitude, dataset.longitude), (Some(-6.5), Some(-36.4567)), "a configured latitude wins");
    let past_the_pole = ImportConfig { latitude: Some(95.0), ..defaults() };
    assert!(matches!(import_text("site", csv, &past_the_pole), Err(CoreError::SiteLocationOutOfRange { .. })));
    let (no_preamble, _) = import_text("site", "t,ws\n2014-01-01 00:00,1\n2014-01-01 00:10,2\n", &defaults()).unwrap();
    assert_eq!((no_preamble.latitude, no_preamble.longitude), (None, None));
}

#[test]
fn monthly_means_average_each_calendar_month_over_every_year() {
    // Hourly over 13 months from January: the value is the month (1–12), so
    // both Januaries average to 1; the first 10 days of March are flagged.
    let mut values = Vec::new();
    for (index, &days) in DAYS_2023.iter().chain([&DAYS_2023[0]]).enumerate() {
        values.extend(std::iter::repeat((index % 12 + 1) as f64).take(days * HOURS_PER_DAY_STEPS));
    }
    let mut dataset = make_dataset(START_2023, SECONDS_PER_HOUR, vec![column("ws", ColumnType::WindSpeed, None, values)]);
    let march_start = (DAYS_2023[0] + DAYS_2023[1]) * HOURS_PER_DAY_STEPS;
    let flagged_hours = 10 * HOURS_PER_DAY_STEPS;
    flags::flag(&mut dataset, &[0], INVALID_FLAG, march_start..march_start + flagged_hours).unwrap();
    let monthly = analysis::monthly_means(&dataset, &view(&dataset), &months(&dataset), 0);
    for month in 0..12 {
        assert_eq!(monthly.means[month], (month + 1) as f64, "month {month}");
    }
    assert_eq!(monthly.samples[0], 2 * DAYS_2023[0] * HOURS_PER_DAY_STEPS, "two Januaries");
    assert_eq!(monthly.samples[2], DAYS_2023[2] * HOURS_PER_DAY_STEPS - flagged_hours, "flagged March hours are out");
}

#[test]
fn turbulence_bins_hand_checked_with_the_iec_category() {
    use analysis::{IecEdition, TurbulenceVersus};
    // TI = SD / speed: 0.11, 0.13 and 0.12 near 15 m/s, 0.10 at 8 m/s; with
    // the IEC cut the dataset chooses, below 4 m/s and without an SD there is no TI.
    let speeds = vec![15.0, 15.2, 14.8, 8.0, 3.0, 15.0];
    let deviations = vec![1.65, 1.976, 1.776, 0.8, 0.3, f64::NAN];
    let directions = vec![0.0, 90.0, 180.0, 270.0, 0.0, 0.0];
    let mut deviation = column("ws sd", ColumnType::WindSpeed, Some(80.0), deviations);
    (deviation.subtype, deviation.parent) = (Subtype::StdDev, Some(0));
    let columns = vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds), deviation, column("dir", ColumnType::WindDirection, Some(78.0), directions)];
    let mut dataset = make_dataset(START_2023, TEN_MINUTES, columns);
    dataset.ti_cutoff_m_s = Some(derived::TI_IEC_MIN_SPEED_M_S);
    derived::append(&mut dataset);
    let stats = DayStats::new(&dataset, &view(&dataset));
    let run = |dataset: &Dataset, versus, edition| analysis::turbulence(dataset, &view(dataset), stats.months(), &[0], versus, edition).unwrap();

    let by_speed = run(&dataset, TurbulenceVersus::Speed { bin_width: 1.0 }, IecEdition::Fourth);
    let centers: Vec<f64> = by_speed.bins.iter().map(|bin| bin.center).collect();
    assert_eq!(centers, (8..=15).map(f64::from).collect::<Vec<_>>(), "from the first bin with data to the last");
    let (eight, fifteen) = (by_speed.bins[0].stats, by_speed.bins[7].stats);
    assert_eq!((eight.count, by_speed.bins[1].stats.count, fifteen.count), (1, 0, 3));
    assert!(close(eight.mean, 0.10, 1e-12) && eight.deviation.is_nan());
    // Mean 0.12, sample SD 0.01, peak 0.13; representative 0.12 + 1.28 · 0.01.
    assert!(close(fifteen.mean, 0.12, 1e-12) && close(fifteen.deviation, 0.01, 1e-9) && close(fifteen.peak, 0.13, 1e-12), "{fifteen:?}");
    assert!(close(fifteen.figure, 0.1328, 1e-9));
    assert_eq!(by_speed.at_15[0].category, Some("C"), "0.1328 is under C's 0.1348 at 15 m/s");
    let second = run(&dataset, TurbulenceVersus::Speed { bin_width: 1.0 }, IecEdition::Second);
    assert!(close(second.at_15[0].stats.figure, 0.13, 1e-9), "characteristic = mean + SD");
    assert_eq!(second.at_15[0].category, Some("B"));

    let by_sector = run(&dataset, TurbulenceVersus::Direction { column: 2, sectors: 4 }, IecEdition::Fourth);
    let sector_means: Vec<f64> = by_sector.bins.iter().map(|bin| bin.stats.mean).collect();
    assert!(sector_means.iter().zip([0.11, 0.13, 0.12, 0.10]).all(|(&found, want)| close(found, want, 1e-12)), "{sector_means:?}");
    let by_hour = run(&dataset, TurbulenceVersus::Hour, IecEdition::Fourth);
    assert_eq!(by_hour.bins.len(), 24);
    assert!(by_hour.bins[0].stats.count == 4 && close(by_hour.bins[0].stats.mean, 0.115, 1e-12));
    let by_month = run(&dataset, TurbulenceVersus::Month, IecEdition::Fourth);
    assert_eq!((by_month.bins[0].center, by_month.bins[0].stats.count, by_month.bins[1].stats.count), (1.0, 4, 0));
    let by_height = run(&dataset, TurbulenceVersus::Height, IecEdition::Fourth);
    assert_eq!((by_height.bins.len(), by_height.bins[0].center, by_height.bins[0].stats.count), (1, 80.0, 4));

    // A flagged speed takes its TI out.
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 1..2).unwrap();
    let flagged = analysis::turbulence(&dataset, &view(&dataset), stats.months(), &[0], TurbulenceVersus::Height, IecEdition::Fourth).unwrap();
    assert_eq!((flagged.bins[0].stats.count, flagged.at_15[0].stats.count), (3, 2));
    let no_ti = analysis::turbulence(&dataset, &view(&dataset), stats.months(), &[2], TurbulenceVersus::Hour, IecEdition::Fourth);
    assert_eq!(no_ti.unwrap_err(), CoreError::NoTurbulenceIntensity { column: "dir".into() });
}

#[test]
fn extreme_winds_hand_checked_by_month_and_by_storm() {
    use analysis::{ExtremePeriod, GumbelFit, PeakMethod, DEFAULT_INDEPENDENCE_S, SECONDS_PER_YEAR};
    const HOUR: i64 = 3_600;
    const HOURS_PER_DAY: usize = 24;
    // January to April 2023 and one day of May, hourly.
    const DAYS: usize = 31 + 28 + 31 + 30 + 1;
    const STEPS: usize = DAYS * HOURS_PER_DAY;
    const MARCH: usize = (31 + 28) * HOURS_PER_DAY;
    let monthly_peaks = [(100, 20.0), (31 * HOURS_PER_DAY + 50, 25.0), (MARCH + 10, 22.0), ((31 + 28 + 31) * HOURS_PER_DAY + 5, 30.0)];
    let mut speeds = vec![5.0; STEPS];
    for (step, value) in monthly_peaks {
        speeds[step] = value;
    }
    // March keeps 11 of its 31 days: below half, so its 22 m/s does not count; nor does May's single day.
    speeds[MARCH + 11 * HOURS_PER_DAY..MARCH + 31 * HOURS_PER_DAY].fill(f64::NAN);
    // January misses its first hour: complete enough at half, not at all.
    speeds[0] = f64::NAN;
    let dataset = make_dataset(START_2023, HOUR, vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds)]);
    let stats = DayStats::new(&dataset, &view(&dataset));
    let monthly = PeakMethod::PeriodicMaxima { period: ExtremePeriod::Month, min_recovery: 0.5 };
    let found = analysis::extremes(&dataset, &view(&dataset), stats.months(), 0, monthly, GumbelFit::MaxLikelihood, true).unwrap();
    let peaks: Vec<(usize, f64)> = found.peaks.iter().map(|peak| (peak.step, peak.value)).collect();
    assert_eq!(peaks, [monthly_peaks[0], monthly_peaks[1], monthly_peaks[3]]);
    assert_eq!(found.per_year, 12.0);
    // Maximum likelihood of 20, 25, 30, solved independently: β = 3.5843389891, μ = 22.9719278312;
    // annualized over 12 months, the 50-year wind is 45.8645464853.
    assert!(close(found.fit.scale, 3.584_338_989_1, 1e-9) && close(found.fit.mode, 22.971_927_831_2, 1e-9), "{:?}", found.fit);
    assert!(close(found.design, 45.864_546_485_3, 1e-9), "{}", found.design);
    assert_eq!(found.interval, None, "a resample of three peaks often has no spread: no interval");
    let strict = PeakMethod::PeriodicMaxima { period: ExtremePeriod::Month, min_recovery: 1.0 };
    let too_few = analysis::extremes(&dataset, &view(&dataset), stats.months(), 0, strict, GumbelFit::MaxLikelihood, false).unwrap_err();
    assert!(matches!(too_few, CoreError::TooFewPeaks { .. }), "{too_few:?}");

    // Storms above 15 m/s: the two first are 17 h apart, under the 48 h independence, so one storm.
    let mut speeds = vec![5.0; STEPS];
    for (step, value) in [(10, 16.0), (11, 18.0), (12, 17.0), (30, 21.0), (31, 16.0), (200, 17.0), (400, 24.0)] {
        speeds[step] = value;
    }
    let dataset = make_dataset(START_2023, HOUR, vec![column("ws", ColumnType::WindSpeed, Some(80.0), speeds)]);
    let storms = PeakMethod::IndependentStorms { threshold: Some(15.0), independence_s: DEFAULT_INDEPENDENCE_S };
    let found = analysis::extremes(&dataset, &view(&dataset), stats.months(), 0, storms, GumbelFit::Gringorten, false).unwrap();
    let peaks: Vec<(usize, f64, Range<usize>)> = found.peaks.iter().map(|peak| (peak.step, peak.value, peak.span.clone())).collect();
    assert_eq!(peaks, [(30, 21.0, 10..32), (200, 17.0, 200..201), (400, 24.0, 400..401)]);
    let years = STEPS as f64 * HOUR as f64 / SECONDS_PER_YEAR;
    assert!(close(found.per_year, 3.0 / years, 1e-12));
    let recommended = PeakMethod::IndependentStorms { threshold: None, independence_s: 0 };
    let found = analysis::extremes(&dataset, &view(&dataset), stats.months(), 0, recommended, GumbelFit::MaxLikelihood, true).unwrap();
    assert!(found.per_year >= analysis::TARGET_STORMS_PER_YEAR, "{} storms a year", found.per_year);
    let interval = found.interval.expect("enough peaks for an interval");
    assert!(interval.low <= found.design && found.design <= interval.high, "{interval:?} around {}", found.design);
}

#[test]
fn period_means_and_the_monthly_table_hand_checked() {
    use analysis::MonthlyTable;
    use series::MeanPeriod;
    // Hourly, 2023 and 2024 (a leap year): each step holds its month's
    // number in 2023 and one more in 2024.
    let start_2024 = START_2023 + 365 * SECONDS_PER_DAY;
    let steps = (365 + 366) * 24;
    let values: Vec<f64> = (0..steps)
        .map(|step| {
            let time = START_2023 + step as i64 * SECONDS_PER_HOUR;
            let month = chrono::DateTime::from_timestamp(time, 0).unwrap().format("%m").to_string().parse::<f64>().unwrap();
            if time < start_2024 { month } else { month + 1.0 }
        })
        .collect();
    let dataset = make_dataset(START_2023, SECONDS_PER_HOUR, vec![column("ws", ColumnType::WindSpeed, Some(80.0), values)]);
    let stats = DayStats::new(&dataset, &view(&dataset));
    let days = stats.period_means(0, MeanPeriod::Day);
    assert_eq!((days.len(), days[0].mean, days[0].valid, days[0].steps.clone()), (731, 1.0, 24, 0..24));
    let months = stats.period_means(0, MeanPeriod::Month);
    let month_means: Vec<f64> = months.iter().map(|month| month.mean).collect();
    assert_eq!(month_means[..12], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    assert_eq!(month_means[12..], [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0]);
    let years = stats.period_means(0, MeanPeriod::Year);
    // 2023: Σ month · days / 365 = 2382 / 365, as the hours weigh them.
    assert!(years.len() == 2 && close(years[0].mean, 2382.0 / 365.0, 1e-12), "{years:?}");

    let table: MonthlyTable = analysis::monthly_table(&dataset, &stats, 0);
    assert_eq!(table.years, [2023, 2024]);
    assert!(table.recovery.iter().flatten().all(|&share| share == 1.0), "every month complete");
    assert_eq!(table.mean_of_years[0], 1.5);
    // Values solved independently: annual means 6.5260273973 and 7.5136612022.
    assert!(close(table.annual[0], 6.526_027_397_260_274, 1e-12) && close(table.annual[1], 7.513_661_202_185_792_5, 1e-12), "{:?}", table.annual);
    assert!(close(table.momm, 7.023_053_334_793_56, 1e-12), "{}", table.momm);
    assert!(close(table.interannual_variability, 0.099_484_052_775_851_47, 1e-9), "{}", table.interannual_variability);
}

#[test]
fn scatter_line_hand_checked_with_a_time_ordered_sample() {
    let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0, f64::NAN];
    let ys = vec![2.1, 3.9, 6.2, 7.8, 10.1, 3.0];
    let dataset = make_dataset(START_2023, TEN_MINUTES, vec![column("x", ColumnType::WindSpeed, None, xs), column("y", ColumnType::WindSpeed, None, ys)]);
    let found = analysis::scatter(&dataset, &view(&dataset), 0, 1, 2).unwrap();
    assert_eq!((found.count, found.stride), (5, 3), "the step without x is left out");
    assert_eq!(found.points, [(1.0, 2.1), (4.0, 7.8)]);
    assert!(close(found.slope, 1.99, 1e-12) && (found.intercept - 0.05).abs() < 1e-12 && close(found.r_squared, 0.997_305_328_900_977, 1e-12), "{found:?}");
}

#[test]
fn mcp_recovers_a_known_relation_and_predicts_the_long_term() {
    use wind_core::mcp::{self, McpMethod};
    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;
    // Reference: hourly, January to June 2023. Site: ten-minute steps, March
    // to May, each hour's steps worth 1.5 · reference + 0.5.
    let reference_hours = (31 + 28 + 31 + 30 + 31 + 30) * 24;
    let reference_speed = |hour: usize| 5.0 + (hour % 17) as f64 * 0.5;
    let reference = make_dataset(START_2023, HOUR, vec![column("ref", ColumnType::WindSpeed, Some(50.0), (0..reference_hours).map(reference_speed).collect())]);
    let march_hour = (31 + 28) * 24;
    let site_hours = (31 + 30 + 31) * 24;
    let site_values: Vec<f64> = (0..site_hours * 6).map(|step| 1.5 * reference_speed(march_hour + step / 6) + 0.5).collect();
    let site = make_dataset(START_2023 + march_hour as i64 * HOUR, TEN_MINUTES, vec![column("ws", ColumnType::WindSpeed, Some(80.0), site_values)]);
    let averaged = |dataset: &Dataset, period| mcp::average(dataset, &view(dataset), 0, period).unwrap();
    let found = mcp::mcp(&averaged(&site, HOUR), &averaged(&reference, HOUR), None, HOUR, true, &mcp::McpOptions::default()).unwrap();
    assert_eq!((found.concurrent, found.folds), (site_hours, 3), "March, April and May");
    assert!(close(found.correlation, 1.0, 1e-12));
    let reference_mean = (0..reference_hours).map(reference_speed).sum::<f64>() / reference_hours as f64;
    let exact_lines = found.results.iter().filter(|result| matches!(result.method, McpMethod::LeastSquares | McpMethod::TotalLeastSquares | McpMethod::VarianceRatio | McpMethod::SpeedSort));
    for result in exact_lines {
        let line = result.predictor.line().unwrap();
        assert!(close(line.slope, 1.5, 1e-9) && close(line.intercept, 0.5, 1e-9), "{:?}: {line:?}", result.method);
        assert!(result.cv_rmse < 1e-9 && close(result.spread_ratio, 1.0, 1e-9), "{result:?}");
        assert!(close(result.long_term_mean, 1.5 * reference_mean + 0.5, 1e-9));
        let interval = result.long_term_interval.expect("three months give an interval");
        assert!(interval.low <= result.long_term_mean + 1e-9 && result.long_term_mean - 1e-9 <= interval.high, "{interval:?}");
    }
    // The slices sit on the exact line, so they predict it; the Weibull mapping only matches distributions.
    let slices = found.results.iter().find(|result| result.method == McpMethod::VerticalSlice).unwrap();
    assert!(close(slices.long_term_mean, 1.5 * reference_mean + 0.5, 1e-9) && slices.cv_rmse < 1e-9, "{slices:?}");
    let weibull = found.results.iter().find(|result| result.method == McpMethod::WeibullFit).unwrap();
    assert!(close(weibull.long_term_mean, 1.5 * reference_mean + 0.5, 0.02), "{}", weibull.long_term_mean);
    let ratio = &found.results[3];
    assert_eq!(ratio.method, McpMethod::SpeedRatio);
    assert!(close(ratio.predictor.line().unwrap().slope, found.site_mean / found.reference_concurrent_mean, 1e-12) && ratio.cv_rmse > 1e-3, "{ratio:?}");
    assert_ne!(found.recommended, McpMethod::SpeedRatio, "a line with an intercept fits better");
    let daily = mcp::mcp(&averaged(&site, DAY), &averaged(&reference, DAY), None, DAY, false, &mcp::McpOptions::default()).unwrap();
    assert_eq!(daily.concurrent, site_hours / 24);
    let too_short = mcp::average(&reference, &view(&reference), 0, TEN_MINUTES).unwrap_err();
    assert_eq!(too_short, CoreError::AveragingShorterThanStep { period_s: TEN_MINUTES, step_s: HOUR });
    let series = mcp::synthesize(&averaged(&reference, HOUR), None, HOUR, &found.results[0], None, &mcp::McpOptions::default());
    assert_eq!((series.len(), series[0].0), (reference_hours, START_2023));
}

#[test]
fn mcp_by_sector_follows_each_sectors_relation_and_turns_the_direction() {
    use wind_core::mcp::{self, Directions, McpMethod};
    const HOUR: i64 = 3_600;
    const OFFSET_DEG: f64 = 20.0;
    // Reference hourly over four months; its wind blows from the north on even
    // hours and from the south on odd ones. The site reads 1.2 · reference
    // from the north and 1.8 · reference from the south, its vane turned 20°.
    let hours = (31 + 28 + 31 + 30) * 24;
    let speed = |hour: usize| 5.0 + (hour % 13) as f64 * 0.7;
    let direction = |hour: usize| if hour % 2 == 0 { 0.0 } else { 180.0 };
    let factor = |hour: usize| if hour % 2 == 0 { 1.2 } else { 1.8 };
    let reference = make_dataset(
        START_2023,
        HOUR,
        vec![column("ws", ColumnType::WindSpeed, Some(50.0), (0..hours).map(speed).collect()), column("dir", ColumnType::WindDirection, Some(50.0), (0..hours).map(direction).collect())],
    );
    let site = make_dataset(
        START_2023,
        HOUR,
        vec![
            column("ws", ColumnType::WindSpeed, Some(80.0), (0..hours).map(|hour| factor(hour) * speed(hour)).collect()),
            column("dir", ColumnType::WindDirection, Some(78.0), (0..hours).map(|hour| direction(hour) + OFFSET_DEG).collect()),
        ],
    );
    let averaged = |dataset: &Dataset, column| mcp::average(dataset, &view(dataset), column, HOUR).unwrap();
    let (reference_direction, site_direction) = (averaged(&reference, 1), averaged(&site, 1));
    let sectored = Directions { reference: &reference_direction, site: Some(&site_direction), sectors: 2 };
    let found = mcp::mcp(&averaged(&site, 0), &averaged(&reference, 0), Some(sectored), HOUR, false, &mcp::McpOptions::default()).unwrap();
    let least_squares = found.results.iter().find(|result| result.method == McpMethod::LeastSquares).unwrap();
    let slopes: Vec<f64> = least_squares.division_predictors.iter().map(|fit| fit.as_ref().unwrap().line().unwrap().slope).collect();
    assert!(close(slopes[0], 1.2, 1e-9) && close(slopes[1], 1.8, 1e-9), "north and south: {slopes:?}");
    assert!(least_squares.cv_rmse < 1e-9, "each sector's own line predicts it exactly");
    let truth = (0..hours).map(|hour| factor(hour) * speed(hour)).sum::<f64>() / hours as f64;
    assert!(close(least_squares.long_term_mean, truth, 1e-9));
    let one_line = mcp::mcp(&averaged(&site, 0), &averaged(&reference, 0), None, HOUR, false, &mcp::McpOptions::default()).unwrap();
    assert!(one_line.results[0].cv_rmse > 1e-3 || one_line.results[0].spread_ratio < 0.99, "a single line cannot follow both sectors");
    let turned = found.direction.expect("both directions");
    assert!(close(turned.offset_deg, OFFSET_DEG, 1e-9), "{}", turned.offset_deg);
    // 16 sectors of 22.5° centred on north: 20° falls in NNE (sector 1), 200° in SSW (sector 9).
    assert!(close(turned.site_frequency[1], 0.5, 1e-12) && close(turned.long_term_frequency[9], 0.5, 1e-12), "{turned:?}");
    let refused = mcp::mcp(&averaged(&site, 0), &averaged(&reference, 0), Some(Directions { sectors: 0, ..sectored }), HOUR, false, &mcp::McpOptions::default()).unwrap_err();
    assert!(matches!(refused, CoreError::SectorsOutOfRange { .. }));
}

#[test]
fn displacement_height_moves_the_shear_laws_to_effective_heights() {
    use wind_core::displacement::Displacement;
    use wind_core::model::Extrapolation;
    const HOUR: i64 = 3_600;
    const ALPHA: f64 = 0.2;
    const DISPLACEMENT_M: f64 = 10.0;
    // U = base · ((z − 10) / 10)^0.2: the base at 20 m, base · 3^0.2 at 40 m.
    let steps = 7 * 24;
    let base: Vec<f64> = (0..steps).map(|step| 5.0 + (step % 5) as f64).collect();
    let at = |height: f64| -> Vec<f64> { base.iter().map(|speed| speed * ((height - DISPLACEMENT_M) / DISPLACEMENT_M).powf(ALPHA)).collect() };
    let speeds = vec![column("ws20", ColumnType::WindSpeed, Some(20.0), at(20.0)), column("ws40", ColumnType::WindSpeed, Some(40.0), at(40.0))];
    let mut dataset = make_dataset(START_2023, HOUR, speeds);
    let shear = |dataset: &Dataset| analysis::shear(dataset, &view(dataset), &[0, 1], None, 12, analysis::ShearAggregation::ApMean);

    let nominal = shear(&dataset).unwrap();
    assert!(close(nominal.alpha, ALPHA * 3f64.ln() / 2f64.ln(), 1e-12), "on the nominal heights the exponent looks steeper");
    assert_eq!(nominal.displacement_m, 0.0);
    dataset.displacement = Some(Displacement::Constant(DISPLACEMENT_M));
    let effective = shear(&dataset).unwrap();
    assert!(close(effective.alpha, ALPHA, 1e-12) && close(effective.displacement_m, DISPLACEMENT_M, 1e-12), "{effective:?}");
    assert!(effective.alpha_by_hour.iter().all(|alpha| close(*alpha, ALPHA, 1e-12)));
    dataset.displacement = Some(Displacement::Monthly(vec![DISPLACEMENT_M; 12]));
    assert!(close(shear(&dataset).unwrap().alpha, ALPHA, 1e-12), "the same month by month");
    dataset.displacement = Some(Displacement::Constant(25.0));
    assert_eq!(shear(&dataset).unwrap_err(), CoreError::DisplacementHeight { height_m: 25.0 }, "above the 20 m sensor");

    // Extrapolated to 60 m from 40 m, the nearer: × (50 / 30)^0.2, exactly the law at 60 m.
    dataset.displacement = Some(Displacement::Constant(DISPLACEMENT_M));
    dataset.extrapolations = vec![Extrapolation { lower: "ws20".into(), upper: "ws40".into(), height_m: 60.0, table_only: false, ..Extrapolation::default() }];
    derived::append(&mut dataset);
    let hub = dataset.columns.iter().position(|column| column.id == "Vel 60 m extrapolada").unwrap();
    assert!(dataset.columns[hub].values.iter().zip(at(60.0)).all(|(value, want)| close(*value, want, 1e-12)));
}

#[test]
fn vertical_extrapolation_hand_checked_with_its_table_fallback() {
    use wind_core::model::Extrapolation;
    const HOUR: i64 = 3_600;
    const ALPHA: f64 = 0.2;
    const STEEP: f64 = 0.35;
    let steps = 31 * 24;
    // α = 0.2 almost everywhere; step 5 is steep (0.35) but fast enough to keep
    // its own exponent; step 6 is too slow below (3 m/s) and takes the table's;
    // step 7 lacks the lower speed, step 8 the upper one.
    let lower: Vec<f64> = (0..steps).map(|step| match step { 6 => 3.0, 7 => f64::NAN, _ => 6.0 }).collect();
    let upper: Vec<f64> = (0..steps)
        .map(|step| match step { 5 => 6.0 * 2f64.powf(STEEP), 6 => 3.0 * 2f64.powf(STEEP), 8 => f64::NAN, _ => 6.0 * 2f64.powf(ALPHA) })
        .collect();
    let mut dataset = make_dataset(START_2023, HOUR, vec![column("ws40", ColumnType::WindSpeed, Some(40.0), lower), column("ws80", ColumnType::WindSpeed, Some(80.0), upper.clone())]);
    let spec = |table_only| Extrapolation { lower: "ws40".into(), upper: "ws80".into(), height_m: 120.0, table_only, ..Extrapolation::default() };
    dataset.extrapolations = vec![spec(false), Extrapolation { lower: "ws40".into(), upper: "ws40".into(), height_m: 100.0, table_only: false, ..Extrapolation::default() }];
    let mut table_only = dataset.clone();
    table_only.extrapolations = vec![spec(true)];
    derived::append(&mut dataset);
    derived::append(&mut table_only);
    let hub = dataset.columns.iter().position(|column| column.id == "Vel 120 m extrapolada").expect("the valid extrapolation");
    assert_eq!(dataset.columns.iter().filter(|column| column.id.ends_with("extrapolada")).count(), 1, "the same column twice is no shear");
    assert!(dataset.columns.iter().any(|column| column.id == "Vel 120 m extrapolada WPD"), "a power density of its own");
    // Carried from 80 m, the nearer height: × 1.5^α.
    let values = &dataset.columns[hub].values;
    let carried = |step: usize, alpha: f64| upper[step] * 1.5f64.powf(alpha);
    assert!(close(values[0], carried(0, ALPHA), 1e-12));
    assert!(close(values[5], carried(5, STEEP), 1e-12), "its own exponent");
    assert!(close(values[6], carried(6, ALPHA), 1e-12), "too slow: the hour's median, 0.2");
    assert!(close(values[7], carried(7, ALPHA), 1e-12), "no lower speed: the table");
    assert!(values[8].is_nan(), "nothing to carry");
    let hub_table = table_only.columns.iter().position(|column| column.id == "Vel 120 m extrapolada").unwrap();
    assert!(close(table_only.columns[hub_table].values[5], carried(5, ALPHA), 1e-12), "the table only, even where the step has its own");
    // The column inherits the flags of both speeds.
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 0..3).unwrap();
    assert_eq!(view(&dataset).excluded(hub), &[0..3]);
}

#[test]
fn reconstruction_fills_gaps_flags_them_and_undoes() {
    use wind_core::edits::{Edit, Reconstruction, RECONSTRUCTED_FLAG};
    const HOUR: i64 = 3_600;
    let steps = 31 * 24;
    let upper_speed = 6.0 * 2f64.powf(0.2);
    // A cycle 5, 6, 7, 8: each 1 m/s bin has a single successor, so a Markov walk retraces it.
    let cycle: Vec<f64> = (0..steps).map(|step| 5.0 + (step % 4) as f64).collect();
    let mut upper = vec![upper_speed; steps];
    upper[10..13].fill(f64::NAN);
    let mut cycling = cycle.clone();
    cycling[21..24].fill(f64::NAN);
    let original = make_dataset(
        START_2023,
        HOUR,
        vec![
            column("ws40", ColumnType::WindSpeed, Some(40.0), vec![6.0; steps]),
            column("ws80", ColumnType::WindSpeed, Some(80.0), upper),
            column("cycle", ColumnType::WindSpeed, Some(60.0), cycling),
            column("dir", ColumnType::WindDirection, Some(78.0), vec![90.0; steps]),
        ],
    );
    let mut dataset = original.clone();
    let mut history = History::default();
    let from_height = Edit::Reconstruct { columns: vec![1], steps: 0..steps, method: Reconstruction::FromHeight { source: 0 } };
    history.apply(&mut dataset, Op::Edit(from_height)).unwrap();
    assert!(dataset.columns[1].values[10..13].iter().all(|&value| close(value, upper_speed, 1e-12)), "α = 0.2 everywhere");
    let flag = dataset.flags.iter().position(|flag| flag.name == RECONSTRUCTED_FLAG.0).expect("the flag was added");
    assert!(dataset.flags[flag].include, "reconstructed data is kept by default");
    assert_eq!(dataset.segments, [Segment { column: 1, flag, start: 10, end: 13 }]);
    history.apply(&mut dataset, Op::Edit(Edit::Reconstruct { columns: vec![2], steps: 0..steps, method: Reconstruction::Markov })).unwrap();
    assert_eq!(dataset.columns[2].values[..], cycle[..], "the chain retraces the cycle");
    history.undo(&mut dataset);
    history.undo(&mut dataset);
    assert!(dataset.columns[1].values[10..13].iter().all(|value| value.is_nan()) && dataset.segments.is_empty());
    let markov_direction = Edit::Reconstruct { columns: vec![3], steps: 0..steps, method: Reconstruction::Markov }.apply(&mut dataset);
    assert_eq!(markov_direction.unwrap_err(), CoreError::MarkovNeedsLinearColumn { column: "dir".into() });
    let same_column = Edit::Reconstruct { columns: vec![1], steps: 0..steps, method: Reconstruction::FromHeight { source: 1 } }.apply(&mut dataset);
    assert_eq!(same_column.unwrap_err(), CoreError::ShearNeedsTwoHeights);
}

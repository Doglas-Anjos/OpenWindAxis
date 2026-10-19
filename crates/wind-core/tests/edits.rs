//! Value edits (scale and offset, delete, time shifts): hand-checked values,
//! the derived columns that follow, undo, refusals and the replay on a
//! re-import. The CSV text and expected numbers are fixtures.
use std::sync::Arc;
use wind_core::derived::CELSIUS_ZERO_K;
use wind_core::edits::{self, Edit};
use wind_core::error::CoreError;
use wind_core::history::History;
use wind_core::import::{import_text, ImportConfig};
use wind_core::model::{Dataset, TimeStampMeaning};
use wind_core::ops::Op;

/// Six ten-minute steps: speed and its SD at 80 m, direction, temperature.
const CSV: &str = "Data/Hora;Vel 80m Média;Vel 80m DP;Dir 78m;Temp 2m\n\
                   01/01/2023 00:00;5,0;0,5;300;20\n\
                   01/01/2023 00:10;6,0;0,6;350;21\n\
                   01/01/2023 00:20;7,0;0,7;10;22\n\
                   01/01/2023 00:30;8,0;0,8;90;23\n\
                   01/01/2023 00:40;9,0;0,9;180;24\n\
                   01/01/2023 00:50;10,0;1,0;270;25\n";
const SPEED: usize = 0;
const DEVIATION: usize = 1;
const DIRECTION: usize = 2;
const TEMPERATURE: usize = 3;
const STEPS: usize = 6;
/// A site elevation, so the air density column exists without a pressure column.
const SEA_LEVEL_M: f64 = 0.0;
const TOLERANCE: f64 = 1e-12;
/// Seconds in one hour and in one step of the fixture.
const HOUR_S: i64 = 3600;
const STEP_S: i64 = 600;

fn imported(meaning: TimeStampMeaning) -> Dataset {
    let config = ImportConfig { elevation_m: Some(SEA_LEVEL_M), timestamp_meaning: meaning, ..ImportConfig::default() };
    let (mut dataset, _) = import_text("torre", CSV, &config).unwrap();
    wind_core::derived::append(&mut dataset);
    dataset
}

fn dataset() -> Dataset {
    imported(TimeStampMeaning::Start)
}

fn column_named(dataset: &Dataset, id: &str) -> usize {
    dataset.columns.iter().position(|column| column.id == id).unwrap_or_else(|| panic!("no column {id}"))
}

fn assert_close(found: &[f64], want: &[f64]) {
    assert_eq!(found.len(), want.len());
    for (step, (&found, &want)) in found.iter().zip(want).enumerate() {
        let same = (found.is_nan() && want.is_nan()) || (found - want).abs() <= TOLERANCE;
        assert!(same, "step {step}: {found} vs {want}; {found:?} vs {want:?}");
    }
}

#[test]
fn scale_and_offset_reach_only_the_chosen_steps_and_derived_columns_follow() {
    let mut dataset = dataset();
    let (scale, offset) = (1.1, 0.2);
    let ti = column_named(&dataset, "Vel 80m Média TI");
    let density = column_named(&dataset, "Densidade do ar");
    let density_before = Arc::clone(&dataset.columns[density].values);
    Edit::ScaleOffset { columns: vec![SPEED], steps: 1..3, scale, offset }.apply(&mut dataset).unwrap();
    let speeds = [5.0, 6.0 * scale + offset, 7.0 * scale + offset, 8.0, 9.0, 10.0];
    assert_close(&dataset.columns[SPEED].values, &speeds);
    assert_close(&dataset.columns[DEVIATION].values, &[0.5, 0.6, 0.7, 0.8, 0.9, 1.0]);
    let deviations = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
    let want_ti: Vec<f64> = speeds.iter().zip(deviations).map(|(speed, deviation)| deviation / speed).collect();
    assert_close(&dataset.columns[ti].values, &want_ti);
    assert!(Arc::ptr_eq(&dataset.columns[density].values, &density_before), "the density depends on no speed");
    assert_eq!(dataset.edits.len(), 1, "logged");
}

#[test]
fn a_column_listed_twice_is_edited_once() {
    let mut dataset = dataset();
    let doubled = Edit::ScaleOffset { columns: vec![SPEED, SPEED], steps: 0..STEPS, scale: 2.0, offset: 0.0 };
    doubled.apply(&mut dataset).unwrap();
    assert_close(&dataset.columns[SPEED].values, &[10.0, 12.0, 14.0, 16.0, 18.0, 20.0]);
    // The log keeps the edit as given, and its replay is the same single edit.
    let mut replayed = self::dataset();
    doubled.apply(&mut replayed).unwrap();
    assert_close(&replayed.columns[SPEED].values, &dataset.columns[SPEED].values);
}

#[test]
fn directions_wrap_around_and_temperatures_scale_in_kelvin() {
    let mut dataset = dataset();
    let quarter_turn = 90.0;
    Edit::ScaleOffset { columns: vec![DIRECTION], steps: 0..STEPS, scale: 1.0, offset: quarter_turn }.apply(&mut dataset).unwrap();
    // The reference product's example: 300° + 90° is 30°.
    assert_close(&dataset.columns[DIRECTION].values, &[30.0, 80.0, 100.0, 180.0, 270.0, 0.0]);

    let density = column_named(&dataset, "Densidade do ar");
    let density_before = dataset.columns[density].values.to_vec();
    let scale = 1.1;
    Edit::ScaleOffset { columns: vec![TEMPERATURE], steps: 0..1, scale, offset: 0.0 }.apply(&mut dataset).unwrap();
    let want = (20.0 + CELSIUS_ZERO_K) * scale - CELSIUS_ZERO_K;
    assert!((dataset.columns[TEMPERATURE].values[0] - want).abs() <= TOLERANCE, "{}", dataset.columns[TEMPERATURE].values[0]);
    let density_after = &dataset.columns[density].values;
    assert!(density_after[0] < density_before[0], "warmer air is lighter");
    assert_close(&density_after[1..], &density_before[1..]);
}

#[test]
fn delete_and_shift_move_values_as_the_reference_product_shows() {
    let mut deleted = dataset();
    Edit::Delete { columns: vec![DIRECTION], steps: 2..4 }.apply(&mut deleted).unwrap();
    assert_close(&deleted.columns[DIRECTION].values, &[300.0, 350.0, f64::NAN, f64::NAN, 180.0, 270.0]);

    let mut later = dataset();
    Edit::ShiftSteps { columns: vec![DIRECTION], steps: 1..3, by: 2 }.apply(&mut later).unwrap();
    assert_close(&later.columns[DIRECTION].values, &[300.0, f64::NAN, f64::NAN, 350.0, 10.0, 270.0]);

    let mut earlier = dataset();
    Edit::ShiftSteps { columns: vec![DIRECTION], steps: 0..2, by: -1 }.apply(&mut earlier).unwrap();
    assert_close(&earlier.columns[DIRECTION].values, &[350.0, f64::NAN, 10.0, 90.0, 180.0, 270.0]);
    assert_eq!(Edit::ShiftSteps { columns: vec![DIRECTION], steps: 0..2, by: -1 }.reach().1, 0..2);
    assert_eq!(Edit::ShiftSteps { columns: vec![DIRECTION], steps: 1..3, by: 2 }.reach().1, 1..5);
}

#[test]
fn undo_and_redo_swap_values_derived_columns_and_the_clock() {
    let mut dataset = dataset();
    let original = dataset.clone();
    let mut history = History::default();
    history.apply(&mut dataset, Op::Edit(Edit::ScaleOffset { columns: vec![SPEED], steps: 0..STEPS, scale: 2.0, offset: 0.0 })).unwrap();
    history.apply(&mut dataset, Op::Edit(Edit::ShiftStart { by_s: HOUR_S })).unwrap();
    assert_eq!(dataset.start, original.start + HOUR_S);
    let edited = dataset.clone();

    history.undo(&mut dataset);
    history.undo(&mut dataset);
    assert_eq!(dataset.start, original.start);
    assert!(dataset.edits.is_empty());
    for (column, before) in dataset.columns.iter().zip(&original.columns) {
        assert!(Arc::ptr_eq(&column.values, &before.values), "{}: the very values from before", column.id);
    }
    history.redo(&mut dataset);
    history.redo(&mut dataset);
    assert_eq!((dataset.start, &dataset.edits), (edited.start, &edited.edits));
    for (column, after) in dataset.columns.iter().zip(&edited.columns) {
        assert!(Arc::ptr_eq(&column.values, &after.values), "{}", column.id);
    }
}

#[test]
fn derived_columns_and_bad_numbers_are_refused_untouched() {
    let mut dataset = dataset();
    let before = dataset.clone();
    let ti = column_named(&dataset, "Vel 80m Média TI");
    let refused = Edit::Delete { columns: vec![SPEED, ti], steps: 0..STEPS }.apply(&mut dataset).unwrap_err();
    assert_eq!(refused, CoreError::DerivedColumnNotEditable { column: "Vel 80m Média TI".into() });
    let not_finite = Edit::ScaleOffset { columns: vec![SPEED], steps: 0..STEPS, scale: f64::NAN, offset: 0.0 };
    assert!(matches!(not_finite.apply(&mut dataset), Err(CoreError::ScaleOffsetNotFinite { .. })));
    let off_the_calendar = Edit::ShiftStart { by_s: i64::MAX };
    assert!(matches!(off_the_calendar.apply(&mut dataset), Err(CoreError::TimestampOutOfRange { .. })));
    assert!(dataset.edits.is_empty() && dataset.start == before.start);
    assert!(dataset.columns.iter().zip(&before.columns).all(|(column, before)| Arc::ptr_eq(&column.values, &before.values)));
}

#[test]
fn a_reimport_replays_the_edits_on_the_same_file_rows() {
    let mut old = dataset();
    Edit::ShiftStart { by_s: HOUR_S }.apply(&mut old).unwrap();
    Edit::Delete { columns: vec![DIRECTION], steps: 1..2 }.apply(&mut old).unwrap();
    // The same file read again with another timestamp meaning: every step starts one step earlier.
    let mut new = imported(TimeStampMeaning::End);
    let fresh_start = new.start;
    edits::carry_over(&old, &mut new).unwrap();
    assert_eq!(new.start, fresh_start + HOUR_S, "the shift replays");
    assert_eq!(new.start, old.start - STEP_S, "and the grid keeps its own meaning");
    assert_close(&new.columns[DIRECTION].values, &old.columns[DIRECTION].values);
    assert_eq!(new.edits, old.edits);
}

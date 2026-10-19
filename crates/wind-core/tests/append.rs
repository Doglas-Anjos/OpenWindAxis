//! Appending daily files (Arquivo › Anexar dados): the record grows over
//! every file, columns link by id, the master keeps its data where both have
//! some, and a file on another step or phase is refused alone.
use wind_core::append::{merge, AppendOutcome};
use wind_core::error::CoreError;
use wind_core::import::{import_text, ImportConfig};
use wind_core::model::Dataset;

fn file(text: &str) -> Dataset {
    import_text("day", text, &ImportConfig::default()).unwrap().0
}

fn values(dataset: &Dataset, id: &str) -> Vec<f64> {
    dataset.columns.iter().find(|column| column.id == id).unwrap_or_else(|| panic!("no {id}")).values.to_vec()
}

/// Same values, NaN equal to NaN.
fn same(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b || (a.is_nan() && b.is_nan()))
}

const NAN: f64 = f64::NAN;
const DAY_ONE: &str = "t,ws,dir\n2018-01-01 00:00,5,90\n2018-01-01 00:10,,95\n2018-01-01 00:20,7,100\n";

#[test]
fn days_join_in_time_order_whichever_comes_first() {
    let after = file("t,ws,dir\n2018-01-01 00:40,9,110\n2018-01-01 00:50,10,115\n");
    let before = file("t,ws,dir\n2017-12-31 23:40,3,80\n2017-12-31 23:50,4,85\n");
    let (merged, outcomes) = merge(file(DAY_ONE), vec![after, before]);
    assert_eq!(merged.len, 8, "23:40 to 00:50, the 00:30 gap kept");
    assert_eq!(merged.start, file(DAY_ONE).start - 2 * 600, "the record starts with the earlier file");
    assert!(same(&values(&merged, "ws"), &[3.0, 4.0, 5.0, NAN, 7.0, NAN, 9.0, 10.0]));
    assert!(outcomes.iter().all(|outcome| matches!(outcome, AppendOutcome::Appended { steps: 2, .. })));
}

#[test]
fn the_master_keeps_its_data_and_appended_files_fill_its_gaps() {
    // 00:10 is missing in the master; the appended file also has 00:00 and 00:20.
    let overlap = file("t,ws,dir\n2018-01-01 00:00,50,1\n2018-01-01 00:10,60,2\n2018-01-01 00:20,70,3\n");
    let later = file("t,ws,dir\n2018-01-01 00:10,99,9\n2018-01-01 00:20,99,9\n");
    let (merged, outcomes) = merge(file(DAY_ONE), vec![overlap, later]);
    assert!(same(&values(&merged, "ws"), &[5.0, 60.0, 7.0]), "master unless missing; the earlier appended file wins over the later");
    assert_eq!(outcomes[0], AppendOutcome::Appended { steps: 3, new_columns: vec![], linked: vec![], filled: 1, overlapped: 5 });
    assert_eq!(outcomes[1], AppendOutcome::Appended { steps: 2, new_columns: vec![], linked: vec![], filled: 0, overlapped: 4 });
}

#[test]
fn a_column_the_master_lacks_is_added_with_its_parent() {
    let wider = file("t,ws,ws80 Avg,ws80 SD\n2018-01-01 00:30,8,6,0.5\n2018-01-01 00:40,9,7,0.6\n");
    let (merged, outcomes) = merge(file(DAY_ONE), vec![wider]);
    let ids: Vec<&str> = merged.columns.iter().map(|column| column.id.as_str()).collect();
    assert_eq!(ids, ["ws", "dir", "ws80 Avg", "ws80 SD"]);
    assert!(same(&values(&merged, "ws80 Avg"), &[NAN, NAN, NAN, 6.0, 7.0]), "missing before the file that brought it");
    let parent = merged.columns[3].parent.map(|parent| merged.columns[parent].id.as_str());
    assert_eq!(parent, Some("ws80 Avg"), "the SD keeps its mean, found again by id");
    assert!(matches!(&outcomes[0], AppendOutcome::Appended { new_columns, .. } if new_columns == &["ws80 Avg", "ws80 SD"]));
}

#[test]
fn a_sensor_renamed_in_the_logger_keeps_its_column_when_one_fits() {
    let master = file("t,Pres 97m Barómetro,Vel 100m A,Vel 100m B\n2018-06-14 23:40,1002,5,5.1\n2018-06-14 23:50,1003,6,6.1\n");
    // The barometer renamed; both 100 m anemometers renamed: two candidates each, so they stay apart.
    let renamed = file("t,Pres 97m Barometer,Vel 100m 1,Vel 100m 2\n2018-06-15 00:00,1004,7,7.1\n2018-06-15 00:10,1005,8,8.1\n");
    let (merged, outcomes) = merge(master, vec![renamed]);
    assert!(same(&values(&merged, "Pres 97m Barómetro"), &[1002.0, 1003.0, 1004.0, 1005.0]), "one pressure sensor at 97 m: linked");
    let AppendOutcome::Appended { linked, new_columns, .. } = &outcomes[0] else { panic!("appended") };
    assert_eq!(linked, &[("Pres 97m Barometer".to_string(), "Pres 97m Barómetro".to_string())]);
    assert_eq!(new_columns, &["Vel 100m 1", "Vel 100m 2"], "two anemometers fit each: no guess");
}

#[test]
fn a_file_on_another_step_or_phase_is_refused_alone() {
    let hourly = file("t,ws,dir\n2018-01-02 00:00,5,90\n2018-01-02 01:00,6,91\n");
    let shifted = file("t,ws,dir\n2018-01-01 00:35,5,90\n2018-01-01 00:45,6,91\n");
    let (merged, outcomes) = merge(file(DAY_ONE), vec![hourly, shifted, file("t,ws,dir\n2018-01-01 00:30,8,120\n2018-01-01 00:40,9,121\n")]);
    assert!(matches!(outcomes[0], AppendOutcome::Refused(CoreError::AppendStepMismatch { master_s: 600, appended_s: 3600 })));
    assert!(matches!(outcomes[1], AppendOutcome::Refused(CoreError::AppendOutOfPhase { offset_s: 300 })));
    assert!(matches!(outcomes[2], AppendOutcome::Appended { steps: 2, .. }));
    assert_eq!(merged.len, 5);
}

#[test]
fn a_record_too_sparse_to_hold_is_refused() {
    let years_later = file("t,ws,dir\n2030-01-01 00:00,5,90\n2030-01-01 00:10,6,91\n");
    let (merged, outcomes) = merge(file(DAY_ONE), vec![years_later]);
    assert!(matches!(outcomes[0], AppendOutcome::Refused(CoreError::TimestampsSpanTooMany { .. })));
    assert_eq!(merged.len, 3, "the master stays as it was");
}

//! Tower shading: the ratio pattern of an anemometer pair (detection, as the
//! original computes it) and the direction-in-sector rule that flags it.
use wind_core::filter::FilterView;
use wind_core::flags;
use wind_core::model::{ColumnType, DataColumn, Dataset, Segment, Subtype, TimeStampMeaning};
use wind_core::rules::{self, Criterion, FlagRule, RuleRun};
use wind_core::shading::{detect, MIN_SECTOR_RATIOS, SHADING_SECTORS};

/// 2023-01-01T00:00Z.
const START_2023: i64 = 1_672_531_200;
const TEN_MINUTES: i64 = 600;
/// Default flags (`flags::DEFAULT_FLAGS`).
const TOWER_SHADOW_FLAG: usize = 1;
const ONE_STEP: usize = 1;
/// Turns of the test tower's vane, one step per degree.
const TURNS: usize = 30;
const DEGREES_PER_TURN: usize = 360;
/// The shaded sensor reads this share of the other.
const SHADED_SHARE: f64 = 0.8;
const FREE_SPEED: f64 = 8.0;

fn column(id: &str, kind: ColumnType, values: Vec<f64>) -> DataColumn {
    DataColumn {
        id: id.into(),
        kind,
        subtype: Subtype::Mean,
        units: if kind == ColumnType::WindDirection { "°".into() } else { "m/s".into() },
        height_m: Some(30.0),
        parent: None,
        values: values.into(),
        derived: None,
    }
}

fn make_dataset(columns: Vec<DataColumn>) -> Dataset {
    Dataset {
        name: "shading".into(),
        start: START_2023,
        step_s: TEN_MINUTES,
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

/// The vane turning one degree a step, at the middle of each degree.
fn directions() -> Vec<f64> {
    (0..TURNS * DEGREES_PER_TURN).map(|step| (step % DEGREES_PER_TURN) as f64 + 0.5).collect()
}

/// `FREE_SPEED`, shaded to `SHADED_SHARE` while the direction is in `from..to`.
fn speeds(directions: &[f64], from: f64, to: f64) -> Vec<f64> {
    directions.iter().map(|degrees| if (from..to).contains(degrees) { FREE_SPEED * SHADED_SHARE } else { FREE_SPEED }).collect()
}

/// B shaded 130–150°, A shaded 300–320° (A/B columns 0 and 1, direction 2).
fn tower_with(edit_a: impl Fn(usize, f64) -> f64, edit_b: impl Fn(usize, f64) -> f64) -> Dataset {
    let directions = directions();
    let speeds_a = speeds(&directions, 300.0, 320.0).into_iter().enumerate().map(|(step, speed)| edit_a(step, speed)).collect();
    let speeds_b = speeds(&directions, 130.0, 150.0).into_iter().enumerate().map(|(step, speed)| edit_b(step, speed)).collect();
    make_dataset(vec![
        column("A", ColumnType::WindSpeed, speeds_a),
        column("B", ColumnType::WindSpeed, speeds_b),
        column("Dir", ColumnType::WindDirection, directions),
    ])
}

fn tower() -> Dataset {
    tower_with(|_, speed| speed, |_, speed| speed)
}

#[test]
fn the_lowest_and_highest_median_sectors_are_the_shaded_centres() {
    let dataset = tower();
    let pattern = detect(&dataset, &FilterView::new(&dataset), 0, 1, 2).unwrap();
    assert_eq!(pattern.medians.len(), SHADING_SECTORS);
    // 300–320° covers the sectors centred on 300 (half), 305, 310, 315 and 320 (half): ties go to the first.
    assert_eq!(pattern.shaded_a, Some(305.0), "300° holds 297.5–302.5°, only half shaded");
    assert_eq!(pattern.shaded_b, Some(135.0));
    assert!((pattern.medians[0] - 1.0).abs() < 1e-12, "north is free");
    assert!((pattern.medians[62] - SHADED_SHARE).abs() < 1e-12, "310°: A reads 80 % of B");
}

#[test]
fn sectors_with_twenty_ratios_or_fewer_do_not_compete() {
    // A reads very low at 5° in the first 20 turns only (sector 1 holds 2.5–7.5°: five steps a turn).
    let first_turns = 4 * DEGREES_PER_TURN;
    let dataset = tower_with(
        |step, speed| {
            let degrees = (step % DEGREES_PER_TURN) as f64 + 0.5;
            match (step < first_turns, (2.5..7.5).contains(&degrees)) {
                (true, true) => 0.1,
                (false, true) => f64::NAN,
                _ => speed,
            }
        },
        |_, speed| speed,
    );
    let pattern = detect(&dataset, &FilterView::new(&dataset), 0, 1, 2).unwrap();
    assert_eq!(pattern.counts[1], MIN_SECTOR_RATIOS - 1);
    assert_eq!(pattern.shaded_a, Some(305.0), "the 5° sector has only 20 ratios");
}

#[test]
fn a_zero_reference_speed_is_skipped_and_unknown_columns_refused() {
    let dataset = tower_with(|_, speed| speed, |step, speed| if step == 0 { 0.0 } else { speed });
    let pattern = detect(&dataset, &FilterView::new(&dataset), 0, 1, 2).unwrap();
    assert_eq!(pattern.counts[0], TURNS * 5 - 1, "sector 0 holds 357.5–2.5°, five directions a turn, one B = 0");
    assert!(detect(&dataset, &FilterView::new(&dataset), 0, 1, 9).is_err());
}

#[test]
fn the_shading_flag_stays_visible_to_detection() {
    let mut dataset = tower();
    // Flag A's whole shaded sector as tower shadow: default filtering hides it, the keeping view does not.
    let steps: Vec<usize> = (0..dataset.len).filter(|&step| (300.0..320.0).contains(&dataset.columns[2].values[step])).collect();
    dataset.segments = steps.iter().map(|&step| Segment { column: 0, flag: TOWER_SHADOW_FLAG, start: step, end: step + 1 }).collect();
    flags::normalize(&mut dataset.segments);
    let hidden = detect(&dataset, &FilterView::new(&dataset), 0, 1, 2).unwrap();
    assert_ne!(hidden.shaded_a, Some(305.0), "flagged, the shade is gone from default filtering");
    let kept = detect(&dataset, &FilterView::keeping(&dataset, TOWER_SHADOW_FLAG), 0, 1, 2).unwrap();
    assert_eq!(kept.shaded_a, Some(305.0));
}

fn sector_rule(center: f64, width: f64, columns: &[usize]) -> FlagRule {
    FlagRule { criterion: Criterion::DirectionInSector { direction: 2, center, width }, min_steps: ONE_STEP, columns: columns.to_vec(), flag: TOWER_SHADOW_FLAG }
}

#[test]
fn a_sector_rule_flags_each_pass_of_the_vane_through_the_sector() {
    let dataset = tower();
    let run = RuleRun::new(vec![sector_rule(310.0, 30.0, &[0])], 0..dataset.len).unwrap();
    let findings = rules::search(&dataset, &FilterView::new(&dataset), &run).unwrap();
    assert_eq!(findings.len(), TURNS, "one stretch a turn: separate passes never merge");
    let first = findings[0].evidence;
    // [295°, 325°): directions 295.5 … 324.5, thirty steps.
    assert_eq!((first.start, first.end), (295, 325));
    assert_eq!(first.statistic, 309.5, "the direction nearest the centre (309.5 and 310.5 tie; the first stays)");
}

#[test]
fn a_sector_through_north_wraps_and_is_half_open() {
    let dataset = tower();
    let wrapped = Criterion::DirectionInSector { direction: 2, center: 0.0, width: 10.0 };
    assert!(wrapped.in_sector(355.0) && wrapped.in_sector(359.9) && wrapped.in_sector(4.9));
    assert!(!wrapped.in_sector(5.0), "the upper edge is outside, like a bin's");
    assert!(!wrapped.in_sector(f64::NAN));
    let run = RuleRun::new(vec![sector_rule(0.0, 10.0, &[1])], 0..dataset.len).unwrap();
    let findings = rules::search(&dataset, &FilterView::new(&dataset), &run).unwrap();
    // The first turn's 0.5 … 4.5° stands alone; each later one joins the previous turn's 355.5 … 359.5°.
    assert_eq!(findings.len(), TURNS + 1);
    assert_eq!((findings[0].evidence.start, findings[0].evidence.end), (0, 5));
    assert_eq!((findings[1].evidence.start, findings[1].evidence.end), (355, 365));
}

#[test]
fn a_sector_rule_refuses_an_empty_or_overfull_width_and_an_unknown_direction() {
    for width in [0.0, -5.0, 361.0, f64::NAN] {
        assert!(Criterion::DirectionInSector { direction: 2, center: 0.0, width }.check(ONE_STEP).is_err(), "width {width}");
    }
    assert!(Criterion::DirectionInSector { direction: 2, center: f64::INFINITY, width: 30.0 }.check(ONE_STEP).is_err());
    let dataset = tower();
    let unknown = RuleRun::new(vec![FlagRule { criterion: Criterion::DirectionInSector { direction: 7, center: 0.0, width: 30.0 }, ..sector_rule(0.0, 30.0, &[0]) }], 0..dataset.len).unwrap();
    assert!(rules::search(&dataset, &FilterView::new(&dataset), &unknown).is_err());
}

#[test]
fn applied_sector_rules_flag_the_shaded_steps_and_keep_their_evidence() {
    let mut dataset = tower();
    let run = RuleRun::new(vec![sector_rule(305.0, 30.0, &[0]), sector_rule(135.0, 30.0, &[1])], 0..dataset.len).unwrap();
    let findings = rules::apply(&mut dataset, &run).unwrap();
    assert_eq!(findings.len(), 2 * TURNS);
    let flagged_a: usize = dataset.segments.iter().filter(|segment| segment.column == 0).map(|segment| segment.end - segment.start).sum();
    assert_eq!(flagged_a, TURNS * 30);
    assert_eq!(dataset.evidence.len(), 2 * TURNS);
    let again = rules::apply(&mut dataset, &run).unwrap();
    assert!(again.is_empty(), "flagged data is excluded, so a second run finds nothing");
    assert_eq!(dataset.evidence.len(), 2 * TURNS, "and the evidence stays");
}

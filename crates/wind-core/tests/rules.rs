//! Flag rules (ROTEIRO item 8): each test on hand-checked series, the trip
//! through the undo history, the evidence invariants, a property test
//! against a step-by-step reference, and a timing probe on 30 columns × 1 M
//! steps (`cargo test --release -p wind-core --test rules -- --ignored
//! --nocapture`). Flag statistics and removal by flag (item 9) at the end.
use std::ops::Range;
use std::time::Instant;
use wind_core::error::CoreError;
use wind_core::filter::FilterView;
use wind_core::flags;
use wind_core::history::History;
use wind_core::import::{import_text, ImportConfig};
use wind_core::model::{ColumnType, DataColumn, Dataset, Flag, Segment, Subtype, TimeStampMeaning};
use wind_core::ops::Op;
use wind_core::rules::{self, Criterion, Evidence, FlagRule, Finding, RuleRun, MAX_RULES, MAX_RULE_HITS};
use wind_core::series::DayStats;

/// 2023-01-01T00:00Z.
const START_2023: i64 = 1_672_531_200;
/// Ten-minute steps.
const TEN_MINUTES: i64 = 600;
/// Default flags (`flags::DEFAULT_FLAGS`): all excluded by default.
const INVALID_FLAG: usize = 0;
const TOWER_SHADOW_FLAG: usize = 1;
const ICING_FLAG: usize = 2;
/// A stretch of one step is the default duration.
const ONE_STEP: usize = 1;
const MISSING: f64 = f64::NAN;

fn column(id: &str, values: Vec<f64>) -> DataColumn {
    DataColumn {
        id: id.into(),
        kind: ColumnType::WindSpeed,
        subtype: Subtype::Mean,
        units: "m/s".into(),
        height_m: None,
        parent: None,
        values: values.into(),
        derived: None,
    }
}

fn make_dataset(columns: Vec<DataColumn>) -> Dataset {
    Dataset {
        name: "rules".into(),
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

fn rule(criterion: Criterion, min_steps: usize, columns: &[usize], flag: usize) -> FlagRule {
    FlagRule { criterion, min_steps, columns: columns.to_vec(), flag }
}

/// A run of `rules` over the whole record of `dataset`.
fn whole_record(dataset: &Dataset, rules: Vec<FlagRule>) -> RuleRun {
    RuleRun::new(rules, 0..dataset.len).unwrap()
}

/// Runs `rules` over the whole record of `dataset`, without changing it.
fn search(dataset: &Dataset, rules: Vec<FlagRule>) -> Vec<Finding> {
    let run = RuleRun::new(rules, 0..dataset.len).unwrap();
    rules::search(dataset, &FilterView::new(dataset), &run).unwrap()
}

/// (start, end, statistic, statistic step) of each finding, in order found.
fn stretches(findings: &[Finding]) -> Vec<(usize, usize, f64, usize)> {
    findings.iter().map(|finding| {
        let evidence = &finding.evidence;
        (evidence.start, evidence.end, evidence.statistic, evidence.statistic_step)
    }).collect()
}

// ---- Each test on a hand-checked series ----

#[test]
fn range_is_half_open_and_keeps_the_value_furthest_outside() {
    let speeds = vec![5.0, 60.0, 70.0, 5.0, -1.0, 50.0, 49.9];
    let dataset = make_dataset(vec![column("ws", speeds)]);
    let range = Criterion::OutsideRange { min: 0.0, max: 50.0 };
    let found = search(&dataset, vec![rule(range, ONE_STEP, &[0], INVALID_FLAG)]);
    // 50 is outside [0, 50); −1 is 1 below, 50 is 0 above: −1 is the evidence.
    assert_eq!(stretches(&found), [(1, 3, 70.0, 2), (4, 6, -1.0, 4)]);
    let only_long = search(&dataset, vec![rule(range, 2, &[0], INVALID_FLAG)]);
    assert_eq!(only_long.len(), 2, "both stretches last two steps");
    let longer = search(&dataset, vec![rule(range, 3, &[0], INVALID_FLAG)]);
    assert!(longer.is_empty(), "no stretch lasts three steps");
    let open_above = Criterion::OutsideRange { min: 0.0, max: f64::INFINITY };
    assert_eq!(stretches(&search(&dataset, vec![rule(open_above, ONE_STEP, &[0], INVALID_FLAG)])), [(4, 5, -1.0, 4)]);
}

#[test]
fn a_change_flags_the_later_step_and_never_spans_a_gap() {
    let speeds = vec![1.0, 2.0, 20.0, 21.0, MISSING, 50.0, 51.0, 40.0];
    let dataset = make_dataset(vec![column("ws", speeds)]);
    let found = search(&dataset, vec![rule(Criterion::ChangeAbove { limit: 5.0 }, ONE_STEP, &[0], INVALID_FLAG)]);
    // 2 → 20 fires at step 2; 21 → gap → 50 has no change; 51 → 40 fires, signed.
    assert_eq!(stretches(&found), [(2, 3, 18.0, 2), (7, 8, -11.0, 7)]);
}

#[test]
fn stuck_counts_the_steps_holding_the_value() {
    let speeds = vec![1.0, 3.0, 3.0, 3.0, 3.0, 4.0, 4.0, MISSING, 4.0, 4.0, 4.05];
    let dataset = make_dataset(vec![column("ws", speeds)]);
    let exact = search(&dataset, vec![rule(Criterion::Stuck { tolerance: 0.0 }, 3, &[0], ICING_FLAG)]);
    // 3,3,3,3 holds 4 steps; 4,4 only 2; after the gap 4,4 then 4.05 moves.
    assert_eq!(stretches(&exact), [(1, 5, 4.0, 1)]);
    let loose = search(&dataset, vec![rule(Criterion::Stuck { tolerance: 0.1 }, 3, &[0], ICING_FLAG)]);
    assert_eq!(stretches(&loose), [(1, 5, 4.0, 1), (8, 11, 3.0, 8)]);
}

#[test]
fn data_excluded_by_default_filtering_is_not_searched() {
    let speeds = vec![5.0, 90.0, 90.0, 5.0, 90.0, 5.0];
    let mut dataset = make_dataset(vec![column("ws", speeds)]);
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 1..3).unwrap();
    let range = Criterion::OutsideRange { min: 0.0, max: 50.0 };
    let found = search(&dataset, vec![rule(range, ONE_STEP, &[0], ICING_FLAG)]);
    assert_eq!(stretches(&found), [(4, 5, 90.0, 4)], "the excluded 90s do not fire");
    // Step 3 follows an excluded step: it has no change to test. Steps 4 and
    // 5 both fire, so they are one stretch; of equal changes the first is kept.
    let change = search(&dataset, vec![rule(Criterion::ChangeAbove { limit: 50.0 }, ONE_STEP, &[0], ICING_FLAG)]);
    assert_eq!(stretches(&change), [(4, 6, 85.0, 4)]);
}

#[test]
fn bad_rules_are_refused_before_anything_runs() {
    let refused = |criterion, min_steps| RuleRun::new(vec![rule(criterion, min_steps, &[0], INVALID_FLAG)], 0..10).unwrap_err();
    assert_eq!(refused(Criterion::OutsideRange { min: 5.0, max: 5.0 }, ONE_STEP), CoreError::RuleRangeInvalid { min: 5.0, max: 5.0 });
    assert!(matches!(refused(Criterion::OutsideRange { min: f64::NAN, max: 5.0 }, ONE_STEP), CoreError::RuleRangeInvalid { .. }));
    assert!(matches!(refused(Criterion::ChangeAbove { limit: -1.0 }, ONE_STEP), CoreError::RuleThresholdInvalid { .. }));
    assert!(matches!(refused(Criterion::Stuck { tolerance: f64::NAN }, 3), CoreError::RuleThresholdInvalid { .. }));
    assert_eq!(refused(Criterion::Stuck { tolerance: 0.0 }, ONE_STEP), CoreError::RuleMinStepsTooSmall { min_steps: 1, least: 2 });
    assert!(matches!(refused(Criterion::ChangeAbove { limit: 1.0 }, 0), CoreError::RuleMinStepsTooSmall { .. }));
    let too_many = vec![rule(Criterion::ChangeAbove { limit: 1.0 }, ONE_STEP, &[0], INVALID_FLAG); MAX_RULES + 1];
    assert!(matches!(RuleRun::new(too_many, 0..10).unwrap_err(), CoreError::TooManyRules { .. }));

    let dataset = make_dataset(vec![column("ws", vec![1.0; 10])]);
    let view = FilterView::new(&dataset);
    let indexed = |columns: &[usize], flag| RuleRun::new(vec![rule(Criterion::ChangeAbove { limit: 1.0 }, ONE_STEP, columns, flag)], 0..10).unwrap();
    assert_eq!(rules::search(&dataset, &view, &indexed(&[3], INVALID_FLAG)).unwrap_err(), CoreError::NoSuchColumn { column: 3 });
    assert_eq!(rules::search(&dataset, &view, &indexed(&[0], 9)).unwrap_err(), CoreError::NoSuchFlag { flag: 9 });
}

#[test]
fn a_rule_that_fires_everywhere_is_stopped_with_an_error() {
    // Alternating values: every other step is out of range.
    let alternating = (0..2 * (MAX_RULE_HITS + 1)).map(|step| (step % 2) as f64).collect();
    let mut dataset = make_dataset(vec![column("ws", alternating)]);
    let run = RuleRun::new(vec![rule(Criterion::OutsideRange { min: 0.0, max: 0.5 }, ONE_STEP, &[0], INVALID_FLAG)], 0..dataset.len).unwrap();
    let error = History::default().apply(&mut dataset, Op::FlagByRules(run)).unwrap_err();
    assert_eq!(error, CoreError::TooManyRuleHits { max: MAX_RULE_HITS });
    assert!(dataset.segments.is_empty() && dataset.evidence.is_empty(), "a failed run changes nothing");
}

// ---- Through the history ----

/// A dataset with a speed (and its SD child) holding an out-of-range spike
/// and a flat, and a temperature column.
fn tower() -> Dataset {
    let mut speeds: Vec<f64> = (0..300).map(|step| 5.0 + (step % 7) as f64).collect();
    speeds[40] = 99.0;
    speeds[100..112].fill(4.0);
    let mut deviation = column("sd", vec![0.5; 300]);
    deviation.subtype = Subtype::StdDev;
    deviation.parent = Some(0);
    let mut temperature = column("t", (0..300).map(|step| 20.0 + (step % 3) as f64).collect());
    temperature.kind = ColumnType::Temperature;
    make_dataset(vec![column("ws", speeds), deviation, temperature])
}

fn tower_rules() -> Vec<FlagRule> {
    vec![
        rule(Criterion::OutsideRange { min: 0.0, max: 50.0 }, ONE_STEP, &[0, 2], INVALID_FLAG),
        rule(Criterion::Stuck { tolerance: 0.0 }, 6, &[0], ICING_FLAG),
    ]
}

/// Whether the incremental summaries after `op` match a rebuild bit for bit.
fn summaries_follow(dataset: &Dataset, stats: &mut DayStats, op: &Op) -> bool {
    let (columns, steps) = op.reach();
    let view = FilterView::new(dataset);
    stats.refresh(dataset, &view, columns, steps);
    let rebuilt = DayStats::new(dataset, &view);
    let bits = |summary: &wind_core::series::ColumnSummary| (summary.valid, summary.mean.to_bits(), summary.min.to_bits(), summary.max.to_bits());
    stats.summaries().iter().zip(rebuilt.summaries().iter()).all(|(incremental, full)| bits(incremental) == bits(full))
}

#[test]
fn a_rule_run_is_an_op_undone_and_redone_exactly() {
    let mut dataset = tower();
    let mut stats = DayStats::new(&dataset, &FilterView::new(&dataset));
    let mut history = History::default();
    let op = Op::FlagByRules(RuleRun::new(tower_rules(), 0..dataset.len).unwrap());
    assert_eq!(op.reach(), (&[0, 2][..], 0..dataset.len), "the run reaches every column it searches");

    let report = history.apply(&mut dataset, op.clone()).unwrap();
    assert!(summaries_follow(&dataset, &mut stats, &op));
    let found: Vec<(usize, usize, usize, f64)> =
        report.findings.iter().map(|finding| (finding.rule, finding.evidence.start, finding.evidence.end, finding.evidence.statistic)).collect();
    assert_eq!(found, [(0, 40, 41, 99.0), (1, 100, 112, 12.0)]);
    let flagged = [Segment { column: 0, flag: INVALID_FLAG, start: 40, end: 41 }, Segment { column: 0, flag: ICING_FLAG, start: 100, end: 112 }];
    assert_eq!(dataset.segments, flagged);
    assert_eq!(dataset.evidence.len(), 2);
    let tallies = rules::tally(&RuleRun::new(tower_rules(), 0..dataset.len).unwrap(), &report.findings);
    let counts: Vec<(usize, usize, usize, usize)> = tallies.iter().map(|tally| (tally.rule, tally.column, tally.stretches, tally.steps)).collect();
    assert_eq!(counts, [(0, 0, 1, 1), (0, 2, 0, 0), (1, 0, 1, 12)], "every searched column is listed");

    // The flagged steps are now excluded, so a re-run finds nothing new and records nothing.
    let again = history.apply(&mut dataset, op.clone()).unwrap();
    assert!(again.findings.is_empty());
    let undone = history.undo(&mut dataset).unwrap();
    assert!(summaries_follow(&dataset, &mut stats, &undone));
    assert!(dataset.segments.is_empty() && dataset.evidence.is_empty());
    assert!(!history.can_undo(), "the re-run was not recorded");
    let redone = history.redo(&mut dataset).unwrap();
    assert!(summaries_follow(&dataset, &mut stats, &redone));
    assert_eq!(dataset.segments, flagged);
    assert_eq!(dataset.evidence.len(), 2);
}

#[test]
fn rerunning_under_an_included_flag_records_nothing() {
    let mut dataset = tower();
    dataset.flags[ICING_FLAG].include = true;
    let mut history = History::default();
    let op = Op::FlagByRules(RuleRun::new(tower_rules(), 0..dataset.len).unwrap());
    history.apply(&mut dataset, op.clone()).unwrap();
    let evidence = dataset.evidence.clone();
    let again = history.apply(&mut dataset, op).unwrap();
    assert_eq!(again.findings.len(), 1, "data under an included flag is searched again");
    assert!(std::sync::Arc::ptr_eq(&evidence, &dataset.evidence), "identical evidence keeps its allocation");
    history.undo(&mut dataset).unwrap();
    assert!(!history.can_undo(), "the identical re-run was not recorded");
}

#[test]
fn unflagging_trims_evidence_and_flagging_again_keeps_it() {
    let mut dataset = tower();
    let run = whole_record(&dataset, tower_rules());
    rules::apply(&mut dataset, &run).unwrap();
    flags::flag(&mut dataset, &[0], ICING_FLAG, 95..120).unwrap();
    assert_eq!(dataset.evidence[1].steps(), 100..112, "re-flagged steps keep their evidence");
    flags::unflag(&mut dataset, &[0], Some(ICING_FLAG), 104..108).unwrap();
    let flat: Vec<Range<usize>> = dataset.evidence.iter().filter(|evidence| evidence.flag == ICING_FLAG).map(Evidence::steps).collect();
    assert_eq!(flat, [100..104, 108..112]);
    let record = 0..dataset.len;
    flags::unflag(&mut dataset, &[0], None, record).unwrap();
    assert!(dataset.evidence.is_empty());
}

#[test]
fn saved_evidence_is_checked_before_it_is_restored() {
    let mut dataset = tower();
    let run = whole_record(&dataset, tower_rules());
    rules::apply(&mut dataset, &run).unwrap();
    let saved: Vec<Evidence> = dataset.evidence.to_vec();
    let mut reopened = tower();
    flags::restore(&mut reopened, flags::default_flags(), dataset.segments.clone()).unwrap();
    let refused = |reopened: &mut Dataset, change: fn(&mut Evidence)| {
        let mut damaged = saved.clone();
        change(&mut damaged[0]);
        rules::restore_evidence(reopened, damaged).unwrap_err()
    };
    assert_eq!(refused(&mut reopened, |evidence| evidence.flag = ICING_FLAG), CoreError::EvidenceNotFlagged { column: 0, start: 40, end: 41 });
    assert_eq!(refused(&mut reopened, |evidence| evidence.column = 7), CoreError::NoSuchColumn { column: 7 });
    assert!(matches!(refused(&mut reopened, |evidence| evidence.end = 301), CoreError::SegmentOutOfRange { .. }));
    assert!(matches!(refused(&mut reopened, |evidence| evidence.statistic_step = 400), CoreError::SegmentOutOfRange { .. }));
    assert!(matches!(refused(&mut reopened, |evidence| evidence.min_steps = 0), CoreError::RuleMinStepsTooSmall { .. }));
    assert!(reopened.evidence.is_empty(), "a refused restore changes nothing");
    rules::restore_evidence(&mut reopened, saved.iter().rev().copied().collect()).unwrap();
    assert!(rules::same_evidence(&reopened.evidence, &dataset.evidence), "restored evidence is canonical");
    flags::restore(&mut reopened, flags::default_flags(), Vec::new()).unwrap();
    assert!(reopened.evidence.is_empty(), "restoring segments drops the evidence of the old ones");
}

#[test]
fn a_reimport_carries_evidence_with_its_segments() {
    let rows: String = (0..8).map(|step| format!("2014-01-01 00:{step}0,{}\n", if step == 3 { 99 } else { step })).collect();
    let csv = format!("t,ws\n{rows}");
    let (mut old, _) = import_text("a", &csv, &ImportConfig::default()).unwrap();
    let range = rule(Criterion::OutsideRange { min: 0.0, max: 50.0 }, ONE_STEP, &[0], INVALID_FLAG);
    let run = whole_record(&old, vec![range]);
    rules::apply(&mut old, &run).unwrap();
    let longer = format!("t,ws\n2013-12-31 23:50,1\n{rows}");
    let (mut new, _) = import_text("a", &longer, &ImportConfig::default()).unwrap();
    flags::carry_over(&old, &mut new);
    assert_eq!(new.segments[0].steps(), 4..5);
    assert_eq!((new.evidence[0].steps(), new.evidence[0].statistic_step, new.evidence[0].statistic), (4..5, 4, 99.0));
}

// ---- Property test ----

/// xorshift64*: deterministic noise in [0, 1).
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
}

/// What `rule` finds on `column`, step by step and with no shared code:
/// the reference the scan must match.
fn reference(dataset: &Dataset, view: &FilterView, rule: &FlagRule, column: usize, window: Range<usize>) -> Vec<(usize, usize, usize, f64, usize)> {
    let values = &dataset.columns[column].values;
    let mut cursor = view.cursor(column);
    let valid: Vec<bool> = (0..dataset.len).map(|step| !cursor.excludes(step) && window.contains(&step) && !values[step].is_nan()).collect();
    let has_previous = |step: usize| step > window.start && valid[step - 1];
    // Per step: does the test hold, how strongly, and with what statistic.
    let fires = |step: usize| -> Option<(f64, f64)> {
        if !valid[step] {
            return None;
        }
        let value = values[step];
        if let Criterion::OutsideRange { min, max } = rule.criterion {
            return (value < min || value >= max).then(|| (if value < min { min - value } else { value - max }, value));
        }
        if !has_previous(step) {
            return None;
        }
        let change = value - values[step - 1];
        match rule.criterion {
            Criterion::ChangeAbove { limit } => (change.abs() > limit).then_some((change.abs(), change)),
            _ => (change.abs() <= rule_tolerance(rule)).then_some((0.0, 0.0)),
        }
    };
    let is_flat = matches!(rule.criterion, Criterion::Stuck { .. });
    let mut found = Vec::new();
    let mut step = 0;
    while step < dataset.len {
        let Some(first) = fires(step) else {
            step += 1;
            continue;
        };
        let start = if is_flat { step - 1 } else { step };
        let (mut strongest, mut statistic, mut statistic_step) = (first.0, first.1, start);
        let mut end = step + 1;
        while let Some((strength, value)) = (end < dataset.len).then(|| fires(end)).flatten() {
            if strength > strongest {
                (strongest, statistic, statistic_step) = (strength, value, end);
            }
            end += 1;
        }
        if is_flat {
            statistic = (end - start) as f64;
        }
        if end - start >= rule.min_steps {
            found.push((column, start, end, statistic, statistic_step));
        }
        step = end;
    }
    found
}

/// The tolerance of a stuck rule.
fn rule_tolerance(rule: &FlagRule) -> f64 {
    match rule.criterion {
        Criterion::Stuck { tolerance } => tolerance,
        _ => unreachable!("only stuck rules have a tolerance"),
    }
}

#[test]
fn the_scan_matches_a_step_by_step_reference() {
    let mut rng = Rng(2026);
    let steps = 3_000;
    for trial in 0..40 {
        // Coarse values so flats and repeats happen; 5 % gaps.
        let columns = (0..3)
            .map(|index| column(&format!("c{index}"), (0..steps).map(|_| if rng.next() < 0.05 { MISSING } else { (rng.below(8) as f64) * 1.5 }).collect()))
            .collect();
        let mut dataset = make_dataset(columns);
        for _ in 0..10 {
            let first = rng.below(steps);
            flags::flag(&mut dataset, &[rng.below(3)], rng.below(3), first..first + rng.below(60)).unwrap();
        }
        let rules: Vec<FlagRule> = (0..4)
            .map(|_| {
                let criterion = match rng.below(3) {
                    0 => Criterion::OutsideRange { min: rng.below(4) as f64, max: 4.0 + rng.below(6) as f64 },
                    1 => Criterion::ChangeAbove { limit: rng.below(8) as f64 },
                    _ => Criterion::Stuck { tolerance: rng.below(3) as f64 * 1.5 },
                };
                let least = if matches!(criterion, Criterion::Stuck { .. }) { 2 } else { 1 };
                let columns: Vec<usize> = (0..3).filter(|_| rng.next() < 0.6).collect();
                rule(criterion, least + rng.below(4), &columns, rng.below(3))
            })
            .collect();
        let window = rng.below(500)..steps - rng.below(500);
        let run = RuleRun::new(rules.clone(), window.clone()).unwrap();
        let view = FilterView::new(&dataset);
        let findings = rules::search(&dataset, &view, &run).unwrap();
        let mut found: Vec<(usize, usize, usize, f64, usize)> = Vec::new();
        let mut want = Vec::new();
        for (index, flag_rule) in rules.iter().enumerate() {
            for &column in &flag_rule.columns {
                want.extend(reference(&dataset, &view, flag_rule, column, window.clone()));
            }
            let mut by_rule: Vec<_> = findings
                .iter()
                .filter(|finding| finding.rule == index)
                .map(|finding| {
                    let evidence = finding.evidence;
                    (evidence.column, evidence.start, evidence.end, evidence.statistic, evidence.statistic_step)
                })
                .collect();
            by_rule.sort_by_key(|&(column, start, ..)| (column, start));
            found.extend(by_rule);
        }
        assert_eq!(found, want, "trial {trial}");

        // Applied, every evidence record sits inside a segment of its column and flag.
        let mut history = History::default();
        history.apply(&mut dataset, Op::FlagByRules(run)).unwrap();
        for evidence in dataset.evidence.iter() {
            let covered = dataset.segments.iter().any(|segment| {
                segment.column == evidence.column && segment.flag == evidence.flag && segment.start <= evidence.start && evidence.end <= segment.end
            });
            assert!(covered, "trial {trial}: {evidence:?}");
        }
    }
}

// ---- Timing ----

/// Steps and columns of the timing probe: 19 years of 10-minute data on a 30-column mast.
const PROBE_STEPS: usize = 1_000_000;
const PROBE_COLUMNS: usize = 30;
/// Rayleigh speeds (Weibull k = 2) with this scale, m/s.
const PROBE_SPEED_SCALE: f64 = 8.0;
/// Share of probe values left missing.
const PROBE_GAP_SHARE: f64 = 0.01;

#[test]
#[ignore = "timing probe; run with --release --ignored --nocapture"]
fn timing_three_rules_on_thirty_columns_of_a_million_steps() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let columns = (0..PROBE_COLUMNS)
        .map(|index| {
            let values = (0..PROBE_STEPS)
                .map(|_| if rng.next() < PROBE_GAP_SHARE { MISSING } else { PROBE_SPEED_SCALE * (-(1.0 - rng.next()).ln()).sqrt() })
                .collect();
            column(&format!("ws{index}"), values)
        })
        .collect();
    let dataset = make_dataset(columns);
    let every_column: Vec<usize> = (0..PROBE_COLUMNS).collect();
    let rules = vec![
        rule(Criterion::OutsideRange { min: 0.0, max: 30.0 }, ONE_STEP, &every_column, INVALID_FLAG),
        rule(Criterion::ChangeAbove { limit: 25.0 }, ONE_STEP, &every_column, INVALID_FLAG),
        rule(Criterion::Stuck { tolerance: 0.0 }, 6, &every_column, ICING_FLAG),
    ];
    let run = RuleRun::new(rules, 0..PROBE_STEPS).unwrap();
    let view = FilterView::new(&dataset);
    let repetitions = 3;
    let mut best = f64::INFINITY;
    let mut found = 0;
    for _ in 0..repetitions {
        let started = Instant::now();
        found = rules::search(&dataset, &view, &run).unwrap().len();
        best = best.min(started.elapsed().as_secs_f64() * 1e3);
    }
    println!("rules: 3 rules × {PROBE_COLUMNS} columns × {PROBE_STEPS} steps: {best:.1} ms, {found} stretches");
}

// ---- Flag statistics and removal by flag (item 9) ----

/// The tower after manual flags and a rule run: speed (column 0) flagged
/// invalid on 0..10 by hand and at the spike (40) by a rule; icing by hand on
/// 95..100, touching the rule's flat 100..112; temperature (column 2) invalid
/// on 5..8 by hand.
fn flagged_tower() -> Dataset {
    let mut dataset = tower();
    flags::flag(&mut dataset, &[0], INVALID_FLAG, 0..10).unwrap();
    flags::flag(&mut dataset, &[2], INVALID_FLAG, 5..8).unwrap();
    flags::flag(&mut dataset, &[0], ICING_FLAG, 95..100).unwrap();
    let run = whole_record(&dataset, tower_rules());
    rules::apply(&mut dataset, &run).unwrap();
    dataset
}

#[test]
fn statistics_count_steps_per_flag_and_column_and_find_the_evidence() {
    let dataset = flagged_tower();
    let statistics = flags::statistics(&dataset);
    assert_eq!(statistics.steps[INVALID_FLAG], [10 + 1, 0, 3]);
    assert_eq!(statistics.steps[TOWER_SHADOW_FLAG], [0, 0, 0]);
    assert_eq!(statistics.steps[ICING_FLAG], [17, 0, 0], "the manual and the rule stretch merged");
    assert_eq!(statistics.share(INVALID_FLAG, 2), 3.0 / 300.0);
    let explained: Vec<(Range<usize>, Vec<Range<usize>>)> = dataset
        .segments
        .iter()
        .zip(&statistics.evidence)
        .map(|(segment, records)| (segment.steps(), dataset.evidence[records.clone()].iter().map(Evidence::steps).collect()))
        .collect();
    assert_eq!(
        explained,
        [(0..10, vec![]), (40..41, vec![40..41]), (95..112, vec![100..112]), (5..8, vec![])],
        "manual segments have no evidence; a merged one keeps the rule's"
    );
}

#[test]
fn statistics_place_every_evidence_record_in_its_segment() {
    let mut rng = Rng(909);
    let steps = 2_000;
    for trial in 0..30 {
        let columns = (0..3).map(|index| column(&format!("c{index}"), (0..steps).map(|_| rng.below(5) as f64).collect())).collect();
        let mut dataset = make_dataset(columns);
        dataset.flags[ICING_FLAG].include = true;
        for _ in 0..12 {
            let first = rng.below(steps);
            let stretch = first..first + rng.below(80);
            match rng.below(3) {
                0 => flags::flag(&mut dataset, &[rng.below(3)], rng.below(3), stretch).unwrap(),
                1 => flags::unflag(&mut dataset, &[rng.below(3)], None, stretch).unwrap(),
                _ => {
                    let criterion = Criterion::Stuck { tolerance: 0.0 };
                    let run = whole_record(&dataset, vec![rule(criterion, 2, &[rng.below(3)], rng.below(3))]);
                    rules::apply(&mut dataset, &run).unwrap();
                }
            }
        }
        let statistics = flags::statistics(&dataset);
        let mut placed = vec![0; dataset.evidence.len()];
        for (segment, records) in dataset.segments.iter().zip(&statistics.evidence) {
            for (index, record) in dataset.evidence.iter().enumerate().take(records.end).skip(records.start) {
                assert!(record.column == segment.column && record.flag == segment.flag, "trial {trial}");
                assert!(segment.start <= record.start && record.end <= segment.end, "trial {trial}");
                placed[index] += 1;
            }
        }
        assert!(placed.iter().all(|&times| times == 1), "trial {trial}: every record in exactly one segment");
        let flagged: usize = dataset.segments.iter().map(|segment| segment.end - segment.start).sum();
        assert_eq!(statistics.steps.iter().flatten().sum::<usize>(), flagged, "trial {trial}");
    }
}

#[test]
fn removing_a_flag_reaches_only_what_carries_it_and_undoes() {
    let mut dataset = flagged_tower();
    let mut stats = DayStats::new(&dataset, &FilterView::new(&dataset));
    let mut history = History::default();
    let before = (dataset.segments.clone(), dataset.evidence.to_vec());

    let speed_only = Op::remove_flag(&dataset, INVALID_FLAG, Some(&[0])).unwrap();
    assert_eq!(speed_only.reach(), (&[0][..], 0..41), "the speed's invalid stretches, nothing more");
    history.apply(&mut dataset, speed_only.clone()).unwrap();
    assert!(summaries_follow(&dataset, &mut stats, &speed_only));
    let statistics = flags::statistics(&dataset);
    assert_eq!((statistics.steps[INVALID_FLAG].clone(), statistics.steps[ICING_FLAG][0]), (vec![0, 0, 3], 17));
    assert!(dataset.evidence.iter().all(|evidence| evidence.flag != INVALID_FLAG), "its evidence went with it");

    let everywhere = Op::remove_flag(&dataset, INVALID_FLAG, None).unwrap();
    assert_eq!(everywhere.reach(), (&[2][..], 5..8));
    history.apply(&mut dataset, everywhere).unwrap();
    assert!(flags::statistics(&dataset).steps[INVALID_FLAG].iter().all(|&steps| steps == 0));

    let nobody = Op::remove_flag(&dataset, TOWER_SHADOW_FLAG, None).unwrap();
    history.apply(&mut dataset, nobody).unwrap();
    history.undo(&mut dataset).unwrap();
    let undone = history.undo(&mut dataset).unwrap();
    assert!(summaries_follow(&dataset, &mut stats, &undone));
    assert_eq!((dataset.segments.clone(), dataset.evidence.to_vec()), before, "two undos: removing a flag nobody had recorded nothing");
    assert_eq!(Op::remove_flag(&dataset, 9, None).unwrap_err(), CoreError::NoSuchFlag { flag: 9 });
}

#[test]
fn redefining_flags_moves_segments_and_evidence_with_them() {
    let dataset = flagged_tower();
    let before = flags::statistics(&dataset);
    let icing_evidence = dataset.evidence.iter().filter(|evidence| evidence.flag == ICING_FLAG).count();
    let kept = |index: usize| (dataset.flags[index].clone(), Some(index));
    let fresh = Flag { name: " Suspeito ".into(), color: "#888888".into(), include: true, notes: "a conferir".into() };

    // Icing first, the invalid flag left out, a new one at the end.
    let mut redefined = dataset.clone();
    assert!(flags::redefine(&mut redefined, vec![kept(ICING_FLAG), kept(TOWER_SHADOW_FLAG), (fresh.clone(), None)]).unwrap());
    let after = flags::statistics(&redefined);
    assert_eq!(after.steps[0], before.steps[ICING_FLAG], "icing is flag 0 now, with its stretches");
    assert_eq!(after.steps[2], [0, 0, 0], "the new flag has none");
    assert_eq!(redefined.evidence.len(), icing_evidence, "the invalid flag's evidence went with it");
    assert!(redefined.evidence.iter().all(|evidence| evidence.flag == 0));
    for (segment, records) in redefined.segments.iter().zip(&after.evidence) {
        assert!(redefined.evidence[records.clone()].iter().all(|record| record.flag == segment.flag && segment.start <= record.start));
    }
    assert_eq!((redefined.flags[2].name.as_str(), redefined.flags[2].notes.as_str()), ("Suspeito", "a conferir"), "trimmed");

    // Edited in place and one added: nothing moves, the history still holds.
    let mut edited = dataset.clone();
    let recoloured = Flag { color: "#000000".into(), ..dataset.flags[INVALID_FLAG].clone() };
    let flags_now = vec![(recoloured, Some(INVALID_FLAG)), kept(TOWER_SHADOW_FLAG), kept(ICING_FLAG), (fresh.clone(), None)];
    assert!(!flags::redefine(&mut edited, flags_now).unwrap());
    assert_eq!((edited.segments.clone(), edited.evidence.to_vec()), (dataset.segments.clone(), dataset.evidence.to_vec()));
    assert_eq!(edited.flags[INVALID_FLAG].color, "#000000");

    // Refused, and the dataset untouched: a repeated name (case and spaces
    // aside), an empty one, a former index twice or past the flags.
    let mut refused = dataset.clone();
    let named = |name: &str| (Flag { name: name.into(), ..fresh.clone() }, None);
    assert_eq!(flags::redefine(&mut refused, vec![kept(ICING_FLAG), named("GELO ")]).unwrap_err(), CoreError::FlagNameInvalid { name: "GELO ".into() });
    assert_eq!(flags::redefine(&mut refused, vec![named("  ")]).unwrap_err(), CoreError::FlagNameInvalid { name: "  ".into() });
    assert_eq!(flags::redefine(&mut refused, vec![kept(ICING_FLAG), (fresh.clone(), Some(ICING_FLAG))]).unwrap_err(), CoreError::NoSuchFlag { flag: ICING_FLAG });
    assert_eq!(flags::redefine(&mut refused, vec![(fresh, Some(9))]).unwrap_err(), CoreError::NoSuchFlag { flag: 9 });
    assert_eq!((refused.flags.len(), refused.segments.clone()), (dataset.flags.len(), dataset.segments.clone()));
}

#[test]
fn a_twin_anemometer_reading_high_for_a_day_is_flagged_with_its_departure() {
    const DAY_STEPS: usize = 144;
    const DAYS: usize = 6;
    // The twin reads 8 m/s; the sensor matches it except on day 4, 5 % high.
    // On day 5 the twin reads 3 m/s, below the comparison range: no verdict.
    let twin: Vec<f64> = (0..DAYS * DAY_STEPS).map(|step| if step / DAY_STEPS == 5 { 3.0 } else { 8.0 }).collect();
    let sensor: Vec<f64> = (0..DAYS * DAY_STEPS).map(|step| if step / DAY_STEPS == 4 { 8.4 } else { twin[step] }).collect();
    let mut dataset = make_dataset(vec![column("sensor", sensor), column("twin", twin)]);
    let tolerance = 0.03;
    let rule = FlagRule { criterion: Criterion::TwinDrift { reference: 1, tolerance }, min_steps: ONE_STEP, columns: vec![0], flag: INVALID_FLAG };
    let run = RuleRun::new(vec![rule], 0..dataset.len).unwrap();
    let findings = rules::apply(&mut dataset, &run).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let evidence = findings[0].evidence;
    assert_eq!((evidence.column, evidence.steps(), evidence.statistic_step), (0, 4 * DAY_STEPS..5 * DAY_STEPS, 4 * DAY_STEPS));
    // The record's ratio over days 0–4 is (4 · 8 + 8.4) / (5 · 8) = 1.01, so day 4 departs by 1.05 / 1.01 − 1.
    assert!((evidence.statistic - (1.05 / 1.01 - 1.0)).abs() < 1e-12, "{}", evidence.statistic);
    assert_eq!(dataset.segments, [Segment { column: 0, flag: INVALID_FLAG, start: 4 * DAY_STEPS, end: 5 * DAY_STEPS }]);
    let refused = Criterion::TwinDrift { reference: 1, tolerance: 0.0 }.check(ONE_STEP);
    assert_eq!(refused, Err(CoreError::RuleThresholdInvalid { threshold: 0.0 }));
}

#[test]
fn a_week_with_its_clock_two_hours_ahead_is_flagged_with_the_shift() {
    const HOUR: i64 = 3_600;
    const HOURS: usize = 24;
    const WEEKS: usize = 4;
    const SHIFTED_WEEK: usize = 2;
    const SHIFT_HOURS: usize = 2;
    let week_steps = 7 * HOURS;
    // A temperature cycle peaking at 14 h; in the third week the logger's
    // clock runs 2 h ahead, so each reading carries a time 2 h too late.
    let cycle = |hour: usize| 20.0 + 5.0 * (2.0 * std::f64::consts::PI * (hour as f64 - 14.0) / HOURS as f64).cos();
    let values: Vec<f64> = (0..WEEKS * week_steps)
        .map(|step| {
            let clock_hour = step % HOURS;
            let true_hour = if step / week_steps == SHIFTED_WEEK { (clock_hour + HOURS - SHIFT_HOURS) % HOURS } else { clock_hour };
            cycle(true_hour)
        })
        .collect();
    let mut temperature = column("temp", values);
    temperature.kind = ColumnType::Temperature;
    let mut dataset = make_dataset(vec![temperature]);
    dataset.step_s = HOUR;
    let rule = FlagRule { criterion: Criterion::ClockShift { min_hours: 1.0 }, min_steps: ONE_STEP, columns: vec![0], flag: INVALID_FLAG };
    let run = RuleRun::new(vec![rule], 0..dataset.len).unwrap();
    let findings = rules::search(&dataset, &FilterView::new(&dataset), &run).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let evidence = findings[0].evidence;
    assert_eq!(evidence.steps(), SHIFTED_WEEK * week_steps..(SHIFTED_WEEK + 1) * week_steps);
    assert_eq!(evidence.statistic, SHIFT_HOURS as f64, "ahead by two hours");
    assert!(Criterion::ClockShift { min_hours: 0.5 }.check(ONE_STEP).is_err(), "shifts are whole hours");
}

//! Flag rules and their evidence on the wire (ROTEIRO item 8; the semantics
//! live in `wind_core::rules`). Rules and evidence name columns by id;
//! evidence comes back from a run and travels in project files inside
//! [`RestoreDto`], as an optional field that older projects lack.
use crate::ops::EditDto;
use crate::requests::RestoreDto;
use crate::responses::{segments, EditStateDto, FlagDto};
use crate::{finite, DatasetId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::{CoreError, CoreResult};
use wind_core::model::Dataset;
use wind_core::rules::{self, Criterion, Evidence, FlagRule, Finding, RuleTally};

/// What a rule looks for in one column; `kind` tells the tests apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum CriterionDto {
    /// The value lies outside `[min, max)`: below `min` or at or above
    /// `max`, in the column's units; `null` = no bound on that side.
    OutsideRange {
        /// Lower bound, inside the range; `null` = none.
        min: Option<f64>,
        /// Upper bound, outside the range; `null` = none.
        max: Option<f64>,
    },
    /// The change from the previous step exceeds `limit` (column units per
    /// step); the later step is flagged.
    ChangeAbove {
        /// Largest change allowed, column units per step.
        limit: f64,
    },
    /// The change from the previous step stays within `tolerance` (column
    /// units per step): the sensor holds its value.
    Stuck {
        /// Largest change that still counts as no change.
        tolerance: f64,
    },
    /// A day whose mean ratio to the `reference` column, a twin anemometer,
    /// departs from the record's ratio by more than `tolerance` (a share:
    /// 0.03 = 3 %); judged where the reference reads at least 4 m/s, on days
    /// at least half paired. The whole day is flagged.
    TwinDrift {
        /// The twin anemometer's column id.
        reference: String,
        /// Largest relative departure allowed.
        tolerance: f64,
    },
    /// A week whose diurnal cycle is shifted from the record's by at least
    /// `minHours` whole hours (a clock jump or a daylight-saving change).
    /// The whole week is flagged.
    ClockShift {
        /// Smallest shift flagged, hours (1 to 12).
        min_hours: f64,
    },
    /// The `direction` column reads inside the sector `[center − width/2,
    /// center + width/2)`, degrees, through north if need be: the searched
    /// anemometer is downwind of the tower (Flag › Sombra da torre).
    DirectionInSector {
        /// The wind direction column id.
        direction: String,
        /// Centre of the sector, degrees.
        center: f64,
        /// Width of the sector, degrees (0 to 360].
        width: f64,
    },
}

impl CriterionDto {
    /// The core's criterion, with a reference column resolved in `dataset`.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a reference `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<Criterion> {
        Ok(match self {
            CriterionDto::OutsideRange { min, max } => {
                Criterion::OutsideRange { min: min.unwrap_or(f64::NEG_INFINITY), max: max.unwrap_or(f64::INFINITY) }
            }
            CriterionDto::ChangeAbove { limit } => Criterion::ChangeAbove { limit },
            CriterionDto::Stuck { tolerance } => Criterion::Stuck { tolerance },
            CriterionDto::TwinDrift { reference, tolerance } => Criterion::TwinDrift { reference: column_index(dataset, reference)?, tolerance },
            CriterionDto::ClockShift { min_hours } => Criterion::ClockShift { min_hours },
            CriterionDto::DirectionInSector { direction, center, width } => {
                Criterion::DirectionInSector { direction: column_index(dataset, direction)?, center, width }
            }
        })
    }

    /// The wire form of `criterion`, with a reference column by its id in `dataset`.
    pub fn new(dataset: &Dataset, criterion: Criterion) -> Self {
        match criterion {
            Criterion::OutsideRange { min, max } => CriterionDto::OutsideRange { min: finite(min), max: finite(max) },
            Criterion::ChangeAbove { limit } => CriterionDto::ChangeAbove { limit },
            Criterion::Stuck { tolerance } => CriterionDto::Stuck { tolerance },
            Criterion::TwinDrift { reference, tolerance } => CriterionDto::TwinDrift { reference: dataset.columns[reference].id.clone(), tolerance },
            Criterion::ClockShift { min_hours } => CriterionDto::ClockShift { min_hours },
            Criterion::DirectionInSector { direction, center, width } => {
                CriterionDto::DirectionInSector { direction: dataset.columns[direction].id.clone(), center, width }
            }
        }
    }
}

/// A flag rule: flag `flag` on the stretches of each column where the
/// criterion holds for at least `minSteps` consecutive steps. Also saved in
/// project files.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagRuleDto {
    /// What the rule tests.
    pub criterion: CriterionDto,
    /// Column ids searched; each is flagged where the rule fires, and its
    /// SD / min / max / gust children follow.
    pub columns: Vec<String>,
    /// Index into `DatasetDto::flags`.
    pub flag: usize,
    /// Shortest stretch flagged, in steps: at least 1, or 2 for `stuck`.
    pub min_steps: usize,
}

impl FlagRuleDto {
    /// The core's rule, with column ids resolved in `dataset`.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<FlagRule> {
        let columns = self.columns.into_iter().map(|id| column_index(dataset, id)).collect::<CoreResult<_>>()?;
        Ok(FlagRule { criterion: self.criterion.into_core(dataset)?, min_steps: self.min_steps, columns, flag: self.flag })
    }
}

/// Run flag rules over a dataset's whole record, as one undoable edit.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagByRulesRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Applied together, as one edit.
    pub rules: Vec<FlagRuleDto>,
}

/// Why a stretch was flagged automatically. Also read back from a saved
/// project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceDto {
    /// Column id.
    pub column: String,
    /// Index into `DatasetDto::flags`.
    pub flag: usize,
    /// First flagged step.
    pub start_step: usize,
    /// One past the last flagged step.
    pub end_step: usize,
    /// The test that fired, with its thresholds.
    pub criterion: CriterionDto,
    /// The rule's minimum duration, in steps.
    pub min_steps: usize,
    /// What fired: for `outside_range` the value furthest outside the range;
    /// for `change_above` the largest change between steps, signed (later
    /// minus earlier); both in the column's units. For `stuck` the length of
    /// the flat, in steps. For `twin_drift` the day's departure from the
    /// record's ratio (a signed share), for `clock_shift` the week's shift in
    /// hours (positive: the logger's clock runs ahead); window tests keep the
    /// strongest window. For `direction_in_sector` the direction nearest the
    /// sector's centre, degrees. `null` for a value that is not finite.
    pub statistic: Option<f64>,
    /// Step where the statistic was measured (the first step of a flat).
    pub statistic_step: usize,
}

impl EvidenceDto {
    /// `evidence` of `dataset`, with its column by id.
    pub fn new(dataset: &Dataset, evidence: &Evidence) -> Self {
        EvidenceDto {
            column: dataset.columns[evidence.column].id.clone(),
            flag: evidence.flag,
            start_step: evidence.start,
            end_step: evidence.end,
            criterion: CriterionDto::new(dataset, evidence.criterion),
            min_steps: evidence.min_steps,
            statistic: finite(evidence.statistic),
            statistic_step: evidence.statistic_step,
        }
    }

    /// The core's evidence, with the column resolved in `dataset`; the core
    /// checks the rest when restoring.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<Evidence> {
        let criterion = self.criterion.into_core(dataset)?;
        Ok(Evidence {
            column: column_index(dataset, self.column)?,
            flag: self.flag,
            start: self.start_step,
            end: self.end_step,
            criterion,
            min_steps: self.min_steps,
            statistic: self.statistic.unwrap_or(f64::NAN),
            statistic_step: self.statistic_step,
        })
    }
}

/// What one rule of a run found in one of its columns.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuleTallyDto {
    /// Index into `FlagByRulesRequest::rules`.
    pub rule: usize,
    /// Column id.
    pub column: String,
    /// Stretches found, already flagged ones included.
    pub stretches: usize,
    /// Steps in those stretches.
    pub steps: usize,
}

/// The result of a rule run: the dataset's new state and what was found.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FlagByRulesDto {
    /// Flags and summaries after the run.
    pub state: EditStateDto,
    /// One per rule and searched column, zeros included, by rule then column.
    pub tallies: Vec<RuleTallyDto>,
    /// Every stretch found, with its evidence, in canonical order (by
    /// column, flag and step); a stretch two rules found alike appears once.
    pub found: Vec<EvidenceDto>,
}

impl FlagByRulesDto {
    /// The DTO for a run on `dataset` that left `state`.
    pub fn new(dataset: &Dataset, state: EditStateDto, tallies: &[RuleTally], findings: &[Finding]) -> Self {
        let mut found: Vec<Evidence> = findings.iter().map(|finding| finding.evidence).collect();
        rules::normalize_evidence(&mut found);
        FlagByRulesDto {
            state,
            tallies: tallies
                .iter()
                .map(|tally| RuleTallyDto {
                    rule: tally.rule,
                    column: dataset.columns[tally.column].id.clone(),
                    stretches: tally.stretches,
                    steps: tally.steps,
                })
                .collect(),
            found: found.iter().map(|evidence| EvidenceDto::new(dataset, evidence)).collect(),
        }
    }
}

impl RestoreDto {
    /// What a project saves for `dataset`: its flags, segments, evidence and
    /// value edits (left out when there are none, so such a project reads
    /// as before edits existed).
    pub fn new(dataset: &Dataset) -> Self {
        RestoreDto {
            flags: dataset.flags.iter().map(FlagDto::from).collect(),
            segments: segments(dataset),
            evidence: Some(dataset.evidence.iter().map(|evidence| EvidenceDto::new(dataset, evidence)).collect()),
            edits: (!dataset.edits.is_empty()).then(|| dataset.edits.iter().map(|edit| EditDto::new(dataset, edit)).collect()),
        }
    }

    /// The saved evidence (none for a project saved before flag rules), with
    /// column ids resolved in `dataset`.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn evidence_into_core(&self, dataset: &Dataset) -> CoreResult<Vec<Evidence>> {
        let saved = self.evidence.iter().flatten();
        saved.map(|evidence| evidence.clone().into_core(dataset)).collect()
    }

    /// Replays the saved value edits on `dataset` and takes them out, so a
    /// later [`RestoreDto::put_back`] restores only the flags.
    ///
    /// # Errors
    /// Any check of an edit.
    pub fn replay_edits(&mut self, dataset: &mut Dataset) -> CoreResult<()> {
        for edit in self.edits.take().unwrap_or_default() {
            edit.into_core(dataset)?.apply(dataset)?;
        }
        Ok(())
    }

    /// Replays the saved value edits in order on `dataset`, just read from
    /// the same file with the same configuration, then puts the saved flags,
    /// segments and evidence back. Edits first, as a re-import does: a
    /// replayed reconstruction flags what it fills, and the saved flags must
    /// win over that (the user may have unflagged it, or renamed or deleted
    /// its flag). Every adapter (browser, CLI) restores through here, so a
    /// project opens the same way everywhere.
    ///
    /// # Errors
    /// Any check of an edit, of [`RestoreDto::into_core`],
    /// [`wind_core::flags::restore`] or [`wind_core::rules::restore_evidence`].
    /// `dataset` may then be half restored: callers drop it on error.
    pub fn put_back(mut self, dataset: &mut Dataset) -> CoreResult<()> {
        // A no-op when [`RestoreDto::replay_edits`] already ran.
        self.replay_edits(dataset)?;
        let evidence = self.evidence_into_core(dataset)?;
        let (saved_flags, segments) = self.into_core(dataset)?;
        wind_core::flags::restore(dataset, saved_flags, segments)?;
        wind_core::rules::restore_evidence(dataset, evidence)
    }
}

/// Index of the column with id `id`.
fn column_index(dataset: &Dataset, id: String) -> CoreResult<usize> {
    dataset.columns.iter().position(|column| column.id == id).ok_or(CoreError::UnknownColumn { id })
}

#[cfg(test)]
mod contract {
    use super::*;
    use crate::project::ProjectDto;
    use wind_core::flags;
    use wind_core::model::{ColumnType, DataColumn, Subtype, TimeStampMeaning};
    use wind_core::rules::{self, RuleRun};

    /// Steps in the test record.
    const STEPS: usize = 20;
    /// A logger error code inside the test record.
    const ERROR_CODE: f64 = 9999.0;
    const ERROR_STEP: usize = 5;
    const INVALID_FLAG: usize = 0;

    fn dataset() -> Dataset {
        let mut speeds = vec![5.0; STEPS];
        speeds[ERROR_STEP] = ERROR_CODE;
        let column = |id: &str, values: Vec<f64>| DataColumn {
            id: id.into(),
            kind: ColumnType::WindSpeed,
            subtype: Subtype::Mean,
            units: "m/s".into(),
            height_m: None,
            parent: None,
            values: values.into(),
            derived: None,
        };
        Dataset {
            name: "torre".into(),
            start: 0,
            step_s: 600,
            len: STEPS,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![column("dir", vec![0.0; STEPS]), column("ws", speeds)],
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

    #[test]
    fn window_criteria_name_their_reference_by_id() {
        let dataset = dataset();
        let json = r#"{"kind": "twin_drift", "reference": "ws", "tolerance": 0.05}"#;
        let criterion = serde_json::from_str::<CriterionDto>(json).unwrap().into_core(&dataset).unwrap();
        assert_eq!(criterion, Criterion::TwinDrift { reference: 1, tolerance: 0.05 }, "ws is the second column");
        assert_eq!(serde_json::to_value(CriterionDto::new(&dataset, criterion)).unwrap(), serde_json::from_str::<serde_json::Value>(json).unwrap());
        let unknown = serde_json::from_str::<CriterionDto>(r#"{"kind": "twin_drift", "reference": "ws80", "tolerance": 0.05}"#).unwrap();
        assert_eq!(unknown.into_core(&dataset).unwrap_err(), CoreError::UnknownColumn { id: "ws80".into() });
        let clock = serde_json::from_str::<CriterionDto>(r#"{"kind": "clock_shift", "minHours": 1}"#).unwrap();
        assert_eq!(clock.into_core(&dataset).unwrap(), Criterion::ClockShift { min_hours: 1.0 });
    }

    #[test]
    fn rule_requests_parse_and_reject_unknown_fields() {
        let request = r#"{"dataset": 3, "rules": [
            {"criterion": {"kind": "outside_range", "min": 0, "max": null}, "columns": ["ws"], "flag": 0, "minSteps": 1},
            {"criterion": {"kind": "stuck", "tolerance": 0.1}, "columns": ["ws"], "flag": 2, "minSteps": 6}
        ]}"#;
        let parsed: FlagByRulesRequest = serde_json::from_str(request).unwrap();
        let first = parsed.rules[0].criterion.clone().into_core(&dataset()).unwrap();
        assert_eq!(first, Criterion::OutsideRange { min: 0.0, max: f64::INFINITY }, "null is no bound");
        let typo = r#"{"kind": "stuck", "tolerance": 0.1, "limit": 2}"#;
        assert!(serde_json::from_str::<CriterionDto>(typo).is_err(), "a field of another test is refused");
        let missing = r#"{"criterion": {"kind": "change_above", "limit": 2}, "columns": [], "flag": 0}"#;
        assert!(serde_json::from_str::<FlagRuleDto>(missing).is_err(), "minSteps is required");
    }

    #[test]
    fn evidence_round_trips_through_a_saved_project() {
        let mut found = dataset();
        let rule = FlagRuleDto {
            criterion: CriterionDto::OutsideRange { min: Some(0.0), max: Some(50.0) },
            columns: vec!["ws".into()],
            flag: INVALID_FLAG,
            min_steps: 1,
        };
        let run = RuleRun::new(vec![rule.into_core(&found).unwrap()], 0..STEPS).unwrap();
        let findings = rules::apply(&mut found, &run).unwrap();
        let saved = RestoreDto::new(&found);
        let evidence = &saved.evidence.as_ref().unwrap()[0];
        assert_eq!((evidence.column.as_str(), evidence.start_step, evidence.statistic), ("ws", ERROR_STEP, Some(ERROR_CODE)));
        let tallies = rules::tally(&run, &findings);
        assert_eq!(FlagByRulesDto::new(&found, EditStateDto::new(&found, &[], &Default::default()), &tallies, &findings).found, [evidence.clone()]);

        // Saved as JSON, read back, restored onto a fresh import of the same data.
        let json = serde_json::to_string(&saved).unwrap();
        let read: RestoreDto = serde_json::from_str(&json).unwrap();
        let mut reopened = dataset();
        let restored_evidence = read.evidence_into_core(&reopened).unwrap();
        let (saved_flags, saved_segments) = read.into_core(&reopened).unwrap();
        flags::restore(&mut reopened, saved_flags, saved_segments).unwrap();
        rules::restore_evidence(&mut reopened, restored_evidence).unwrap();
        assert!(rules::same_evidence(&reopened.evidence, &found.evidence));
    }

    #[test]
    fn projects_without_evidence_or_rules_still_read() {
        let format_one = r##"{"format": 1, "datasets": [{"sourceName": "a.csv", "sourceBytes": 10, "config": {"columns": []},
            "restore": {"flags": [], "segments": []}}]}"##;
        let project: ProjectDto = serde_json::from_str(format_one).unwrap();
        let saved = &project.datasets[0];
        assert!(saved.restore.evidence.is_none() && saved.rules.is_none());
        assert!(saved.restore.evidence_into_core(&dataset()).unwrap().is_empty());
        let with_rules = format_one.replace(r#""config""#, r#""rules": [{"criterion": {"kind": "change_above", "limit": 4}, "columns": ["ws"], "flag": 0, "minSteps": 1}], "config""#);
        let project: ProjectDto = serde_json::from_str(&with_rules).unwrap();
        assert_eq!(project.datasets[0].rules.as_ref().unwrap()[0].columns, ["ws"]);
        assert!(project.datasets[0].display.is_none(), "a project saved before display settings shows columns as read");
        let display = r#""display": {"naming": "systematic", "heightDecimals": 1, "order": "type_then_height", "highestFirst": true,
            "hidden": [{"kind": "wind_speed", "subtype": "min"}], "colors": "by_type"}, "config""#;
        let project: ProjectDto = serde_json::from_str(&format_one.replace(r#""config""#, display)).unwrap();
        assert_eq!(project.datasets[0].display.as_ref().unwrap().hidden[0].subtype, crate::ColumnSubtype::Min);
    }

    #[test]
    fn infinite_statistics_travel_as_null() {
        let evidence = Evidence {
            column: 1,
            flag: INVALID_FLAG,
            start: 0,
            end: 1,
            criterion: Criterion::OutsideRange { min: f64::NEG_INFINITY, max: 50.0 },
            min_steps: 1,
            statistic: f64::INFINITY,
            statistic_step: 0,
        };
        let json = serde_json::to_string(&EvidenceDto::new(&dataset(), &evidence)).unwrap();
        assert!(json.contains(r#""statistic":null"#) && json.contains(r#""min":null"#), "{json}");
    }
}

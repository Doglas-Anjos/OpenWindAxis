//! Flag statistics and removal by flag on the wire (ROTEIRO item 9,
//! ESPEC-INTERFACE §5.3; the counting lives in `wind_core::flags::statistics`).
use crate::rules::EvidenceDto;
use crate::DatasetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::flags::FlagStatistics;
use wind_core::model::Dataset;

/// Flagged steps per flag and column, and every flagged segment with the rule
/// evidence inside it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FlagStatisticsDto {
    /// Steps in the record.
    pub step_count: usize,
    /// `[flag][column]`, aligned with `DatasetDto::flags` and
    /// `DatasetDto::columns`: steps of each column's own segments with each
    /// flag. SD / min / max / gust children follow their mean column and
    /// count 0 here.
    pub steps: Vec<Vec<usize>>,
    /// `[flag][column]`: `steps` as a share of the record, 0..1.
    pub share: Vec<Vec<f64>>,
    /// Every flagged segment, by column, flag and first step.
    pub segments: Vec<FlaggedSegmentDto>,
}

/// A flagged segment and the rule evidence inside it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FlaggedSegmentDto {
    /// Column id.
    pub column: String,
    /// Index into `DatasetDto::flags`.
    pub flag: usize,
    /// First flagged step.
    pub start_step: usize,
    /// One past the last flagged step.
    pub end_step: usize,
    /// Why rules flagged parts of the segment; empty for a manual flag.
    pub evidence: Vec<EvidenceDto>,
}

impl FlagStatisticsDto {
    /// The DTO of `statistics`, computed on `dataset`.
    pub fn new(dataset: &Dataset, statistics: &FlagStatistics) -> Self {
        let share = (0..statistics.steps.len())
            .map(|flag| (0..dataset.columns.len()).map(|column| statistics.share(flag, column)).collect())
            .collect();
        let segments = dataset
            .segments
            .iter()
            .zip(&statistics.evidence)
            .map(|(segment, records)| FlaggedSegmentDto {
                column: dataset.columns[segment.column].id.clone(),
                flag: segment.flag,
                start_step: segment.start,
                end_step: segment.end,
                evidence: dataset.evidence[records.clone()].iter().map(|record| EvidenceDto::new(dataset, record)).collect(),
            })
            .collect();
        FlagStatisticsDto { step_count: statistics.step_count, steps: statistics.steps.clone(), share, segments }
    }
}

/// Remove one flag over the whole record, from some columns or from all of
/// them, as one undoable edit.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveFlagRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Index into `DatasetDto::flags`.
    pub flag: usize,
    /// Column ids; omitted = every column.
    #[serde(default)]
    #[ts(optional)]
    pub columns: Option<Vec<String>>,
}

#[cfg(test)]
mod contract {
    use super::*;
    use crate::rules::{CriterionDto, FlagRuleDto};
    use wind_core::flags;
    use wind_core::model::{ColumnType, DataColumn, Subtype, TimeStampMeaning};
    use wind_core::rules::{self, RuleRun};

    /// Steps in the test record.
    const STEPS: usize = 20;
    const INVALID_FLAG: usize = 0;
    /// A speed column with one out-of-range step, flagged by a rule.
    const SPIKE_STEP: usize = 12;
    const SPIKE_SPEED: f64 = 80.0;
    /// Steps flagged by hand.
    const MANUAL_STEPS: std::ops::Range<usize> = 2..5;

    #[test]
    fn statistics_carry_counts_shares_and_evidence_per_segment() {
        let mut speeds = vec![5.0; STEPS];
        speeds[SPIKE_STEP] = SPIKE_SPEED;
        let column = DataColumn {
            id: "ws".into(),
            kind: ColumnType::WindSpeed,
            subtype: Subtype::Mean,
            units: "m/s".into(),
            height_m: None,
            parent: None,
            values: speeds.into(),
            derived: None,
        };
        let mut dataset = Dataset {
            name: "torre".into(),
            start: 0,
            step_s: 600,
            len: STEPS,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![column],
            flags: flags::default_flags(),
            segments: Vec::new(),
            evidence: Default::default(),
            edits: Vec::new(),
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            displacement: None,
        };
        flags::flag(&mut dataset, &[0], INVALID_FLAG, MANUAL_STEPS).unwrap();
        let rule = FlagRuleDto {
            criterion: CriterionDto::OutsideRange { min: None, max: Some(50.0) },
            columns: vec!["ws".into()],
            flag: INVALID_FLAG,
            min_steps: 1,
        };
        let run = RuleRun::new(vec![rule.into_core(&dataset).unwrap()], 0..STEPS).unwrap();
        rules::apply(&mut dataset, &run).unwrap();

        let json = serde_json::to_value(FlagStatisticsDto::new(&dataset, &flags::statistics(&dataset))).unwrap();
        assert_eq!(json["stepCount"], STEPS);
        assert_eq!(json["steps"][INVALID_FLAG][0], 4);
        assert_eq!(json["share"][INVALID_FLAG][0], 4.0 / STEPS as f64);
        assert_eq!(json["segments"][0]["evidence"], serde_json::json!([]), "a manual segment has no evidence");
        assert_eq!(json["segments"][1]["startStep"], SPIKE_STEP);
        assert_eq!(json["segments"][1]["evidence"][0]["statistic"], SPIKE_SPEED);
        assert_eq!(json["segments"][1]["evidence"][0]["criterion"]["kind"], "outside_range");
    }

    #[test]
    fn removal_requests_default_to_every_column_and_reject_typos() {
        let every: RemoveFlagRequest = serde_json::from_str(r#"{"dataset": 1, "flag": 2}"#).unwrap();
        assert!(every.columns.is_none());
        let some: RemoveFlagRequest = serde_json::from_str(r#"{"dataset": 1, "flag": 2, "columns": ["ws"]}"#).unwrap();
        assert_eq!(some.columns.unwrap(), ["ws"]);
        assert!(serde_json::from_str::<RemoveFlagRequest>(r#"{"dataset": 1, "flag": 2, "column": "ws"}"#).is_err());
    }
}

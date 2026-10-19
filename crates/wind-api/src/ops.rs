//! Edits as data: the wire form of the core's [`Op`]. A JSON list of
//! [`OpDto`] is a workflow (ARQUITETURA C2): it can be versioned and run by
//! `wind apply` without a UI. Columns are named by id and stretches by
//! instants, so a list still applies after a re-import that changed the
//! column order or the time step; both are resolved against the dataset at
//! run time, the way [`crate::requests::RestoreDto::into_core`] resolves ids.
use crate::filter::FilterDto;
use crate::rules::FlagRuleDto;
use crate::DatasetId;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use ts_rs::TS;
use wind_core::edits::{Edit, Reconstruction};
use wind_core::error::{CoreError, CoreResult};
use wind_core::filter::FilterView;
use wind_core::model::Dataset;
use wind_core::region;
use wind_core::ops::Op;
use wind_core::rules::RuleRun;

/// Milliseconds in a second: instants on the wire are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;

/// One edit of a dataset, tagged by `op`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum OpDto {
    /// Flag the steps between two instants on some columns; their SD / min /
    /// max / gust children follow.
    Flag {
        /// Column ids.
        columns: Vec<String>,
        /// Index into the dataset's flags.
        flag: usize,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
    },
    /// Remove flags from the steps between two instants on some columns.
    Unflag {
        /// Column ids; their SD / min / max / gust children follow.
        columns: Vec<String>,
        /// Only this flag; omitted = every flag.
        #[serde(default)]
        #[ts(optional)]
        flag: Option<usize>,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
    },
    /// Flag the steps whose point lies inside a selection drawn on the scatter
    /// of `x` against `y` (Flag › Flag por dispersão), among the steps
    /// `filter` keeps; the columns' SD / min / max / gust children follow.
    FlagRegion {
        /// The selection: the scatter's columns, its shapes and its filter.
        #[serde(flatten)]
        #[ts(flatten)]
        selection: SelectionDto,
        /// Column ids flagged.
        columns: Vec<String>,
        /// Index into the dataset's flags.
        flag: usize,
    },
    /// Remove flags from the steps a scatter selection holds.
    UnflagRegion {
        /// The selection: the scatter's columns, its shapes and its filter.
        #[serde(flatten)]
        #[ts(flatten)]
        selection: SelectionDto,
        /// Column ids cleared.
        columns: Vec<String>,
        /// Only this flag; omitted = every flag.
        #[serde(default)]
        #[ts(optional)]
        flag: Option<usize>,
    },
    /// Run flag rules over the whole record, keeping each stretch's evidence.
    FlagByRules {
        /// Applied together, as one edit.
        rules: Vec<FlagRuleDto>,
    },
    /// `value · scale + offset` on the steps between two instants of some
    /// columns (Revisar › Escala e offset); directions wrap into 0–360°,
    /// temperatures are scaled in kelvin.
    ScaleOffset {
        /// Column ids; their SD / min / max / gust children are not included.
        columns: Vec<String>,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
        /// Factor applied first.
        scale: f64,
        /// Added after scaling, in the column's units.
        offset: f64,
    },
    /// Delete the values between two instants of some columns (Revisar ›
    /// Excluir dados): they become missing.
    DeleteData {
        /// Column ids.
        columns: Vec<String>,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
    },
    /// Move the values between two instants of some columns by a number of
    /// steps, over what is there (Revisar › Deslocar tempo, a stretch).
    ShiftSteps {
        /// Column ids.
        columns: Vec<String>,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
        /// Steps to move by; positive = later.
        by_steps: i32,
    },
    /// Fill the missing values between two instants of some columns
    /// (Revisar › Reconstruir lacunas), flagging them "Reconstruído".
    ReconstructGaps {
        /// Column ids.
        columns: Vec<String>,
        /// Window start, epoch ms.
        from_ms: f64,
        /// Window end, epoch ms; the step containing it is included.
        to_ms: f64,
        /// How.
        method: ReconstructionDto,
    },
    /// Move the whole record in time (Revisar › Deslocar tempo, the whole
    /// dataset): only the time stamps change.
    ShiftStart {
        /// Milliseconds to add to every time stamp, rounded to whole seconds.
        by_ms: f64,
    },
}

/// A selection drawn on a scatter plot ([`wind_core::region`]).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionDto {
    /// Column id on the horizontal axis.
    pub x: String,
    /// Column id on the vertical axis.
    pub y: String,
    /// Polygons in the plot's data units, [x, y] corners, at least three
    /// each: free-form, rectangular and triangular selections alike.
    pub shapes: Vec<Vec<[f64; 2]>>,
    /// The scatter's "Filtrar por" criteria; omitted = default filtering.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

impl SelectionDto {
    /// The runs of steps the selection holds in `dataset`.
    ///
    /// # Errors
    /// An unknown column, the filter's, or a shape of fewer than three corners.
    pub fn runs(&self, dataset: &Dataset) -> CoreResult<Vec<Range<usize>>> {
        let view = match &self.filter {
            None => FilterView::new(dataset),
            Some(filter) => FilterView::with_criteria(dataset, &filter.clone().into_core(dataset)?)?,
        };
        region::steps_inside(dataset, &view, column_index(dataset, &self.x)?, column_index(dataset, &self.y)?, &self.shapes)
    }
}

/// How many steps a scatter selection holds, before it is flagged.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// The selection.
    #[serde(flatten)]
    #[ts(flatten)]
    pub selection: SelectionDto,
}

/// The steps a scatter selection holds.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCountDto {
    /// Steps inside the selection.
    pub steps: usize,
}

/// How a reconstruction fills gaps, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReconstructionDto {
    /// From another speed column at another height (its id), by the power
    /// law with the month × hour shear exponents.
    FromHeight {
        /// The source column id.
        source: String,
    },
    /// A Markov chain over the column's own values.
    Markov,
}

impl ReconstructionDto {
    fn new(dataset: &Dataset, method: Reconstruction) -> Self {
        match method {
            Reconstruction::FromHeight { source } => ReconstructionDto::FromHeight { source: dataset.columns[source].id.clone() },
            Reconstruction::Markov => ReconstructionDto::Markov,
        }
    }

    fn into_core(self, dataset: &Dataset) -> CoreResult<Reconstruction> {
        Ok(match self {
            ReconstructionDto::FromHeight { source } => Reconstruction::FromHeight { source: column_index(dataset, &source)? },
            ReconstructionDto::Markov => Reconstruction::Markov,
        })
    }
}

/// Apply one op to a dataset, as one undoable edit. Returns `EditStateDto`.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyOpRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// The edit.
    pub op: OpDto,
}

/// A value edit as a project saves it ([`wind_core::edits`]): columns by
/// id and steps as indices, like saved segments, since a project reopens
/// its file with the configuration it was saved with.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(tag = "edit", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum EditDto {
    /// See [`Edit::ScaleOffset`].
    ScaleOffset {
        /// Column ids.
        columns: Vec<String>,
        /// First step edited.
        start_step: usize,
        /// One past the last step edited.
        end_step: usize,
        /// Factor applied first.
        scale: f64,
        /// Added after scaling.
        offset: f64,
    },
    /// See [`Edit::Delete`].
    Delete {
        /// Column ids.
        columns: Vec<String>,
        /// First step deleted.
        start_step: usize,
        /// One past the last step deleted.
        end_step: usize,
    },
    /// See [`Edit::ShiftSteps`].
    ShiftSteps {
        /// Column ids.
        columns: Vec<String>,
        /// First step moved.
        start_step: usize,
        /// One past the last step moved.
        end_step: usize,
        /// Steps moved by; positive = later.
        by_steps: i64,
    },
    /// See [`Edit::Reconstruct`].
    Reconstruct {
        /// Column ids.
        columns: Vec<String>,
        /// First step searched.
        start_step: usize,
        /// One past the last step searched.
        end_step: usize,
        /// How.
        method: ReconstructionDto,
    },
    /// See [`Edit::ShiftStart`].
    ShiftStart {
        /// Seconds added to every time stamp.
        by_seconds: i64,
    },
}

impl EditDto {
    /// The wire form of `edit`, with column ids from `dataset`.
    pub fn new(dataset: &Dataset, edit: &Edit) -> Self {
        let ids = |columns: &[usize]| columns.iter().map(|&column| dataset.columns[column].id.clone()).collect();
        match edit {
            Edit::ScaleOffset { columns, steps, scale, offset } => {
                EditDto::ScaleOffset { columns: ids(columns), start_step: steps.start, end_step: steps.end, scale: *scale, offset: *offset }
            }
            Edit::Delete { columns, steps } => EditDto::Delete { columns: ids(columns), start_step: steps.start, end_step: steps.end },
            Edit::ShiftSteps { columns, steps, by } => {
                EditDto::ShiftSteps { columns: ids(columns), start_step: steps.start, end_step: steps.end, by_steps: *by }
            }
            Edit::Reconstruct { columns, steps, method } => {
                EditDto::Reconstruct { columns: ids(columns), start_step: steps.start, end_step: steps.end, method: ReconstructionDto::new(dataset, *method) }
            }
            Edit::ShiftStart { by_s } => EditDto::ShiftStart { by_seconds: *by_s },
        }
    }

    /// The core's edit, with column ids resolved in `dataset`; the core
    /// checks the rest when applying it.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<Edit> {
        Ok(match self {
            EditDto::ScaleOffset { columns, start_step, end_step, scale, offset } => {
                Edit::ScaleOffset { columns: column_indices(dataset, &columns)?, steps: start_step..end_step, scale, offset }
            }
            EditDto::Delete { columns, start_step, end_step } => Edit::Delete { columns: column_indices(dataset, &columns)?, steps: start_step..end_step },
            EditDto::ShiftSteps { columns, start_step, end_step, by_steps } => {
                Edit::ShiftSteps { columns: column_indices(dataset, &columns)?, steps: start_step..end_step, by: by_steps }
            }
            EditDto::Reconstruct { columns, start_step, end_step, method } => {
                Edit::Reconstruct { columns: column_indices(dataset, &columns)?, steps: start_step..end_step, method: method.into_core(dataset)? }
            }
            EditDto::ShiftStart { by_seconds } => Edit::ShiftStart { by_s: by_seconds },
        })
    }
}

impl OpDto {
    /// The core's op, with column ids and instants resolved in `dataset`.
    /// The flag index is checked when the op is applied.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks, or a rule's
    /// own check (thresholds, durations, rule count).
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<Op> {
        Ok(match self {
            OpDto::Flag { columns, flag, from_ms, to_ms } => Op::Flag {
                columns: column_indices(dataset, &columns)?,
                flag,
                steps: steps_between(dataset, from_ms, to_ms),
            },
            OpDto::Unflag { columns, flag, from_ms, to_ms } => Op::Unflag {
                columns: column_indices(dataset, &columns)?,
                flag,
                steps: steps_between(dataset, from_ms, to_ms),
            },
            OpDto::FlagRegion { selection, columns, flag } => {
                Op::FlagSteps { columns: column_indices(dataset, &columns)?, flag, runs: selection.runs(dataset)? }
            }
            OpDto::UnflagRegion { selection, columns, flag } => {
                Op::UnflagSteps { columns: column_indices(dataset, &columns)?, flag, runs: selection.runs(dataset)? }
            }
            OpDto::FlagByRules { rules } => {
                let rules = rules.into_iter().map(|rule| rule.into_core(dataset)).collect::<CoreResult<Vec<_>>>()?;
                Op::FlagByRules(RuleRun::new(rules, 0..dataset.len)?)
            }
            OpDto::ScaleOffset { columns, from_ms, to_ms, scale, offset } => Op::Edit(Edit::ScaleOffset {
                columns: column_indices(dataset, &columns)?,
                steps: steps_between(dataset, from_ms, to_ms),
                scale,
                offset,
            }),
            OpDto::DeleteData { columns, from_ms, to_ms } => {
                Op::Edit(Edit::Delete { columns: column_indices(dataset, &columns)?, steps: steps_between(dataset, from_ms, to_ms) })
            }
            OpDto::ShiftSteps { columns, from_ms, to_ms, by_steps } => Op::Edit(Edit::ShiftSteps {
                columns: column_indices(dataset, &columns)?,
                steps: steps_between(dataset, from_ms, to_ms),
                by: i64::from(by_steps),
            }),
            OpDto::ReconstructGaps { columns, from_ms, to_ms, method } => Op::Edit(Edit::Reconstruct {
                columns: column_indices(dataset, &columns)?,
                steps: steps_between(dataset, from_ms, to_ms),
                method: method.into_core(dataset)?,
            }),
            // A float-to-int `as` saturates, and NaN becomes 0: no shift.
            OpDto::ShiftStart { by_ms } => Op::Edit(Edit::ShiftStart { by_s: (by_ms / MS_PER_SECOND).round() as i64 }),
        })
    }
}

/// Index of the column with id `id` in `dataset`.
///
/// # Errors
/// [`CoreError::UnknownColumn`] when no column has it.
pub fn column_index(dataset: &Dataset, id: &str) -> CoreResult<usize> {
    dataset
        .columns
        .iter()
        .position(|column| column.id == id)
        .ok_or_else(|| CoreError::UnknownColumn { id: id.to_string() })
}

/// [`column_index`] of every id, in order.
///
/// # Errors
/// [`CoreError::UnknownColumn`] for the first id `dataset` lacks.
pub fn column_indices(dataset: &Dataset, ids: &[String]) -> CoreResult<Vec<usize>> {
    ids.iter().map(|id| column_index(dataset, id)).collect()
}

/// Steps overlapping the instants `from_ms..=to_ms` (epoch ms), as the
/// browser's flag requests resolve them; empty for a reversed or NaN window.
pub fn steps_between(dataset: &Dataset, from_ms: f64, to_ms: f64) -> Range<usize> {
    if !(from_ms <= to_ms) {
        return 0..0;
    }
    // A float-to-int `as` saturates, so even ±∞ lands on a clamped step.
    let to_seconds = |ms: f64| (ms / MS_PER_SECOND).floor() as i64;
    dataset.steps_between(to_seconds(from_ms), to_seconds(to_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wind_core::model::{ColumnType, DataColumn, Subtype, TimeStampMeaning};

    /// Ten-minute steps.
    const STEP_S: i64 = 600;
    const STEPS: usize = 10;
    const MS_PER_STEP: f64 = STEP_S as f64 * MS_PER_SECOND;
    const INVALID_FLAG: usize = 0;
    /// The stretch flagged: a window from the start of one step to 1 ms into another.
    const FIRST_FLAGGED_STEP: usize = 2;
    const LAST_FLAGGED_STEP: usize = 4;

    fn dataset() -> Dataset {
        let column = |id: &str| DataColumn {
            id: id.into(),
            kind: ColumnType::WindSpeed,
            subtype: Subtype::Mean,
            units: String::new(),
            height_m: None,
            parent: None,
            values: vec![1.0; STEPS].into(),
            derived: None,
        };
        Dataset {
            name: "torre".into(),
            start: 0,
            step_s: STEP_S,
            len: STEPS,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![column("dir"), column("ws")],
            evidence: Default::default(),
            edits: Vec::new(),
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            displacement: None,
            flags: wind_core::flags::default_flags(),
            segments: Vec::new(),
        }
    }

    #[test]
    fn ops_resolve_ids_and_instants() {
        let json = format!(
            r#"[{{"op": "flag", "columns": ["ws"], "flag": {INVALID_FLAG}, "fromMs": {}, "toMs": {}}},
                {{"op": "unflag", "columns": ["dir", "ws"], "fromMs": 0, "toMs": 0}}]"#,
            FIRST_FLAGGED_STEP as f64 * MS_PER_STEP,
            LAST_FLAGGED_STEP as f64 * MS_PER_STEP + 1.0
        );
        let ops: Vec<OpDto> = serde_json::from_str(&json).unwrap();
        let dataset = dataset();
        let resolved: Vec<Op> = ops.into_iter().map(|op| op.into_core(&dataset).unwrap()).collect();
        let flagged = FIRST_FLAGGED_STEP..LAST_FLAGGED_STEP + 1;
        assert_eq!(resolved[0], Op::Flag { columns: vec![1], flag: INVALID_FLAG, steps: flagged });
        assert_eq!(resolved[1], Op::Unflag { columns: vec![0, 1], flag: None, steps: 0..1 });
    }

    #[test]
    fn typos_and_unknown_columns_are_refused() {
        let typo = r#"{"op": "flag", "columns": ["ws"], "flag": 0, "fromMs": 0, "toMS": 1}"#;
        assert!(serde_json::from_str::<OpDto>(typo).is_err(), "unknown field");
        let unknown_op = r#"{"op": "scale", "columns": ["ws"], "fromMs": 0, "toMs": 1}"#;
        assert!(serde_json::from_str::<OpDto>(unknown_op).is_err(), "unknown op");
        let missing: OpDto =
            serde_json::from_str(r#"{"op": "unflag", "columns": ["ws80"], "fromMs": 0, "toMs": 1}"#).unwrap();
        assert_eq!(missing.into_core(&dataset()).unwrap_err(), CoreError::UnknownColumn { id: "ws80".into() });
    }

    #[test]
    fn rule_runs_are_ops_too() {
        let rule = |min_steps: usize| {
            format!(
                r#"{{"op": "flag_by_rules", "rules": [{{"criterion": {{"kind": "outside_range", "min": 0, "max": null}},
                    "columns": ["ws"], "flag": {INVALID_FLAG}, "minSteps": {min_steps}}}]}}"#
            )
        };
        let shortest = 1;
        let op: OpDto = serde_json::from_str(&rule(shortest)).unwrap();
        let resolved = op.into_core(&dataset()).unwrap();
        assert!(matches!(&resolved, Op::FlagByRules(run) if run.columns() == [1] && run.steps() == (0..STEPS)), "{resolved:?}");
        let too_short: OpDto = serde_json::from_str(&rule(shortest - 1)).unwrap();
        assert!(matches!(too_short.into_core(&dataset()), Err(CoreError::RuleMinStepsTooSmall { .. })));
    }

    #[test]
    fn a_reversed_or_nan_window_flags_nothing() {
        let dataset = dataset();
        assert_eq!(steps_between(&dataset, MS_PER_STEP, 0.0), 0..0);
        assert_eq!(steps_between(&dataset, f64::NAN, MS_PER_STEP), 0..0);
        assert_eq!(steps_between(&dataset, f64::NEG_INFINITY, f64::INFINITY), 0..STEPS);
    }

    #[test]
    fn value_edits_resolve_ids_and_instants() {
        let json = format!(
            r#"[{{"op": "scale_offset", "columns": ["ws"], "fromMs": 0, "toMs": {MS_PER_STEP}, "scale": 2, "offset": 0.5}},
                {{"op": "delete_data", "columns": ["dir"], "fromMs": 0, "toMs": 0}},
                {{"op": "shift_steps", "columns": ["dir"], "fromMs": 0, "toMs": 0, "bySteps": -1}},
                {{"op": "shift_start", "byMs": 1500}}]"#
        );
        let ops: Vec<OpDto> = serde_json::from_str(&json).unwrap();
        let dataset = dataset();
        let resolved: Vec<Op> = ops.into_iter().map(|op| op.into_core(&dataset).unwrap()).collect();
        assert_eq!(resolved[0], Op::Edit(Edit::ScaleOffset { columns: vec![1], steps: 0..2, scale: 2.0, offset: 0.5 }));
        assert_eq!(resolved[1], Op::Edit(Edit::Delete { columns: vec![0], steps: 0..1 }));
        assert_eq!(resolved[2], Op::Edit(Edit::ShiftSteps { columns: vec![0], steps: 0..1, by: -1 }));
        assert_eq!(resolved[3], Op::Edit(Edit::ShiftStart { by_s: 2 }), "1.5 s rounds to whole seconds");
    }

    #[test]
    fn a_reconstruction_reopens_with_its_values_and_flag() {
        let mut edited = dataset();
        Edit::Delete { columns: vec![1], steps: 3..6 }.apply(&mut edited).unwrap();
        Edit::Reconstruct { columns: vec![1], steps: 0..STEPS, method: Reconstruction::Markov }.apply(&mut edited).unwrap();
        assert!(edited.columns[1].values.iter().all(|&value| value == 1.0), "a constant column refills with its value");
        let saved = serde_json::to_string(&crate::requests::RestoreDto::new(&edited)).unwrap();
        let mut reopened = dataset();
        serde_json::from_str::<crate::requests::RestoreDto>(&saved).unwrap().put_back(&mut reopened).unwrap();
        assert_eq!(reopened.columns[1].values[..], edited.columns[1].values[..]);
        assert_eq!((&reopened.segments, reopened.flags.len()), (&edited.segments, edited.flags.len()));
    }

    #[test]
    fn saved_flags_win_over_a_replayed_reconstruction() {
        // Reconstruct, then unflag what was filled and rename the flag: the
        // project reopens as saved, not as the replay would flag it.
        let mut edited = dataset();
        Edit::Delete { columns: vec![1], steps: 3..6 }.apply(&mut edited).unwrap();
        Edit::Reconstruct { columns: vec![1], steps: 0..STEPS, method: Reconstruction::Markov }.apply(&mut edited).unwrap();
        let filled = edited.flags.len() - 1;
        wind_core::flags::unflag(&mut edited, &[1], Some(filled), 0..STEPS).unwrap();
        let mut flags: Vec<_> = edited.flags.iter().cloned().enumerate().map(|(index, flag)| (flag, Some(index))).collect();
        flags[filled].0.name = "Preenchido".into();
        wind_core::flags::redefine(&mut edited, flags).unwrap();
        let saved = serde_json::to_string(&crate::requests::RestoreDto::new(&edited)).unwrap();
        let mut reopened = dataset();
        serde_json::from_str::<crate::requests::RestoreDto>(&saved).unwrap().put_back(&mut reopened).unwrap();
        assert_eq!(reopened.columns[1].values[..], edited.columns[1].values[..], "the values are refilled");
        assert!(reopened.segments.is_empty(), "the unflagged steps stay unflagged: {:?}", reopened.segments);
        let names = |dataset: &Dataset| dataset.flags.iter().map(|flag| flag.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&reopened), names(&edited), "no second Reconstruído flag");
    }

    #[test]
    fn value_edits_survive_a_project_round_trip() {
        let mut edited = dataset();
        Edit::ScaleOffset { columns: vec![1], steps: 2..5, scale: 3.0, offset: 1.0 }.apply(&mut edited).unwrap();
        Edit::ShiftStart { by_s: STEP_S }.apply(&mut edited).unwrap();
        let saved = serde_json::to_string(&crate::requests::RestoreDto::new(&edited)).unwrap();
        let mut reopened = dataset();
        serde_json::from_str::<crate::requests::RestoreDto>(&saved).unwrap().put_back(&mut reopened).unwrap();
        assert_eq!(reopened.columns[1].values[..], edited.columns[1].values[..]);
        assert_eq!((reopened.start, &reopened.edits), (edited.start, &edited.edits));
        let untouched = serde_json::to_string(&crate::requests::RestoreDto::new(&dataset())).unwrap();
        assert!(!untouched.contains("edits"), "no edits, no field: the project reads as before edits existed");
    }
}

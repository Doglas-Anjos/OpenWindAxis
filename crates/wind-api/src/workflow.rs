//! Workflows (ESPEC-INTERFACE §1, menu Fluxos; ARQUITETURA C2): a saved
//! list of edits ([`OpDto`]: columns by id, stretches by instant) and of
//! analyses (by the names of [`analyses::run`]) applied to datasets one
//! after another, in the browser or with `wind run`. The same list fits any
//! dataset with the same column ids.
use crate::analyses::{self, Prepared};
use crate::ops::OpDto;
use crate::responses::EditStateDto;
use crate::{DatasetId, ErrorDto};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use wind_core::filter::FilterView;
use wind_core::history::History;
use wind_core::model::Dataset;
use wind_core::series::DayStats;

/// Version of the workflow layout; readers refuse others.
pub const WORKFLOW_FORMAT: u32 = 1;
/// Code of an [`ErrorDto`] for a workflow of another layout.
pub const OTHER_WORKFLOW_FORMAT: &str = "other_workflow_format";
/// Request field naming the dataset: set to each dataset in turn.
const DATASET_FIELD: &str = "dataset";

/// A saved workflow.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDto {
    /// [`WORKFLOW_FORMAT`].
    pub format: u32,
    /// A name to show.
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// Edits applied in order, each one undoable on its own.
    pub ops: Vec<OpDto>,
    /// Analyses run after the edits.
    pub analyses: Vec<WorkflowAnalysisDto>,
}

/// One analysis of a workflow.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowAnalysisDto {
    /// One of [`analyses::ANALYSIS_NAMES`].
    pub analysis: String,
    /// Its request; the `dataset` field is set to each dataset in turn.
    #[ts(type = "unknown")]
    pub request: Value,
}

/// Run a workflow on some datasets of the session, in order.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRunRequest {
    /// The datasets.
    pub datasets: Vec<DatasetId>,
    /// The workflow.
    pub workflow: WorkflowDto,
}

/// What a workflow did to one dataset.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowOutcomeDto {
    /// The dataset.
    pub dataset: DatasetId,
    /// Its name.
    pub name: String,
    /// Edits applied before the first failure (all of them without one).
    pub ops_applied: usize,
    /// Why the next edit failed; the analyses were not run then.
    pub op_error: Option<ErrorDto>,
    /// One per analysis, in order.
    pub results: Vec<WorkflowResultDto>,
    /// The dataset's state after the edits.
    pub state: EditStateDto,
}

/// One analysis's answer or failure.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowResultDto {
    /// The analysis.
    pub analysis: String,
    /// Its response DTO.
    #[ts(type = "unknown")]
    pub result: Option<Value>,
    /// Or why it failed.
    pub error: Option<ErrorDto>,
}

/// Fails unless `workflow` is of [`WORKFLOW_FORMAT`].
///
/// # Errors
/// [`OTHER_WORKFLOW_FORMAT`].
pub fn check(workflow: &WorkflowDto) -> Result<(), ErrorDto> {
    if workflow.format == WORKFLOW_FORMAT {
        Ok(())
    } else {
        Err(ErrorDto { code: OTHER_WORKFLOW_FORMAT.into(), message: format!("workflow format {} (this version reads {WORKFLOW_FORMAT})", workflow.format) })
    }
}

/// Applies `workflow` to dataset `id` (`data`, with its undo `history`):
/// the edits in order through the history, stopping at the first failure,
/// then, when all applied, the analyses on the edited data. The caller's
/// filter and per-day summaries are stale afterwards: rebuild them.
pub fn run(id: DatasetId, data: &mut Dataset, history: &mut History, workflow: &WorkflowDto) -> WorkflowOutcomeDto {
    let (mut ops_applied, mut values_changed) = (0, false);
    let mut op_error = None;
    for op in &workflow.ops {
        let applied = op.clone().into_core(data).and_then(|op| {
            values_changed |= op.changes_values();
            history.apply(data, op)
        });
        match applied {
            Ok(_) => ops_applied += 1,
            Err(error) => {
                op_error = Some(ErrorDto::from(&error));
                break;
            }
        }
    }
    let view = FilterView::new(data);
    let stats = DayStats::new(data, &view);
    let results = if op_error.is_some() {
        Vec::new()
    } else {
        workflow
            .analyses
            .iter()
            .map(|step| {
                let mut request = step.request.clone();
                if let Value::Object(fields) = &mut request {
                    fields.insert(DATASET_FIELD.into(), Value::from(id.0));
                }
                match analyses::run(Prepared { data, view: &view, stats: &stats }, &step.analysis, request) {
                    Ok(result) => WorkflowResultDto { analysis: step.analysis.clone(), result: Some(result), error: None },
                    Err(error) => WorkflowResultDto { analysis: step.analysis.clone(), result: None, error: Some(error) },
                }
            })
            .collect()
    };
    let mut state = EditStateDto::new(data, &stats.summaries(), history);
    state.values_changed = values_changed;
    WorkflowOutcomeDto { dataset: id, name: data.name.clone(), ops_applied, op_error, results, state }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wind_core::import;

    /// One day of ten-minute speeds, with a logger error code at 06:00.
    fn dataset() -> Dataset {
        let mut csv = String::from("t,ws\n");
        for step in 0..144 {
            let speed = if step == 36 { 99.0 } else { 5.0 + (step % 7) as f64 };
            csv += &format!("2023-01-01 {:02}:{:02},{speed}\n", step / 6, (step % 6) * 10);
        }
        let (mut data, _) = import::import_text("torre.csv", &csv, &Default::default()).unwrap();
        crate::requests::finish_import(&mut data, None, None).unwrap();
        data
    }

    fn workflow(ops: Value) -> WorkflowDto {
        serde_json::from_value(json!({
            "format": WORKFLOW_FORMAT,
            "ops": ops,
            "analyses": [{"analysis": "histogram", "request": {"dataset": 9, "column": "ws", "binWidth": 1}}, {"analysis": "spectrum", "request": {}}],
        }))
        .unwrap()
    }

    #[test]
    fn edits_then_analyses_with_each_outcome() {
        let rule = json!({"op": "flag_by_rules", "rules": [{"criterion": {"kind": "outside_range", "min": 0, "max": 50}, "columns": ["ws"], "flag": 0, "minSteps": 1}]});
        let (mut data, mut history) = (dataset(), History::default());
        let outcome = run(DatasetId(4), &mut data, &mut history, &workflow(json!([rule])));
        assert_eq!((outcome.ops_applied, outcome.op_error), (1, None));
        assert_eq!(outcome.state.segments.len(), 1, "the 99 m/s step is flagged");
        let histogram = outcome.results[0].result.as_ref().expect("the histogram ran");
        assert_eq!(histogram["samples"], json!(143), "on the flagged data, whatever dataset the request named");
        assert_eq!(outcome.results[1].error.as_ref().unwrap().code, analyses::UNKNOWN_ANALYSIS, "an unknown analysis fails alone");
        assert!(history.can_undo(), "the edits are undoable");
    }

    #[test]
    fn a_failing_edit_stops_the_dataset() {
        let bad = json!([{"op": "delete_data", "columns": ["ws80"], "fromMs": 0, "toMs": 1}]);
        let (mut data, mut history) = (dataset(), History::default());
        let outcome = run(DatasetId(0), &mut data, &mut history, &workflow(bad));
        assert_eq!((outcome.ops_applied, outcome.results.len()), (0, 0));
        assert_eq!(outcome.op_error.unwrap().code, "unknown_column");
        let mut other = workflow(json!([]));
        other.format = WORKFLOW_FORMAT + 1;
        assert_eq!(check(&other).unwrap_err().code, OTHER_WORKFLOW_FORMAT);
    }
}

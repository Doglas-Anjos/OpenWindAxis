//! Browser binding of wind-core, speaking the wind-api contract: every method
//! takes its request as JSON and returns a response DTO as JSON; bulk numbers
//! (chart series) cross as Float64Array. A failure reaches JS as an `Error`
//! whose message is an `ErrorDto` in JSON (`{ code, message }`).
#![warn(missing_docs)]

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::ops::Range;
use wasm_bindgen::prelude::*;
use wind_api::requests::*;
use wind_api::responses::{self, DatasetDto, EditStateDto, ImportResultDto};
use wind_api::rules::{FlagByRulesDto, FlagByRulesRequest};
use wind_api::flag_statistics::{FlagStatisticsDto, RemoveFlagRequest};
use wind_api::analyses::{self, Prepared};
use wind_api::ops::{ApplyOpRequest, SelectionCountDto, SelectionRequest};
use wind_api::turbulence::{self, TurbulenceRequest};
use wind_api::append;
use wind_api::distribution::{self, SpeedDistributionRequest};
use wind_api::distortion::{self, TowerDistortionRequest};
use wind_api::shear_views::{self, ShearFrequencyRequest, ShearViewRequest};
use wind_api::shading::{self, TowerShadingRequest};
use wind_api::extremes::{self, ExtremesRequest};
use wind_api::means::{self, MonthlyTableRequest, PeriodMeansRequest};
use wind_api::scatter::{self, ScatterRequest};
use wind_api::recipe::AnalyzeRequest;
use wind_api::workflow::{self, WorkflowRunRequest};
use wind_api::mcp::{self as long_term, McpRequest, McpSeriesRequest};
use wind_api::{DatasetId, ErrorDto, API_VERSION};
use wind_core::error::CoreError;
use wind_core::filter::FilterView;
use wind_core::history::History;
use wind_core::model::Dataset;
use wind_core::ops::Op;
use wind_core::ops::OpReport;
use wind_core::rules::RuleRun;
use wind_core::series::DayStats;
use wind_core::{export, flags, series};

/// Milliseconds in a second: JS times are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;
/// `series` output holds a time and a value per point.
const NUMBERS_PER_POINT: usize = 2;
/// Why serializing a DTO cannot fail: plain fields and string keys only.
const DTO_SERIALIZES: &str = "DTOs hold plain fields and string keys only";

/// All datasets of one browser session.
#[wasm_bindgen]
#[derive(Default)]
pub struct Session {
    datasets: Vec<Entry>,
    /// Id the next imported dataset gets; ids are never reused.
    next_id: u32,
}

/// A dataset with its filter and per-day summaries, kept in step on every
/// edit so analyses never rebuild them, and its undo history. A re-import
/// starts a new history: the old revisions describe another grid.
struct Entry {
    id: DatasetId,
    data: Dataset,
    view: FilterView,
    stats: DayStats,
    history: History,
}

impl Entry {
    /// The dataset with its kept view and summaries, for the analysis calls.
    fn prepared(&self) -> Prepared<'_> {
        Prepared { data: &self.data, view: &self.view, stats: &self.stats }
    }

    fn new(id: DatasetId, data: Dataset) -> Self {
        let view = FilterView::new(&data);
        let stats = DayStats::new(&data, &view);
        Entry { id, data, view, stats, history: History::default() }
    }

    /// Applies `op` through the history and returns the new state.
    fn edit(&mut self, op: Op) -> Result<String, JsError> {
        self.history.apply(&mut self.data, op.clone()).map_err(core_error)?;
        self.refresh(&op);
        self.state(Some(&op))
    }

    /// Applies `op` through the history, brings the filter and summaries in
    /// step and returns what the op found.
    fn apply(&mut self, op: Op) -> Result<OpReport, JsError> {
        let report = self.history.apply(&mut self.data, op.clone()).map_err(core_error)?;
        self.refresh(&op);
        Ok(report)
    }

    /// Brings the filter and summaries in step after `op` was applied, undone
    /// or redone ([`DayStats::follow`]).
    fn refresh(&mut self, op: &Op) {
        self.view = FilterView::new(&self.data);
        self.stats.follow(&self.data, &self.view, op);
    }

    /// The `EditStateDto` the edit methods return, after `op` was applied,
    /// undone or redone (`None`: nothing was).
    fn state(&self, op: Option<&Op>) -> Result<String, JsError> {
        let mut state = EditStateDto::new(&self.data, &self.stats.summaries(), &self.history);
        state.values_changed = op.is_some_and(Op::changes_values);
        respond(&state)
    }

    /// Index of the column with this id.
    fn column(&self, id: &str) -> Result<usize, JsError> {
        self.data
            .columns
            .iter()
            .position(|column| column.id == id)
            .ok_or_else(|| core_error(CoreError::UnknownColumn { id: id.to_string() }))
    }

    fn columns(&self, ids: &[String]) -> Result<Vec<usize>, JsError> {
        ids.iter().map(|id| self.column(id)).collect()
    }

    /// Steps overlapping the instants `from_ms..=to_ms` (epoch ms); empty for
    /// a reversed or NaN window.
    fn steps_between(&self, from_ms: f64, to_ms: f64) -> Range<usize> {
        if !(from_ms <= to_ms) {
            return 0..0;
        }
        let to_seconds = |ms: f64| (ms / MS_PER_SECOND).floor() as i64;
        self.data.steps_between(to_seconds(from_ms), to_seconds(to_ms))
    }
}

#[wasm_bindgen]
impl Session {
    /// An empty session: `new Session()` in JS.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Session {
        Session::default()
    }

    /// Version of the wire contract; the UI refuses to run on a mismatch.
    #[wasm_bindgen(js_name = apiVersion)]
    pub fn api_version() -> u32 {
        API_VERSION
    }

    /// Imports a delimited text file from its raw bytes (pass
    /// `new Uint8Array(await file.arrayBuffer())`; no UTF-16 round trip).
    /// `request`: `ImportRequest`. Returns `ImportResultDto`.
    #[wasm_bindgen(js_name = importCsv)]
    pub fn import_csv(&mut self, bytes: &[u8], request: &str) -> Result<String, JsError> {
        let request: ImportRequest = parse(request)?;
        let config = request.config.unwrap_or_default().into_core();
        let appended = request.appended.unwrap_or_default();
        let (mut data, report, appended) = append::import_with_appended(&request.name, bytes, &appended, &config).map_err(core_error)?;
        let replacing = request.replace.map(|id| self.entry(id).map(|entry| &entry.data)).transpose()?;
        finish_import(&mut data, request.restore, replacing).map_err(core_error)?;
        // Everything that can fail has run: the session changes only now.
        let id = match request.replace {
            Some(id) => {
                let position = self.position(id)?;
                self.datasets[position] = Entry::new(id, data);
                id
            }
            None => {
                let id = DatasetId(self.next_id);
                self.next_id += 1;
                self.datasets.push(Entry::new(id, data));
                id
            }
        };
        let entry = self.entry(id)?;
        respond(&ImportResultDto {
            dataset: DatasetDto::new(id, &entry.data),
            report: report.into(),
            summaries: responses::summaries(&entry.data, &entry.stats.summaries()),
            appended,
        })
    }

    /// Decimated series for charts. `request`: `SeriesRequest`. Returns
    /// `[t_ms, value, t_ms, value, ...]`, NaN where the data has a gap.
    pub fn series(&self, request: &str) -> Result<Vec<f64>, JsError> {
        let request: SeriesRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let column = entry.column(&request.column)?;
        let steps = entry.steps_between(request.from_ms, request.to_ms);
        let points = series::decimate(&entry.data.columns[column].values, steps, request.buckets as usize);
        let mut packed = Vec::with_capacity(points.len() * NUMBERS_PER_POINT);
        for (step, value) in points {
            packed.push(entry.data.time_at(step) as f64 * MS_PER_SECOND);
            packed.push(value);
        }
        Ok(packed)
    }

    /// `request`: `FlagRequest`. Returns `EditStateDto`.
    pub fn flag(&mut self, request: &str) -> Result<String, JsError> {
        let request: FlagRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let columns = entry.columns(&request.columns)?;
        let steps = entry.steps_between(request.from_ms, request.to_ms);
        entry.edit(Op::Flag { columns, flag: request.flag, steps })
    }

    /// Applies one op (`OpDto`: flags, rules, scale and offset, delete,
    /// time shifts) as one undoable edit. `request`: `ApplyOpRequest`.
    /// Returns `EditStateDto`.
    #[wasm_bindgen(js_name = applyOp)]
    pub fn apply_op(&mut self, request: &str) -> Result<String, JsError> {
        let request: ApplyOpRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let op = request.op.into_core(&entry.data).map_err(core_error)?;
        entry.edit(op)
    }

    /// Replaces the dataset's flags (`DefineFlagsRequest`): segments and
    /// evidence follow their flag; leaving one out or moving one restarts the
    /// undo history, whose revisions speak of the old flags. Filter and
    /// summaries are rebuilt, as a flag's treatment may have changed.
    /// Returns `EditStateDto`.
    #[wasm_bindgen(js_name = defineFlags)]
    pub fn define_flags(&mut self, request: &str) -> Result<String, JsError> {
        let request: DefineFlagsRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        if flags::redefine(&mut entry.data, request.into_core()).map_err(core_error)? {
            entry.history = History::default();
        }
        // A flag now included or excluded changes what the calculated columns read.
        wind_core::calculated::refresh(&mut entry.data);
        entry.view = FilterView::new(&entry.data);
        entry.stats = DayStats::new(&entry.data, &entry.view);
        entry.state(None)
    }

    /// `request`: `UnflagRequest`. Returns `EditStateDto`.
    pub fn unflag(&mut self, request: &str) -> Result<String, JsError> {
        let request: UnflagRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let columns = entry.columns(&request.columns)?;
        let steps = entry.steps_between(request.from_ms, request.to_ms);
        entry.edit(Op::Unflag { columns, flag: request.flag, steps })
    }

    /// Takes back the last edit. `request`: `DatasetRequest`. Returns
    /// `EditStateDto`, unchanged when there is nothing to undo.
    pub fn undo(&mut self, request: &str) -> Result<String, JsError> {
        let request: DatasetRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let op = entry.history.undo(&mut entry.data);
        if let Some(op) = &op {
            entry.refresh(op);
        }
        entry.state(op.as_ref())
    }

    /// Reapplies the last undone edit. `request`: `DatasetRequest`. Returns
    /// `EditStateDto`, unchanged when there is nothing to redo.
    pub fn redo(&mut self, request: &str) -> Result<String, JsError> {
        let request: DatasetRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let op = entry.history.redo(&mut entry.data);
        if let Some(op) = &op {
            entry.refresh(op);
        }
        entry.state(op.as_ref())
    }

    /// `request`: `CoverageRequest`. Returns `CoverageDto`.
    pub fn coverage(&self, request: &str) -> Result<String, JsError> {
        let request: CoverageRequest = parse(request)?;
        respond(&analyses::coverage(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `HistogramRequest`. Returns `HistogramDto`.
    pub fn histogram(&self, request: &str) -> Result<String, JsError> {
        let request: HistogramRequest = parse(request)?;
        respond(&analyses::histogram(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `WindRoseRequest`. Returns `WindRoseDto`.
    #[wasm_bindgen(js_name = windRose)]
    pub fn wind_rose(&self, request: &str) -> Result<String, JsError> {
        let request: WindRoseRequest = parse(request)?;
        respond(&analyses::wind_rose(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `DiurnalRequest`. Returns `DiurnalDto`.
    pub fn diurnal(&self, request: &str) -> Result<String, JsError> {
        let request: DiurnalRequest = parse(request)?;
        respond(&analyses::diurnal(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `MonthlyMeansRequest`. Returns `MonthlyMeansDto`.
    #[wasm_bindgen(js_name = monthlyMeans)]
    pub fn monthly_means(&self, request: &str) -> Result<String, JsError> {
        let request: MonthlyMeansRequest = parse(request)?;
        respond(&analyses::monthly_means(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `PeriodMeansRequest`. Returns `PeriodMeansDto`.
    #[wasm_bindgen(js_name = periodMeans)]
    pub fn period_means(&self, request: &str) -> Result<String, JsError> {
        let request: PeriodMeansRequest = parse(request)?;
        respond(&means::period_means(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `MonthlyTableRequest`. Returns `MonthlyTableDto`.
    #[wasm_bindgen(js_name = monthlyTable)]
    pub fn monthly_table(&self, request: &str) -> Result<String, JsError> {
        let request: MonthlyTableRequest = parse(request)?;
        respond(&means::monthly_table(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `ScatterRequest`. Returns `ScatterDto`.
    pub fn scatter(&self, request: &str) -> Result<String, JsError> {
        let request: ScatterRequest = parse(request)?;
        respond(&scatter::scatter(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// Any analysis by name, through the dispatch recipes replay through.
    /// `request`: `AnalyzeRequest`. Returns the analysis's response DTO.
    pub fn analyze(&self, request: &str) -> Result<String, JsError> {
        let request: AnalyzeRequest = parse(request)?;
        let answer = analyses::run(self.entry(request.dataset)?.prepared(), &request.analysis, request.request);
        respond(&answer.map_err(|error| JsError::new(&error.to_json()))?)
    }

    /// Applies a workflow (edits, then analyses) to some datasets, in order.
    /// `request`: `WorkflowRunRequest`. Returns `WorkflowOutcomeDto[]`.
    #[wasm_bindgen(js_name = runWorkflow)]
    pub fn run_workflow(&mut self, request: &str) -> Result<String, JsError> {
        let request: WorkflowRunRequest = parse(request)?;
        workflow::check(&request.workflow).map_err(|error| JsError::new(&error.to_json()))?;
        // Every dataset is found before any is edited, so a bad id edits
        // nothing; one named twice runs once.
        let mut ids: Vec<DatasetId> = Vec::with_capacity(request.datasets.len());
        for id in request.datasets {
            self.position(id)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut outcomes = Vec::with_capacity(ids.len());
        for id in ids {
            let entry = self.entry_mut(id)?;
            outcomes.push(workflow::run(id, &mut entry.data, &mut entry.history, &request.workflow));
            entry.view = FilterView::new(&entry.data);
            entry.stats = DayStats::new(&entry.data, &entry.view);
        }
        respond(&outcomes)
    }

    /// The long-term adjustment of a site column against a reference column
    /// of another dataset. `request`: `McpRequest`. Returns `McpDto`.
    pub fn mcp(&self, request: &str) -> Result<String, JsError> {
        let request: McpRequest = parse(request)?;
        let (site, reference) = (self.entry(request.site.dataset)?, self.entry(request.reference.dataset)?);
        respond(&long_term::long_term(site.prepared(), reference.prepared(), &request).map_err(core_error)?)
    }

    /// The optimal time offset analysis. `request`: `TimeOffsetRequest`.
    /// Returns `TimeOffsetDto`.
    #[wasm_bindgen(js_name = timeOffset)]
    pub fn time_offset(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::mcp::TimeOffsetRequest = parse(request)?;
        let (site, reference) = (self.entry(request.site.dataset)?, self.entry(request.reference.dataset)?);
        respond(&long_term::time_offset(site.prepared(), reference.prepared(), &request).map_err(core_error)?)
    }

    /// The site's long-term series by one method. `request`:
    /// `McpSeriesRequest`. Returns the CSV text itself.
    #[wasm_bindgen(js_name = mcpSeries)]
    pub fn mcp_series(&self, request: &str) -> Result<String, JsError> {
        let request: McpSeriesRequest = parse(request)?;
        let (site, reference) = (self.entry(request.site.dataset)?, self.entry(request.reference.dataset)?);
        long_term::long_term_series(site.prepared(), reference.prepared(), &request).map_err(core_error)
    }

    /// `request`: `ExtremesRequest`. Returns `ExtremesDto`.
    pub fn extremes(&self, request: &str) -> Result<String, JsError> {
        let request: ExtremesRequest = parse(request)?;
        respond(&extremes::extremes(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `TurbulenceRequest`. Returns `TurbulenceDto`.
    pub fn turbulence(&self, request: &str) -> Result<String, JsError> {
        let request: TurbulenceRequest = parse(request)?;
        respond(&turbulence::turbulence(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `SelectionRequest`. Returns `SelectionCountDto`: how many
    /// steps a scatter selection holds (Flag › Flag por dispersão).
    #[wasm_bindgen(js_name = selectionCount)]
    pub fn selection_count(&self, request: &str) -> Result<String, JsError> {
        let request: SelectionRequest = parse(request)?;
        let runs = request.selection.runs(&self.entry(request.dataset)?.data).map_err(core_error)?;
        respond(&SelectionCountDto { steps: runs.iter().map(ExactSizeIterator::len).sum() })
    }

    /// `request`: `SpeedDistributionRequest`. Returns `SpeedDistributionDto`.
    #[wasm_bindgen(js_name = speedDistribution)]
    pub fn speed_distribution(&self, request: &str) -> Result<String, JsError> {
        let request: SpeedDistributionRequest = parse(request)?;
        respond(&distribution::speed_distribution(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `TowerDistortionRequest`. Returns `TowerDistortionDto`.
    #[wasm_bindgen(js_name = towerDistortion)]
    pub fn tower_distortion(&self, request: &str) -> Result<String, JsError> {
        let request: TowerDistortionRequest = parse(request)?;
        respond(&distortion::tower_distortion(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `DistortionDetailRequest`. Returns `DistortionDetailDto`.
    #[wasm_bindgen(js_name = distortionDetail)]
    pub fn distortion_detail(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::distortion::DistortionDetailRequest = parse(request)?;
        respond(&distortion::distortion_detail(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `ExpressionCheckRequest`. Returns `ExpressionCheckDto`.
    #[wasm_bindgen(js_name = checkExpression)]
    pub fn check_expression(&self, request: &str) -> Result<String, JsError> {
        respond(&wind_api::calculated::check_expression(parse(request)?))
    }

    /// `request`: `ShearViewRequest`. Returns `ShearViewDto`.
    #[wasm_bindgen(js_name = shearView)]
    pub fn shear_view(&self, request: &str) -> Result<String, JsError> {
        let request: ShearViewRequest = parse(request)?;
        respond(&shear_views::shear_view(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `ShearFrequencyRequest`. Returns `ShearFrequencyDto`.
    #[wasm_bindgen(js_name = shearFrequency)]
    pub fn shear_frequency(&self, request: &str) -> Result<String, JsError> {
        let request: ShearFrequencyRequest = parse(request)?;
        respond(&shear_views::shear_frequency(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `TowerShadingRequest`. Returns `TowerShadingDto`.
    #[wasm_bindgen(js_name = towerShading)]
    pub fn tower_shading(&self, request: &str) -> Result<String, JsError> {
        let request: TowerShadingRequest = parse(request)?;
        respond(&shading::tower_shading(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `ShearRequest`. Returns `ShearDto`.
    pub fn shear(&self, request: &str) -> Result<String, JsError> {
        let request: ShearRequest = parse(request)?;
        respond(&analyses::shear(self.entry(request.dataset)?.prepared(), request).map_err(core_error)?)
    }

    /// `request`: `ExportCsvRequest`. Returns the CSV text itself.
    #[wasm_bindgen(js_name = exportCsv)]
    pub fn export_csv(&self, request: &str) -> Result<String, JsError> {
        let request: ExportCsvRequest = parse(request)?;
        export::to_csv(&self.entry(request.dataset)?.data, &request.options()).map_err(core_error)
    }

    /// `request`: `DatasetRequest`. Returns the dataset's `DatasetDto` as
    /// pretty JSON, for download.
    #[wasm_bindgen(js_name = exportJson)]
    pub fn export_json(&self, request: &str) -> Result<String, JsError> {
        let request: DatasetRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        Ok(serde_json::to_string_pretty(&DatasetDto::new(entry.id, &entry.data)).expect(DTO_SERIALIZES))
    }

    /// `request`: `ExportWaspTabRequest`. Returns the `.tab` file text
    /// (docs/formatos/wasp-tab.md).
    #[wasm_bindgen(js_name = exportWaspTab)]
    pub fn export_wasp_tab(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::wasp::ExportWaspTabRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let options = request.options(&entry.data).map_err(core_error)?;
        export::wasp_tab::to_tab(&entry.data, &entry.view, &options).map_err(core_error)
    }

    /// The EPE file of a dataset. `request`: `ExportEpeRequest`. Returns
    /// `EpeFileDto` (the file's text and the name EPE asks for).
    #[wasm_bindgen(js_name = exportEpe)]
    pub fn export_epe(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::epe::ExportEpeRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let options = request.options(&entry.data).map_err(core_error)?;
        let text = export::epe::to_epe(&entry.data, &entry.view, &options).map_err(core_error)?;
        respond(&wind_api::epe::EpeFileDto { name: export::epe::file_name(&entry.data, &options), text })
    }

    /// `request`: `ExportOpenwindRequest`. Returns `OpenwindFileDto`.
    #[wasm_bindgen(js_name = exportOpenwind)]
    pub fn export_openwind(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::openwind::ExportOpenwindRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let options = request.scaled_options(&entry.data, &entry.view).map_err(core_error)?;
        let text = export::openwind::to_openwind_mm(&entry.data, &entry.view, &options).map_err(core_error)?;
        respond(&wind_api::openwind::OpenwindFileDto { name: wind_api::openwind::file_name(&entry.data), text })
    }

    /// The original's three means of what an Openwind file would export.
    /// `request`: `ExportOpenwindRequest`. Returns `OpenwindMeansDto`.
    #[wasm_bindgen(js_name = openwindMeans)]
    pub fn openwind_means(&self, request: &str) -> Result<String, JsError> {
        let request: wind_api::openwind::ExportOpenwindRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let options = request.options(&entry.data).map_err(core_error)?;
        let means = export::openwind::openwind_means(&entry.data, &entry.view, &options).map_err(core_error)?;
        respond(&wind_api::openwind::OpenwindMeansDto::from(means))
    }

    /// Drops a dataset. `request`: `DatasetRequest`.
    pub fn remove(&mut self, request: &str) -> Result<(), JsError> {
        let request: DatasetRequest = parse(request)?;
        let position = self.position(request.dataset)?;
        self.datasets.remove(position);
        Ok(())
    }

    /// Runs flag rules over the whole record as one undoable edit.
    /// `request`: `FlagByRulesRequest`. Returns `FlagByRulesDto`.
    #[wasm_bindgen(js_name = flagByRules)]
    pub fn flag_by_rules(&mut self, request: &str) -> Result<String, JsError> {
        let request: FlagByRulesRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let rules = request.rules.into_iter().map(|rule| rule.into_core(&entry.data)).collect::<Result<Vec<_>, _>>();
        let run = RuleRun::new(rules.map_err(core_error)?, 0..entry.data.len).map_err(core_error)?;
        let report = entry.apply(Op::FlagByRules(run.clone()))?;
        let tallies = wind_core::rules::tally(&run, &report.findings);
        let state = EditStateDto::new(&entry.data, &entry.stats.summaries(), &entry.history);
        respond(&FlagByRulesDto::new(&entry.data, state, &tallies, &report.findings))
    }

    /// What flag rules would flag, without flagging it ("Testar", the
    /// original's Test Selected Rules). `request`: `FlagByRulesRequest`.
    /// Returns `FlagByRulesDto` with the dataset's state unchanged.
    #[wasm_bindgen(js_name = testRules)]
    pub fn test_rules(&self, request: &str) -> Result<String, JsError> {
        let request: FlagByRulesRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        let rules = request.rules.into_iter().map(|rule| rule.into_core(&entry.data)).collect::<Result<Vec<_>, _>>();
        let run = RuleRun::new(rules.map_err(core_error)?, 0..entry.data.len).map_err(core_error)?;
        let findings = wind_core::rules::search(&entry.data, &entry.view, &run).map_err(core_error)?;
        let tallies = wind_core::rules::tally(&run, &findings);
        let state = EditStateDto::new(&entry.data, &entry.stats.summaries(), &entry.history);
        respond(&FlagByRulesDto::new(&entry.data, state, &tallies, &findings))
    }

    /// What a project saves for a dataset: flags, segments and rule
    /// evidence. `request`: `DatasetRequest`. Returns `RestoreDto`.
    #[wasm_bindgen(js_name = projectState)]
    pub fn project_state(&self, request: &str) -> Result<String, JsError> {
        let request: DatasetRequest = parse(request)?;
        respond(&RestoreDto::new(&self.entry(request.dataset)?.data))
    }

    /// Flagged steps per flag and column, and every flagged segment with its
    /// rule evidence. `request`: `DatasetRequest`. Returns `FlagStatisticsDto`.
    #[wasm_bindgen(js_name = flagStatistics)]
    pub fn flag_statistics(&self, request: &str) -> Result<String, JsError> {
        let request: DatasetRequest = parse(request)?;
        let entry = self.entry(request.dataset)?;
        respond(&FlagStatisticsDto::new(&entry.data, &flags::statistics(&entry.data)))
    }

    /// Removes one flag over the whole record, from some columns or all, as
    /// one undoable edit. `request`: `RemoveFlagRequest`. Returns `EditStateDto`.
    #[wasm_bindgen(js_name = removeFlag)]
    pub fn remove_flag(&mut self, request: &str) -> Result<String, JsError> {
        let request: RemoveFlagRequest = parse(request)?;
        let entry = self.entry_mut(request.dataset)?;
        let columns = request.columns.as_deref().map(|ids| entry.columns(ids)).transpose()?;
        let op = Op::remove_flag(&entry.data, request.flag, columns.as_deref()).map_err(core_error)?;
        entry.edit(op)
    }
}

impl Session {
    fn position(&self, id: DatasetId) -> Result<usize, JsError> {
        self.datasets
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| core_error(CoreError::NoSuchDataset { dataset: id.0 as usize }))
    }

    fn entry(&self, id: DatasetId) -> Result<&Entry, JsError> {
        Ok(&self.datasets[self.position(id)?])
    }

    fn entry_mut(&mut self, id: DatasetId) -> Result<&mut Entry, JsError> {
        let position = self.position(id)?;
        Ok(&mut self.datasets[position])
    }
}

fn parse<T: DeserializeOwned>(request: &str) -> Result<T, JsError> {
    serde_json::from_str(request).map_err(|error| JsError::new(&ErrorDto::bad_request(error).to_json()))
}

fn respond<T: Serialize>(response: &T) -> Result<String, JsError> {
    Ok(serde_json::to_string(response).expect(DTO_SERIALIZES))
}

fn core_error(error: CoreError) -> JsError {
    JsError::new(&ErrorDto::from(&error).to_json())
}

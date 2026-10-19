//! `wind`, the command line adapter of wind-core (ARQUITETURA §1, C1, C2).
//! It speaks the browser's contract: requests are wind-api DTOs built from
//! files and `name=value` pairs, responses go to standard output as the same
//! JSON the browser receives, and a failure goes to standard error as an
//! `ErrorDto` with a non-zero exit code. Like every adapter it does no
//! calculation: it reads files, calls the core and converts DTOs.
#![warn(missing_docs)]

mod args;
mod error;

use args::{Command, ExportFormat, Invocation, Parsed};
use error::CliError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use wind_api::analyses::{self, Prepared};
use wind_api::append::{self, AppendStatusDto, AppendedSourceDto};
use wind_api::ops::OpDto;
use wind_api::recipe::{self, RecipeDto};
use wind_api::workflow::{self, WorkflowDto, WorkflowOutcomeDto};
use wind_api::project::{read_project, PROJECT_MAGIC};
use wind_api::requests::*;
use wind_api::responses::{self, DatasetDto, EditStateDto, ImportResultDto};
use wind_api::epe::ExportEpeRequest;
use wind_api::wasp::ExportWaspTabRequest;
use wind_api::DatasetId;
use wind_core::error::CoreError;
use wind_core::filter::FilterView;
use wind_core::history::History;
use wind_core::import::ImportReport;
use wind_core::model::Dataset;
use wind_core::series::DayStats;
use wind_core::export;

/// Output path that means standard output.
const STANDARD_OUTPUT: &str = "-";
/// Request field naming the dataset; filled with [`FIRST_DATASET`] when absent.
const DATASET_FIELD: &str = "dataset";
/// The dataset a request without [`DATASET_FIELD`] refers to.
const FIRST_DATASET: u32 = 0;
/// Request field of the CSV delimiter, and its default on the command line.
const DELIMITER_FIELD: &str = "delimiter";
const DEFAULT_DELIMITER: &str = ",";
/// Why serializing a DTO cannot fail: plain fields and string keys only.
const DTO_SERIALIZES: &str = "DTOs hold plain fields and string keys only";

fn main() -> ExitCode {
    // `args()` would panic on an argument that is not Unicode; a mangled
    // path fails later as `read_failed` instead.
    let arguments = std::env::args_os().skip(1).map(|argument| argument.to_string_lossy().into_owned());
    let outcome = args::parse(arguments).and_then(|parsed| match parsed {
        Parsed::Help(text) => write_output(STANDARD_OUTPUT.as_ref(), text.as_bytes()),
        Parsed::Run(invocation) => run(&invocation),
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Nothing better to do if standard error itself is gone.
            let _ = writeln!(std::io::stderr(), "{}", error.to_dto().to_json());
            error.exit_code()
        }
    }
}

/// A dataset loaded from the input, as the browser's session keeps it.
struct Loaded {
    id: DatasetId,
    data: Dataset,
    report: ImportReport,
    history: History,
    /// Whether an op of `--ops` changed values, not only flags.
    values_changed: bool,
    /// The source file's name and bytes and how it was read, for recipes.
    name: String,
    source: Vec<u8>,
    config: ImportConfigDto,
    /// Files appended to the source, whose bytes follow its own in `source`.
    appended: Vec<AppendedSourceDto>,
    /// What became of each appended file.
    append_statuses: Vec<AppendStatusDto>,
}

impl Loaded {
    /// Reads a source file and puts back saved flags, as the browser's
    /// `importCsv` does.
    fn import(
        id: DatasetId,
        name: &str,
        bytes: &[u8],
        appended: Vec<AppendedSourceDto>,
        config: ImportConfigDto,
        restore: Option<RestoreDto>,
    ) -> Result<Self, CoreError> {
        let (mut data, report, append_statuses) = append::import_with_appended(name, bytes, &appended, &config.clone().into_core())?;
        finish_import(&mut data, restore, None)?;
        let (source, history, name) = (bytes.to_vec(), History::default(), name.to_string());
        Ok(Loaded { id, data, report, history, values_changed: false, name, source, config, appended, append_statuses })
    }

    /// The filter and per-day summaries of the current flags.
    fn filtered(&self) -> (FilterView, DayStats) {
        let view = FilterView::new(&self.data);
        let stats = DayStats::new(&self.data, &view);
        (view, stats)
    }

    /// What the browser's `importCsv` answers for this dataset.
    fn import_result(&self) -> ImportResultDto {
        let (_, stats) = self.filtered();
        ImportResultDto {
            dataset: DatasetDto::new(self.id, &self.data),
            report: self.report.clone().into(),
            summaries: responses::summaries(&self.data, &stats.summaries()),
            appended: self.append_statuses.clone(),
        }
    }
}

/// Runs a parsed command line. `import` reads its input as CSV and
/// `project` as a project; the other commands accept either, telling them
/// apart by the project magic.
fn run(invocation: &Invocation) -> Result<(), CliError> {
    match invocation.command {
        Command::Import => {
            let csv = read_file(&invocation.input)?;
            print(&import_csv(invocation, &csv)?.import_result())
        }
        Command::Project => {
            let file = read_file(&invocation.input)?;
            let results: Vec<ImportResultDto> = open_project(&file)?.iter().map(Loaded::import_result).collect();
            print(&results)
        }
        Command::Replay => {
            let recipe: RecipeDto = read_json(&invocation.input)?;
            let source = read_file(invocation.out.as_deref().expect("replay always has its source"))?;
            let report = recipe::replay(recipe, &source).map_err(CliError::Api)?;
            print(&report)?;
            if report.source_matches && report.result_matches {
                Ok(())
            } else {
                Err(CliError::ReplayMismatch)
            }
        }
        Command::Run => {
            let plan: WorkflowDto = read_json(invocation.workflow.as_deref().expect("run always has its workflow"))?;
            workflow::check(&plan).map_err(CliError::Api)?;
            let inputs: Vec<&Path> = std::iter::once(invocation.input.as_path()).chain(invocation.more_inputs.iter().map(PathBuf::as_path)).collect();
            let mut outcomes = Vec::new();
            for result in in_parallel(&inputs, |path| run_workflow(invocation, path, &plan)) {
                outcomes.extend(result?);
            }
            // Each input numbers its datasets from 0: number them across the inputs.
            for (number, outcome) in (FIRST_DATASET..).zip(&mut outcomes) {
                outcome.dataset = DatasetId(number);
            }
            print(&outcomes)
        }
        Command::Apply | Command::Export | Command::Analyze | Command::Recipe => {
            let file = read_file(&invocation.input)?;
            let mut datasets = if file.starts_with(PROJECT_MAGIC.as_bytes()) {
                if invocation.config.is_some() {
                    return Err(CliError::ConfigForProject);
                }
                open_project(&file)?
            } else {
                vec![import_csv(invocation, &file)?]
            };
            match (invocation.command, invocation.analysis, invocation.export.as_ref()) {
                (Command::Analyze | Command::Recipe, Some(analysis), _) => analyze(invocation, &mut datasets, analysis),
                (Command::Export, _, Some((format, out))) => export_to(invocation, &mut datasets, *format, out),
                _ => {
                    let request: DatasetRequest = request(&invocation.parameters)?;
                    let loaded = edited(invocation, &mut datasets, request.dataset)?;
                    let (_, stats) = loaded.filtered();
                    let mut state = EditStateDto::new(&loaded.data, &stats.summaries(), &loaded.history);
                    state.values_changed = loaded.values_changed;
                    print(&state)
                }
            }
        }
    }
}

/// The input as a CSV, read with `--config` when given.
fn import_csv(invocation: &Invocation, csv: &[u8]) -> Result<Loaded, CliError> {
    import_csv_at(invocation, &invocation.input, csv)
}

/// The CSV `csv`, read from `path`, with `--config` when given.
fn import_csv_at(invocation: &Invocation, path: &Path, csv: &[u8]) -> Result<Loaded, CliError> {
    let config = match &invocation.config {
        Some(path) => read_json(path)?,
        None => ImportConfigDto::default(),
    };
    let file_name = |path: &Path| path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut bytes = csv.to_vec();
    let mut appended = Vec::with_capacity(invocation.appended.len());
    for path in &invocation.appended {
        let more = read_file(path)?;
        appended.push(AppendedSourceDto { name: file_name(path), bytes: more.len() });
        bytes.extend_from_slice(&more);
    }
    Ok(Loaded::import(DatasetId(FIRST_DATASET), &file_name(path), &bytes, appended, config, None)?)
}

/// `plan` applied to every dataset of the CSV or project at `path`.
fn run_workflow(invocation: &Invocation, path: &Path, plan: &WorkflowDto) -> Result<Vec<WorkflowOutcomeDto>, CliError> {
    let file = read_file(path)?;
    let mut datasets = if file.starts_with(PROJECT_MAGIC.as_bytes()) {
        if invocation.config.is_some() {
            return Err(CliError::ConfigForProject);
        }
        open_project(&file)?
    } else {
        vec![import_csv_at(invocation, path, &file)?]
    };
    Ok(datasets.iter_mut().map(|loaded| workflow::run(loaded.id, &mut loaded.data, &mut loaded.history, plan)).collect())
}

/// `work` on each of `items`, on as many threads as the machine has
/// processors (no more than items), each thread taking the next item when
/// it is free; the answers in the order of `items`.
fn in_parallel<Item: Sync, Answer: Send>(items: &[Item], work: impl Fn(&Item) -> Answer + Sync) -> Vec<Answer> {
    let threads = std::thread::available_parallelism().map_or(1, NonZeroUsize::get).min(items.len());
    let next = AtomicUsize::new(0);
    let mut answered: Vec<(usize, Answer)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else { break done };
                        done.push((index, work(item)));
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|worker| worker.join().expect("the core does not panic on a workflow")).collect()
    });
    answered.sort_by_key(|(index, _)| *index);
    answered.into_iter().map(|(_, answer)| answer).collect()
}

/// Every dataset of a project file, re-imported with its saved configuration
/// and flags; ids follow the project's order.
fn open_project(file: &[u8]) -> Result<Vec<Loaded>, CliError> {
    let mut datasets = Vec::new();
    for (id, source) in (FIRST_DATASET..).zip(read_project(file)?) {
        datasets.push(Loaded::import(DatasetId(id), &source.name, source.bytes, source.appended, source.config, Some(source.restore))?);
    }
    Ok(datasets)
}

/// Dataset `id`, after the `--ops` list (if any) was applied to it.
fn edited<'session>(invocation: &Invocation, datasets: &'session mut [Loaded], id: DatasetId) -> Result<&'session mut Loaded, CliError> {
    let loaded = datasets
        .iter_mut()
        .find(|loaded| loaded.id == id)
        .ok_or(CoreError::NoSuchDataset { dataset: id.0 as usize })?;
    if let Some(path) = &invocation.ops {
        let ops: Vec<OpDto> = read_json(path)?;
        for op in ops {
            let op = op.into_core(&loaded.data)?;
            loaded.values_changed |= op.changes_values();
            loaded.history.apply(&mut loaded.data, op)?;
        }
    }
    Ok(loaded)
}

/// Runs one analysis through the dispatch the browser's recipes use and
/// prints its response DTO, or for `recipe` writes the recipe of it.
fn analyze(invocation: &Invocation, datasets: &mut [Loaded], analysis: &str) -> Result<(), CliError> {
    let mut fields = invocation.parameters.clone();
    fields.entry(DATASET_FIELD).or_insert_with(|| Value::from(FIRST_DATASET));
    let id: DatasetId = serde_json::from_value(fields[DATASET_FIELD].clone()).map_err(|error| CliError::BadRequest { detail: error.to_string() })?;
    let loaded = edited(invocation, datasets, id)?;
    let (view, stats) = loaded.filtered();
    let request = Value::Object(fields);
    let answer = analyses::run(Prepared { data: &loaded.data, view: &view, stats: &stats }, analysis, request.clone()).map_err(CliError::Api)?;
    match (invocation.command, invocation.out.as_deref()) {
        (Command::Recipe, Some(out)) => {
            // ponytail: a recipe names one source file; appended files would need a list there.
            if !loaded.appended.is_empty() {
                return Err(CliError::BadRequest { detail: "a recipe holds one source file; this dataset has appended files".into() });
            }
            let restore = RestoreDto::new(&loaded.data);
            let recipe = RecipeDto::new(&loaded.name, &loaded.source, loaded.config.clone(), restore, analysis, request, answer);
            write_output(out, serde_json::to_string_pretty(&recipe).expect(DTO_SERIALIZES).as_bytes())
        }
        _ => print(&answer),
    }
}

/// Writes one export to `out`.
fn export_to(invocation: &Invocation, datasets: &mut [Loaded], format: ExportFormat, out: &Path) -> Result<(), CliError> {
    let contents = match format {
        ExportFormat::Csv => {
            let mut parameters = invocation.parameters.clone();
            parameters.entry(DELIMITER_FIELD).or_insert_with(|| Value::String(DEFAULT_DELIMITER.into()));
            let request: ExportCsvRequest = request(&parameters)?;
            let loaded = edited(invocation, datasets, request.dataset)?;
            export::to_csv(&loaded.data, &request.options())?
        }
        ExportFormat::Json => {
            let request: DatasetRequest = request(&invocation.parameters)?;
            let loaded = edited(invocation, datasets, request.dataset)?;
            serde_json::to_string_pretty(&DatasetDto::new(loaded.id, &loaded.data)).expect(DTO_SERIALIZES)
        }
        ExportFormat::WaspTab => {
            let request: ExportWaspTabRequest = request(&invocation.parameters)?;
            let loaded = edited(invocation, datasets, request.dataset)?;
            let options = request.options(&loaded.data)?;
            let (view, _) = loaded.filtered();
            export::wasp_tab::to_tab(&loaded.data, &view, &options)?
        }
        ExportFormat::Openwind => {
            let request: wind_api::openwind::ExportOpenwindRequest = request(&invocation.parameters)?;
            let loaded = edited(invocation, datasets, request.dataset)?;
            let (view, _) = loaded.filtered();
            let options = request.scaled_options(&loaded.data, &view)?;
            export::openwind::to_openwind_mm(&loaded.data, &view, &options)?
        }
        ExportFormat::Epe => {
            let request: ExportEpeRequest = request(&invocation.parameters)?;
            let loaded = edited(invocation, datasets, request.dataset)?;
            let options = request.options(&loaded.data)?;
            let (view, _) = loaded.filtered();
            export::epe::to_epe(&loaded.data, &view, &options)?
        }
    };
    write_output(out, contents.as_bytes())
}

/// The request DTO `T` from `name=value` pairs, with [`DATASET_FIELD`]
/// defaulting to [`FIRST_DATASET`].
fn request<T: DeserializeOwned>(parameters: &Map<String, Value>) -> Result<T, CliError> {
    let mut fields = parameters.clone();
    fields.entry(DATASET_FIELD).or_insert_with(|| Value::from(FIRST_DATASET));
    serde_json::from_value(Value::Object(fields)).map_err(|error| CliError::BadRequest { detail: error.to_string() })
}

/// The whole file at `path`.
fn read_file(path: &Path) -> Result<Vec<u8>, CliError> {
    std::fs::read(path).map_err(|error| CliError::ReadFailed { path: path.display().to_string(), detail: error.to_string() })
}

/// A JSON file as the DTO `T`.
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, CliError> {
    serde_json::from_slice(&read_file(path)?).map_err(|error| CliError::BadRequest { detail: format!("{}: {error}", path.display()) })
}

/// A response DTO as one line of JSON on standard output.
fn print<T: Serialize>(response: &T) -> Result<(), CliError> {
    let mut json = serde_json::to_vec(response).expect(DTO_SERIALIZES);
    json.push(b'\n');
    write_output(STANDARD_OUTPUT.as_ref(), &json)
}

/// Writes `contents` to the file `out`, or to standard output for `-`.
fn write_output(out: &Path, contents: &[u8]) -> Result<(), CliError> {
    let failed = |error: std::io::Error| CliError::WriteFailed { path: out.display().to_string(), detail: error.to_string() };
    if out == Path::new(STANDARD_OUTPUT) {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(contents).and_then(|()| stdout.flush()).map_err(failed)
    } else {
        std::fs::write(out, contents).map_err(failed)
    }
}

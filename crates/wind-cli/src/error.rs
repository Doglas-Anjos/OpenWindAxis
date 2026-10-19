//! Every way a `wind` run can fail. Core failures keep the core's code; the
//! command line adds its own, for usage mistakes and file access, each with
//! a stable snake_case [`CliError::code`] and its English text in one
//! `Display`, like `CoreError` (ARQUITETURA §2.4).
use std::fmt;
use std::process::ExitCode;
use wind_api::{ErrorDto, BAD_REQUEST};
use wind_core::error::CoreError;

/// Exit code of a run that failed on its data or files.
const EXIT_FAILURE: u8 = 1;
/// Exit code of a command line that could not be understood (the usual
/// convention for usage errors).
const EXIT_USAGE: u8 = 2;

/// A failed run.
#[derive(Debug)]
pub enum CliError {
    // ---- Usage ----
    /// A command `wind` does not have.
    UnknownCommand { command: String },
    /// An analysis `wind analyze` does not have.
    UnknownAnalysis { analysis: String },
    /// An option `wind` does not have.
    UnknownOption { option: String },
    /// An option given as the last argument, without its value.
    MissingValue { option: String },
    /// A required positional argument was not given.
    MissingArgument { argument: &'static str },
    /// An argument this command does not take.
    UnexpectedArgument { argument: String },
    /// `wind export` needs exactly one output format.
    ExportFormatCount,
    /// `wind apply` needs `--ops`.
    OpsRequired,
    /// `wind recipe` needs `--out`.
    OutRequired,
    /// `wind run` needs `--workflow`.
    WorkflowRequired,
    /// `--config` describes a CSV; a project carries its own.
    ConfigForProject,

    // ---- Files ----
    /// A file could not be read.
    ReadFailed { path: String, detail: String },
    /// A file (or standard output) could not be written.
    WriteFailed { path: String, detail: String },

    // ---- Contract and core ----
    /// A request, configuration or op list that is not valid JSON for its DTO.
    BadRequest { detail: String },
    /// The core refused the data or the request.
    Core(CoreError),
    /// A failure the shared analysis dispatch or the replay already put in
    /// the contract's form.
    Api(ErrorDto),
    /// A replay that did not reproduce its recipe (another file, or another
    /// answer).
    ReplayMismatch,
}

impl CliError {
    /// Stable identifier, safe to match on in scripts.
    pub fn code(&self) -> &'static str {
        use CliError::*;
        match self {
            UnknownCommand { .. } => "unknown_command",
            UnknownAnalysis { .. } => "unknown_analysis",
            UnknownOption { .. } => "unknown_option",
            MissingValue { .. } => "missing_value",
            MissingArgument { .. } => "missing_argument",
            UnexpectedArgument { .. } => "unexpected_argument",
            ExportFormatCount => "export_format_count",
            OpsRequired => "ops_required",
            OutRequired => "out_required",
            WorkflowRequired => "workflow_required",
            ConfigForProject => "config_for_project",
            ReadFailed { .. } => "read_failed",
            WriteFailed { .. } => "write_failed",
            BadRequest { .. } => BAD_REQUEST,
            Core(error) => error.code(),
            Api(_) => "api_error",
            ReplayMismatch => "replay_mismatch",
        }
    }

    /// Usage errors exit with [`EXIT_USAGE`], everything else with
    /// [`EXIT_FAILURE`].
    pub fn exit_code(&self) -> ExitCode {
        use CliError::*;
        let usage = matches!(
            self,
            UnknownCommand { .. }
                | UnknownAnalysis { .. }
                | UnknownOption { .. }
                | MissingValue { .. }
                | MissingArgument { .. }
                | UnexpectedArgument { .. }
                | ExportFormatCount
                | OpsRequired
                | OutRequired
                | WorkflowRequired
                | ConfigForProject
        );
        ExitCode::from(if usage { EXIT_USAGE } else { EXIT_FAILURE })
    }

    /// The failure as the browser would receive it.
    pub fn to_dto(&self) -> ErrorDto {
        match self {
            CliError::Core(error) => ErrorDto::from(error),
            CliError::BadRequest { detail } => ErrorDto::bad_request(detail),
            CliError::Api(dto) => dto.clone(),
            other => ErrorDto { code: other.code().into(), message: other.to_string() },
        }
    }
}

impl From<CoreError> for CliError {
    fn from(error: CoreError) -> Self {
        CliError::Core(error)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CliError::*;
        match self {
            UnknownCommand { command } => write!(formatter, "unknown command {command:?}; see 'wind --help'"),
            UnknownAnalysis { analysis } => {
                write!(formatter, "unknown analysis {analysis:?}; see 'wind analyze --help'")
            }
            UnknownOption { option } => write!(formatter, "unknown option {option:?}"),
            MissingValue { option } => write!(formatter, "option {option} needs a value"),
            MissingArgument { argument } => write!(formatter, "missing argument <{argument}>"),
            UnexpectedArgument { argument } => write!(formatter, "this command does not take {argument:?}"),
            ExportFormatCount => write!(formatter, "export needs exactly one of --csv, --json or --wasp-tab"),
            OpsRequired => write!(formatter, "apply needs --ops <file.json>"),
            OutRequired => write!(formatter, "recipe needs --out <recipe.json>"),
            WorkflowRequired => write!(formatter, "run needs --workflow <workflow.json>"),
            ConfigForProject => write!(formatter, "--config is for CSV input; a project keeps its own configuration"),
            ReadFailed { path, detail } => write!(formatter, "could not read {path}: {detail}"),
            WriteFailed { path, detail } => write!(formatter, "could not write {path}: {detail}"),
            BadRequest { detail } => write!(formatter, "invalid request: {detail}"),
            Core(error) => write!(formatter, "{error}"),
            Api(dto) => write!(formatter, "{}", dto.message),
            ReplayMismatch => write!(formatter, "the replay did not reproduce the recipe: see the report on standard output"),
        }
    }
}

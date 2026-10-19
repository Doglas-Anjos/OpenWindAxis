//! The command line, parsed by hand (no dependency):
//! `wind <command> <input> [analysis] [--option value ...] [name=value ...]`,
//! and the help texts. `name=value` pairs fill the command's request DTO,
//! the same JSON the browser sends, so the CLI needs no flag per field.
use crate::error::CliError;
use wind_api::analyses::ANALYSIS_NAMES;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Prefix of every option.
const OPTION_PREFIX: &str = "--";
/// Asks for help, anywhere on the line.
const HELP_FLAGS: [&str; 2] = ["-h", "--help"];
/// Separates a request field from its value.
const PARAMETER_SEPARATOR: char = '=';

/// Option names, without [`OPTION_PREFIX`].
const CONFIG_OPTION: &str = "config";
const APPEND_OPTION: &str = "append";
const OPS_OPTION: &str = "ops";
const CSV_OPTION: &str = "csv";
const JSON_OPTION: &str = "json";
const WASP_TAB_OPTION: &str = "wasp-tab";
const EPE_OPTION: &str = "epe";
const OPENWIND_OPTION: &str = "openwind";
const OUT_OPTION: &str = "out";
const WORKFLOW_OPTION: &str = "workflow";

/// Names of the positional arguments, as the help and the errors show them.
const INPUT_ARGUMENT: &str = "input";
const ANALYSIS_ARGUMENT: &str = "analysis";
const RECIPE_ARGUMENT: &str = "recipe";
const SOURCE_ARGUMENT: &str = "source";

/// What `wind` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Read a delimited text file.
    Import,
    /// Open a project file.
    Project,
    /// Apply a list of ops and show the state.
    Apply,
    /// Write an export file.
    Export,
    /// Run one analysis.
    Analyze,
    /// Run one analysis and write its reproducible recipe.
    Recipe,
    /// Replay a recipe on a source file and compare.
    Replay,
    /// Apply a workflow to every dataset of the input.
    Run,
}

/// Each command with its name and help, in help order.
const COMMANDS: [(&str, Command, &str); 8] = [
    ("import", Command::Import, HELP_IMPORT),
    ("project", Command::Project, HELP_PROJECT),
    ("apply", Command::Apply, HELP_APPLY),
    ("export", Command::Export, HELP_EXPORT),
    ("analyze", Command::Analyze, HELP_ANALYZE),
    ("recipe", Command::Recipe, HELP_RECIPE),
    ("replay", Command::Replay, HELP_REPLAY),
    ("run", Command::Run, HELP_RUN),
];

impl Command {
    /// The command's own help text.
    fn help(self) -> &'static str {
        COMMANDS.iter().find(|(_, command, _)| *command == self).map_or(HELP, |(_, _, help)| help)
    }

    /// Names of the positional arguments after the command name, in order.
    fn positionals(self) -> &'static [&'static str] {
        match self {
            Command::Analyze | Command::Recipe => &[INPUT_ARGUMENT, ANALYSIS_ARGUMENT],
            Command::Replay => &[RECIPE_ARGUMENT, SOURCE_ARGUMENT],
            _ => &[INPUT_ARGUMENT],
        }
    }
}

/// The file formats of `wind export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// The time series as CSV.
    Csv,
    /// The dataset's metadata (`DatasetDto`) as JSON.
    Json,
    /// A WAsP observed wind climate (`.tab`).
    WaspTab,
    /// The EPE format of Brazilian auctions.
    Epe,
    /// Openwind's met mast file (MM 2.1).
    Openwind,
}

/// A parsed command line.
#[derive(Debug)]
pub struct Invocation {
    /// What to do.
    pub command: Command,
    /// The CSV or project file.
    pub input: PathBuf,
    /// `analyze` and `recipe`: the analysis, one of
    /// `wind_api::analyses::ANALYSIS_NAMES`.
    pub analysis: Option<&'static str>,
    /// `recipe`: where to write it (`-` = standard output). `replay`: the
    /// source file.
    pub out: Option<PathBuf>,
    /// `run`: the workflow file.
    pub workflow: Option<PathBuf>,
    /// `run`: the inputs after the first, run alongside it.
    pub more_inputs: Vec<PathBuf>,
    /// `ImportConfigDto` file for a CSV input.
    pub config: Option<PathBuf>,
    /// Files appended to a CSV input, in order (Arquivo › Anexar dados).
    pub appended: Vec<PathBuf>,
    /// `OpDto` list file.
    pub ops: Option<PathBuf>,
    /// `export` only: the format and where to write it (`-` = standard output).
    pub export: Option<(ExportFormat, PathBuf)>,
    /// Request DTO fields from `name=value` pairs.
    pub parameters: Map<String, Value>,
}

/// What the command line asks for.
#[derive(Debug)]
pub enum Parsed {
    /// Print this help and stop.
    Help(&'static str),
    /// Run this command.
    Run(Invocation),
}

/// Parses the arguments after the program name.
///
/// # Errors
/// A usage variant of [`CliError`].
pub fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Parsed, CliError> {
    let Some(name) = arguments.next() else { return Ok(Parsed::Help(HELP)) };
    if HELP_FLAGS.contains(&name.as_str()) {
        return Ok(Parsed::Help(HELP));
    }
    let command = COMMANDS
        .iter()
        .find(|(command_name, _, _)| *command_name == name)
        .map(|(_, command, _)| *command)
        .ok_or(CliError::UnknownCommand { command: name })?;
    let positional_count = command.positionals().len();
    let mut positionals = Vec::with_capacity(positional_count);
    let (mut config, mut ops, mut out, mut workflow, mut exports) = (None, None, None, None, Vec::new());
    let mut parameters = Map::new();
    let mut more_inputs = Vec::new();
    let mut appended = Vec::new();
    while let Some(argument) = arguments.next() {
        if HELP_FLAGS.contains(&argument.as_str()) {
            return Ok(Parsed::Help(command.help()));
        }
        if let Some(option) = argument.strip_prefix(OPTION_PREFIX) {
            let value = PathBuf::from(arguments.next().ok_or_else(|| CliError::MissingValue { option: argument.clone() })?);
            match option {
                CONFIG_OPTION => config = Some(value),
                APPEND_OPTION => appended.push(value),
                OPS_OPTION => ops = Some(value),
                OUT_OPTION => out = Some(value),
                WORKFLOW_OPTION => workflow = Some(value),
                CSV_OPTION => exports.push((ExportFormat::Csv, value)),
                JSON_OPTION => exports.push((ExportFormat::Json, value)),
                WASP_TAB_OPTION => exports.push((ExportFormat::WaspTab, value)),
                EPE_OPTION => exports.push((ExportFormat::Epe, value)),
                OPENWIND_OPTION => exports.push((ExportFormat::Openwind, value)),
                _ => return Err(CliError::UnknownOption { option: argument }),
            }
            continue;
        }
        if positionals.len() < positional_count {
            positionals.push(argument);
            continue;
        }
        if command == Command::Run && !argument.contains(PARAMETER_SEPARATOR) {
            more_inputs.push(PathBuf::from(argument));
            continue;
        }
        match argument.split_once(PARAMETER_SEPARATOR) {
            Some((field, value)) => {
                parameters.insert(field.to_string(), json_or_text(value));
            }
            None => return Err(CliError::UnexpectedArgument { argument }),
        }
    }

    let mut positionals = positionals.into_iter();
    let input = PathBuf::from(positionals.next().ok_or(CliError::MissingArgument { argument: INPUT_ARGUMENT })?);
    let analysis = match command {
        Command::Analyze | Command::Recipe => {
            let name = positionals.next().ok_or(CliError::MissingArgument { argument: ANALYSIS_ARGUMENT })?;
            let known = ANALYSIS_NAMES.iter().find(|known| **known == name).copied();
            Some(known.ok_or(CliError::UnknownAnalysis { analysis: name })?)
        }
        _ => None,
    };
    match command {
        Command::Replay => {
            out = Some(PathBuf::from(positionals.next().ok_or(CliError::MissingArgument { argument: SOURCE_ARGUMENT })?));
        }
        Command::Recipe if out.is_none() => return Err(CliError::OutRequired),
        Command::Run if workflow.is_none() => return Err(CliError::WorkflowRequired),
        Command::Recipe => {}
        _ if out.is_some() => return Err(CliError::UnexpectedArgument { argument: format!("{OPTION_PREFIX}{OUT_OPTION}") }),
        _ => {}
    }
    if command != Command::Run && workflow.is_some() {
        return Err(CliError::UnexpectedArgument { argument: format!("{OPTION_PREFIX}{WORKFLOW_OPTION}") });
    }
    let reads_only_its_input = matches!(command, Command::Import | Command::Project | Command::Replay | Command::Run);
    let unexpected = |argument: String| Err(CliError::UnexpectedArgument { argument });
    if command == Command::Project && (config.is_some() || !appended.is_empty()) {
        return Err(CliError::ConfigForProject);
    }
    if reads_only_its_input && ops.is_some() {
        return unexpected(format!("{OPTION_PREFIX}{OPS_OPTION}"));
    }
    if let (true, Some(field)) = (reads_only_its_input, parameters.keys().next()) {
        return unexpected(field.clone());
    }
    if command == Command::Apply && ops.is_none() {
        return Err(CliError::OpsRequired);
    }
    let export = match (command, exports.len()) {
        (Command::Export, 1) => exports.pop(),
        (Command::Export, _) => return Err(CliError::ExportFormatCount),
        (_, 0) => None,
        _ => return unexpected(format!("{OPTION_PREFIX}{}", export_option(exports[0].0))),
    };
    Ok(Parsed::Run(Invocation { command, input, analysis, out, workflow, more_inputs, config, appended, ops, export, parameters }))
}

/// The option that selects `format`.
fn export_option(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => CSV_OPTION,
        ExportFormat::Json => JSON_OPTION,
        ExportFormat::WaspTab => WASP_TAB_OPTION,
        ExportFormat::Epe => EPE_OPTION,
        ExportFormat::Openwind => OPENWIND_OPTION,
    }
}

/// A parameter value: JSON when it parses (`12`, `true`, `["a","b"]`),
/// otherwise the text itself, so column ids need no JSON quoting.
fn json_or_text(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

/// `wind --help`.
pub const HELP: &str = "\
wind: wind resource analysis from the command line, with the same core and
the same JSON contract (wind-api DTOs) as the browser.

Usage: wind <command> <input> [options] [name=value ...]

Commands:
  import <file.csv>                  read a delimited text file
  project <file.wproj>               open a project
  apply <input> --ops <ops.json>     apply a list of edits
  export <input> --csv|--json|--wasp-tab|--epe|--openwind <out>
                                     write an export file
  analyze <input> <analysis>         run one analysis
  recipe <input> <analysis> --out <recipe.json>
                                     run one analysis and write its recipe
  replay <recipe.json> <source>      reproduce a recipe and compare
  run <input>... --workflow <file.json>
                                     apply a workflow to every dataset of
                                     each input, the inputs in parallel

<input> is a CSV file or a project file; projects are recognized by content.

Options:
  --config <file.json>   ImportConfigDto for a CSV input (omitted = detected)
  --append <file>        a file appended to a CSV input, read with the same
                         configuration; repeat for more (a logger's days)
  --ops <file.json>      list of OpDto applied after loading (apply, export,
                         analyze)
  -h, --help             this text; 'wind <command> --help' for one command

name=value pairs fill the command's request DTO, with the field names of
web/src/api/generated. A value is read as JSON when it parses (12, true,
[\"a\",\"b\"]) and as text otherwise; 'dataset' defaults to 0, the first
dataset of a project.

Output is JSON on standard output. On failure the exit code is 1 (2 for a
usage error) and an ErrorDto {\"code\", \"message\"} goes to standard error.
";

/// `wind import --help`.
const HELP_IMPORT: &str = "\
Usage: wind import <file.csv> [--config <config.json>]

Reads a delimited text file, detecting what --config leaves out, and prints
an ImportResultDto (dataset, import report, column summaries).
";

/// `wind project --help`.
const HELP_PROJECT: &str = "\
Usage: wind project <file.wproj>

Opens a project saved by the browser: re-imports every dataset with its
saved configuration, puts its flags back, and prints a JSON array with one
ImportResultDto per dataset. Dataset ids are 0, 1, ... in project order.
";

/// `wind apply --help`.
const HELP_APPLY: &str = "\
Usage: wind apply <input> --ops <ops.json> [--config <config.json>] [dataset=<id>]

Applies a JSON list of OpDto to one dataset, in order, and prints its
EditStateDto (segments, column summaries). Ops: flag, unflag,
flag_by_rules, scale_offset, delete_data, shift_steps, shift_start.
Example ops.json:
  [{\"op\": \"flag\", \"columns\": [\"Vel 100m Média\"], \"flag\": 0,
    \"fromMs\": 1672531200000, \"toMs\": 1672617600000},
   {\"op\": \"scale_offset\", \"columns\": [\"Temp 2m\"], \"fromMs\": 0,
    \"toMs\": 1704067199000, \"scale\": 1, \"offset\": -0.5}]
";

/// `wind export --help`.
const HELP_EXPORT: &str = "\
Usage: wind export <input> --csv|--json|--wasp-tab|--epe|--openwind <out> [--ops <ops.json>]
                   [--config <config.json>] [name=value ...]

Writes one export; <out> '-' writes to standard output.
  --csv <out>        the time series; fields of ExportCsvRequest
                     (delimiter defaults to ',')
  --json <out>       the dataset's metadata (DatasetDto)
  --wasp-tab <out>   a WAsP observed wind climate; fields of
                     ExportWaspTabRequest (docs/formatos/wasp-tab.md)
  --epe <out>        the EPE format (NT EPE-DEE-RE-057/2016-R4); fields
                     of ExportEpeRequest: station=<numeric code>, and column
                     ids for upperSpeed, lowerSpeed, upperVane... (omitted =
                     chosen by height; \"\" = no such sensor)
Examples:
  wind export torre.csv --wasp-tab torre.tab \"speed=Vel 100m Média\" \"direction=Dir 98m\"
       sectors=12 binWidthMS=1 firstBinHalf=true latitudeDeg=-5.12 longitudeDeg=-36.46
  wind export torre.csv --epe 1234567.txt station=1234567
";

/// `wind analyze --help`.
const HELP_ANALYZE: &str = "\
Usage: wind analyze <input> <analysis> [--ops <ops.json>] [--config <config.json>]
                    [name=value ...]

Runs one analysis and prints its response DTO, as the browser receives it.
  coverage     DatasetRequest     -> CoverageDto
  histogram    HistogramRequest   -> HistogramDto     column=... binWidth=...
  rose         WindRoseRequest    -> WindRoseDto      direction=... sectors=...
  diurnal      DiurnalRequest     -> DiurnalDto       column=...
  monthly      MonthlyMeansRequest -> MonthlyMeansDto column=...
  shear        ShearRequest       -> ShearDto         columns=[...] sectors=...
  shear_view   ShearViewRequest   -> ShearViewDto     quantity=... columns=[...] divisions=[...]
  shear_frequency ShearFrequencyRequest -> ShearFrequencyDto columns=[...] width=...
  turbulence   TurbulenceRequest  -> TurbulenceDto    speeds=[...] versus={...}
  extremes     ExtremesRequest    -> ExtremesDto      column=... method={...}
  table        MonthlyTableRequest -> MonthlyTableDto column=...
  scatter      ScatterRequest     -> ScatterDto       x=... y=...
Example:
  wind analyze torre.csv histogram \"column=Vel 100m Média\" binWidth=1 weibull=max_likelihood
";

/// `wind recipe --help`.
const HELP_RECIPE: &str = "\
Usage: wind recipe <input> <analysis> --out <recipe.json> [--ops <ops.json>]
                   [--config <config.json>] [name=value ...]

Runs one analysis as 'wind analyze' does and writes its recipe (RecipeDto):
the source file's name, size and SHA-256, the import configuration, the
flags, rule evidence and value edits, the analysis and its request, the
answer and the core's version. 'wind replay' reproduces it.
";

/// `wind run --help`.
const HELP_RUN: &str = "\
Usage: wind run <input>... --workflow <workflow.json> [--config <config.json>]

Applies a workflow (WorkflowDto: a list of OpDto edits, then analyses by
the names of 'wind analyze') to every dataset of each <input>: a CSV (read
with --config), or each dataset of a project. Several inputs run in
parallel, one per processor at most. Prints one JSON array of
WorkflowOutcomeDto, in input order, datasets numbered across the inputs:
the edits applied, the first that failed, each analysis's answer or error,
and the dataset's state. A failed edit stops its dataset, not the others;
an input that cannot be read stops the command.
";

/// `wind replay --help`.
const HELP_REPLAY: &str = "\
Usage: wind replay <recipe.json> <source>

Reads <source> with the recipe's configuration, puts back its state, runs
the analysis again and prints a ReplayDto: whether the file has the
recipe's SHA-256, whether this core is the same version, and whether the
answer is the same, number by number (with the paths that differ). Fails
with replay_mismatch when the file or the answer differs.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_line(line: &[&str]) -> Result<Parsed, CliError> {
        parse(line.iter().map(|argument| argument.to_string()))
    }

    fn invocation(line: &[&str]) -> Invocation {
        match parse_line(line) {
            Ok(Parsed::Run(invocation)) => invocation,
            other => panic!("{line:?} gave {other:?}"),
        }
    }

    #[test]
    fn positionals_options_and_parameters() {
        let parsed = invocation(&["analyze", "a=b.csv", "histogram", "--ops", "ops.json", "column=Vel 100m", "binWidth=0.5"]);
        assert_eq!(parsed.input, PathBuf::from("a=b.csv"), "a positional slot wins over '='");
        assert_eq!(parsed.analysis, Some("histogram"));
        assert_eq!(parsed.ops, Some(PathBuf::from("ops.json")));
        assert_eq!(parsed.parameters["column"], Value::String("Vel 100m".into()));
        assert_eq!(parsed.parameters["binWidth"], serde_json::json!(0.5));
    }

    #[test]
    fn help_and_usage_errors() {
        assert!(matches!(parse_line(&[]), Ok(Parsed::Help(HELP))));
        assert!(matches!(parse_line(&["export", "--help"]), Ok(Parsed::Help(HELP_EXPORT))));
        let code = |line: &[&str]| parse_line(line).unwrap_err().code();
        assert_eq!(code(&["bogus"]), "unknown_command");
        assert_eq!(code(&["import"]), "missing_argument");
        assert_eq!(code(&["import", "a.csv", "--ops"]), "missing_value");
        assert_eq!(code(&["import", "a.csv", "--colour", "x"]), "unknown_option");
        assert_eq!(code(&["import", "a.csv", "column=ws"]), "unexpected_argument");
        assert_eq!(code(&["project", "a.wproj", "--config", "c.json"]), "config_for_project");
        assert_eq!(code(&["apply", "a.csv"]), "ops_required");
        assert_eq!(code(&["recipe", "a.csv", "histogram"]), "out_required");
        assert_eq!(code(&["run", "a.csv"]), "workflow_required");
        assert_eq!(code(&["run", "a.csv", "x=1", "--workflow", "w.json"]), "unexpected_argument", "run takes files, not fields");
        let batch = invocation(&["run", "a.csv", "b.wproj", "c.csv", "--workflow", "w.json"]);
        assert_eq!((batch.input.as_path(), batch.more_inputs.len()), (std::path::Path::new("a.csv"), 2), "every input after the first");
        assert_eq!(code(&["analyze", "a.csv", "coverage", "--workflow", "w.json"]), "unexpected_argument");
        assert_eq!(code(&["replay", "r.json"]), "missing_argument");
        assert_eq!(code(&["analyze", "a.csv", "spectrum"]), "unknown_analysis");
        assert_eq!(code(&["analyze", "a.csv", "coverage", "--out", "x"]), "unexpected_argument");
        assert_eq!(code(&["export", "a.csv"]), "export_format_count");
        assert_eq!(code(&["export", "a.csv", "--csv", "a", "--json", "b"]), "export_format_count");
        assert_eq!(code(&["analyze", "a.csv", "polar"]), "unknown_analysis");
        assert_eq!(code(&["analyze", "a.csv", "rose", "--csv", "out.csv"]), "unexpected_argument");
    }
}

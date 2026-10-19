//! The `wind` binary end to end, on samples/torre_exemplo.csv and on project
//! files written here by the layout of crates/wind-api/src/project.rs (the
//! same one web/src/project.ts writes), independently of the Rust reader.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The sample tower, 10-minute steps over 2023.
const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/torre_exemplo.csv");
/// Its mean speed at 100 m, its direction and the mean speed at 80 m.
const SPEED_100: &str = "Vel 100m Média";
const DIRECTION: &str = "Dir 98m";
const SPEED_80: &str = "Vel 80m Média";
/// Ten minutes, the sample's step, in ms.
const STEP_MS: f64 = 600_000.0;
/// Steps flagged by the tests: the first day.
const FLAGGED_STEPS: usize = 144;
const INVALID_FLAG: usize = 0;
/// Height the saved configuration gives the 100 m speed, to see it applied.
const CORRECTED_HEIGHT_M: f64 = 101.0;
/// File column of the 100 m speed (column 0 is the timestamp).
const SPEED_100_FILE_COLUMN: usize = 1;
/// Exit codes of a data failure and of a usage error.
const EXIT_FAILURE: i32 = 1;
const EXIT_USAGE: i32 = 2;
/// The project layout: magic, then the manifest length as a little-endian u32.
const PROJECT_MAGIC: &[u8] = b"WPRJ";
const PROJECT_FORMAT: u32 = 1;
/// The small second tower's clock and values.
const STEPS_PER_HOUR: usize = 6;
const MINUTES_PER_STEP: usize = 10;
const SPEED_PERIOD: usize = 20;
const DIRECTION_STRIDE_DEG: usize = 7;
const DEGREES_PER_TURN: usize = 360;
/// Sectors asked of the rose, hours of a diurnal profile, months of the sample.
const ROSE_SECTORS: usize = 12;
const HOURS_PER_DAY: usize = 24;
const SAMPLE_MONTHS: usize = 12;
/// An instant after any record: flags everything from the start.
const FAR_FUTURE_MS: f64 = 1e15;
/// The sample tower's site, as typed on the command line, and the height
/// the importer reads from the 100 m speed's header.
/// Coordinates in the sample's logger preamble.
const SAMPLE_LATITUDE_DEG: f64 = -5.1234;
const SAMPLE_LONGITUDE_DEG: f64 = -36.4567;
const SITE_LATITUDE_DEG: &str = "-6.5";
const SITE_LONGITUDE_DEG: &str = "-35.25";
const SPEED_100_HEIGHT_M: &str = "100";

/// A scratch directory of its own per test.
fn scratch(test: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("wind-cli-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn wind(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wind")).args(arguments).output().expect("the wind binary runs")
}

/// Standard output of a run that must succeed, as JSON.
fn answer(arguments: &[&str]) -> Value {
    let output = wind(arguments);
    assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("JSON on standard output")
}

/// Exit code and `ErrorDto` code of a run that must fail.
fn failure(arguments: &[&str]) -> (i32, String) {
    let output = wind(arguments);
    assert!(output.stdout.is_empty(), "nothing on standard output");
    let error: Value = serde_json::from_slice(&output.stderr).expect("an ErrorDto on standard error");
    assert!(error["message"].is_string());
    (output.status.code().unwrap(), error["code"].as_str().unwrap().to_string())
}

fn write_json(path: &Path, value: &Value) -> String {
    std::fs::write(path, value.to_string()).unwrap();
    path.to_str().unwrap().to_string()
}

/// A project file: magic, manifest length, manifest, sources in order.
fn project_file(manifest: &Value, sources: &[&[u8]]) -> Vec<u8> {
    let manifest = manifest.to_string();
    let mut file = PROJECT_MAGIC.to_vec();
    file.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    file.extend_from_slice(manifest.as_bytes());
    sources.iter().for_each(|source| file.extend_from_slice(source));
    file
}

/// A small second tower: one day at 10 minutes, speeds cycling to 20.
fn small_tower() -> Vec<u8> {
    let mut csv = String::from("t,ws,dir\n");
    for step in 0..FLAGGED_STEPS {
        let (hour, minute) = (step / STEPS_PER_HOUR, step % STEPS_PER_HOUR * MINUTES_PER_STEP);
        let direction = step * DIRECTION_STRIDE_DEG % DEGREES_PER_TURN;
        csv.push_str(&format!("2014-01-01 {hour:02}:{minute:02},{},{direction}\n", step % SPEED_PERIOD));
    }
    csv.into_bytes()
}

#[test]
fn a_workflow_runs_on_several_inputs_in_their_order() {
    let directory = scratch("batch");
    let small = directory.join("pequena.csv");
    std::fs::write(&small, small_tower()).unwrap();
    let workflow = json!({"format": 1, "ops": [], "analyses": [{"analysis": "coverage", "request": {}}]});
    let workflow = write_json(&directory.join("fluxo.json"), &workflow);
    let small = small.to_str().unwrap();
    let outcomes = answer(&["run", SAMPLE, small, SAMPLE, "--workflow", &workflow]);
    let names: Vec<&str> = outcomes.as_array().unwrap().iter().map(|outcome| outcome["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["torre_exemplo.csv", "pequena.csv", "torre_exemplo.csv"], "in input order, whichever thread finished first");
    let ids: Vec<u64> = outcomes.as_array().unwrap().iter().map(|outcome| outcome["dataset"].as_u64().unwrap()).collect();
    assert_eq!(ids, [0, 1, 2], "numbered across the inputs");
    assert!(outcomes.as_array().unwrap().iter().all(|outcome| outcome["results"][0]["error"].is_null()));
    let missing = directory.join("nao-existe.csv");
    assert_eq!(failure(&["run", SAMPLE, missing.to_str().unwrap(), "--workflow", &workflow]), (EXIT_FAILURE, "read_failed".to_string()));
}

#[test]
fn import_prints_the_import_result() {
    let result = answer(&["import", SAMPLE]);
    let columns = result["dataset"]["columns"].as_array().unwrap();
    assert_eq!(result["dataset"]["name"], "torre_exemplo.csv");
    assert_eq!(result["summaries"].as_array().unwrap().len(), columns.len());
    assert_eq!(columns[0]["id"], SPEED_100);
    assert!(result["report"]["rowsRead"].as_u64().unwrap() > 0);
}

#[test]
fn a_project_opens_like_its_sources_with_their_flags() {
    let directory = scratch("project");
    let sample = std::fs::read(SAMPLE).unwrap();
    let tower = small_tower();
    let config = json!({"columns": [{"fileColumn": SPEED_100_FILE_COLUMN, "heightM": CORRECTED_HEIGHT_M}]});
    let flags = answer(&["import", SAMPLE])["dataset"]["flags"].clone();
    let segment = json!({"column": SPEED_100, "flag": INVALID_FLAG, "startStep": 0, "endStep": FLAGGED_STEPS});
    let manifest = json!({"format": PROJECT_FORMAT, "datasets": [
        {"sourceName": "torre_exemplo.csv", "sourceBytes": sample.len(), "config": config,
         "restore": {"flags": flags, "segments": [segment]}},
        {"sourceName": "torre_b.csv", "sourceBytes": tower.len(), "config": {"columns": []},
         "restore": {"flags": flags, "segments": []}},
    ]});
    let project = directory.join("projeto.wproj");
    std::fs::write(&project, project_file(&manifest, &[&sample, &tower])).unwrap();
    let opened = answer(&["project", project.to_str().unwrap()]);
    let opened = opened.as_array().unwrap();
    assert_eq!(opened.len(), 2);

    // The same source, configuration and flags given to the other commands.
    let config_file = write_json(&directory.join("config.json"), &config);
    let start_ms = opened[0]["dataset"]["startMs"].as_f64().unwrap();
    let ops = json!([{"op": "flag", "columns": [SPEED_100], "flag": INVALID_FLAG,
                      "fromMs": start_ms, "toMs": start_ms + (FLAGGED_STEPS - 1) as f64 * STEP_MS}]);
    let ops_file = write_json(&directory.join("ops.json"), &ops);
    let imported = answer(&["import", SAMPLE, "--config", &config_file]);
    let edited = answer(&["apply", SAMPLE, "--config", &config_file, "--ops", &ops_file]);
    assert_eq!(opened[0]["dataset"]["columns"], imported["dataset"]["columns"]);
    assert_eq!(opened[0]["dataset"]["columns"][0]["heightM"], CORRECTED_HEIGHT_M, "the saved configuration applies");
    assert_eq!(opened[0]["report"], imported["report"]);
    assert_eq!(opened[0]["dataset"]["segments"], json!([segment]));
    assert_eq!(opened[0]["dataset"]["segments"], edited["segments"]);
    assert_eq!(opened[0]["summaries"], edited["summaries"], "summaries see the restored flags");

    let tower_file = directory.join("torre_b.csv");
    std::fs::write(&tower_file, &tower).unwrap();
    let tower_alone = answer(&["import", tower_file.to_str().unwrap()]);
    assert_eq!(opened[1]["dataset"]["id"], 1);
    assert_eq!(opened[1]["summaries"], tower_alone["summaries"]);

    // Commands that take any input read the project too; `dataset` picks one.
    let coverage = answer(&["analyze", project.to_str().unwrap(), "coverage", "dataset=1"]);
    // One entry per column, the derived ones (the speed's WPD) included.
    let column_count = opened[1]["dataset"]["columns"].as_array().unwrap().len();
    assert_eq!(coverage["total"], json!(vec![1.0; column_count]));
    let state = answer(&["apply", project.to_str().unwrap(), "--ops", &ops_file]);
    assert_eq!(state["segments"], edited["segments"], "re-flagging the restored stretch changes nothing");
    assert_eq!(state["canUndo"], false);
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn damaged_or_newer_projects_fail_with_an_error_dto() {
    let directory = scratch("damaged");
    let tower = small_tower();
    let manifest = |format: u32| {
        json!({"format": format, "datasets": [{"sourceName": "t.csv", "sourceBytes": tower.len(),
               "config": {"columns": []}, "restore": {"flags": [], "segments": []}}]})
    };
    let good = project_file(&manifest(PROJECT_FORMAT), &[&tower]);
    let cases: [(&str, Vec<u8>, &str); 3] = [
        ("truncated", good[..good.len() - 1].to_vec(), "not_a_project"),
        ("trailing", [good.as_slice(), b"x"].concat(), "not_a_project"),
        ("newer", project_file(&manifest(PROJECT_FORMAT + 1), &[&tower]), "newer_project_format"),
    ];
    for (case, bytes, code) in cases {
        let path = directory.join(format!("{case}.wproj"));
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(failure(&["project", path.to_str().unwrap()]), (EXIT_FAILURE, code.to_string()), "{case}");
    }
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn bad_ops_and_requests_fail_with_their_codes() {
    let directory = scratch("ops");
    let unknown = write_json(&directory.join("unknown.json"), &json!([{"op": "flag", "columns": ["ws999"], "flag": 0, "fromMs": 0, "toMs": 1}]));
    let typo = write_json(&directory.join("typo.json"), &json!([{"op": "flag", "column": [SPEED_100], "flag": 0, "fromMs": 0, "toMs": 1}]));
    let bad_flag = write_json(&directory.join("flag.json"), &json!([{"op": "flag", "columns": [SPEED_100], "flag": 99, "fromMs": 0, "toMs": 1}]));
    assert_eq!(failure(&["apply", SAMPLE, "--ops", &unknown]), (EXIT_FAILURE, "unknown_column".into()));
    assert_eq!(failure(&["apply", SAMPLE, "--ops", &typo]), (EXIT_FAILURE, "bad_request".into()));
    assert_eq!(failure(&["apply", SAMPLE, "--ops", &bad_flag]), (EXIT_FAILURE, "no_such_flag".into()));
    assert_eq!(failure(&["analyze", SAMPLE, "histogram", "column=ws999", "binWidth=1"]), (EXIT_FAILURE, "unknown_column".into()));
    assert_eq!(failure(&["analyze", SAMPLE, "histogram", "binWidht=1"]), (EXIT_FAILURE, "bad_request".into()));
    assert_eq!(failure(&["analyze", SAMPLE, "coverage", "dataset=3"]), (EXIT_FAILURE, "no_such_dataset".into()));
    assert_eq!(failure(&["import", "missing.csv"]), (EXIT_FAILURE, "read_failed".into()));
    assert_eq!(failure(&["frobnicate"]), (EXIT_USAGE, "unknown_command".into()));
    assert_eq!(failure(&["apply", SAMPLE]), (EXIT_USAGE, "ops_required".into()));
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn exports_write_files_or_standard_output() {
    let directory = scratch("export");
    let csv = directory.join("torre.csv");
    let ops = write_json(
        &directory.join("ops.json"),
        &json!([{"op": "flag", "columns": [SPEED_100], "flag": INVALID_FLAG, "fromMs": 0, "toMs": FAR_FUTURE_MS}]),
    );
    assert!(wind(&["export", SAMPLE, "--csv", csv.to_str().unwrap(), "delimiter=;", "decimalComma=true"]).status.success());
    let original = answer(&["import", SAMPLE]);
    let reread = answer(&["import", csv.to_str().unwrap()]);
    assert_eq!(reread["dataset"]["stepCount"], original["dataset"]["stepCount"]);
    assert_eq!(reread["summaries"][0]["mean"], original["summaries"][0]["mean"], "the values survive the round trip");

    let metadata = answer(&["export", SAMPLE, "--ops", &ops, "--json", "-"]);
    assert_eq!(metadata["segments"][0]["column"], SPEED_100, "exports see the ops");
    let stdout = wind(&["export", SAMPLE, "--csv", "-"]).stdout;
    assert!(String::from_utf8(stdout).unwrap().starts_with("Timestamp,"));
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn every_analysis_answers_with_its_dto() {
    let speed = format!("column={SPEED_100}");
    let histogram = answer(&["analyze", SAMPLE, "histogram", &speed, "binWidth=1", "weibull=max_likelihood"]);
    assert!(histogram["weibull"]["k"].as_f64().unwrap() > 1.0);
    let sectors = format!("sectors={ROSE_SECTORS}");
    let rose = answer(&["analyze", SAMPLE, "rose", &format!("direction={DIRECTION}"), &format!("speed={SPEED_100}"), &sectors]);
    assert_eq!(rose["frequency"].as_array().unwrap().len(), ROSE_SECTORS);
    let diurnal = answer(&["analyze", SAMPLE, "diurnal", &speed]);
    assert_eq!(diurnal["all"].as_array().unwrap().len(), HOURS_PER_DAY);
    let columns = json!([SPEED_80, SPEED_100]).to_string();
    let shear = answer(&["analyze", SAMPLE, "shear", &format!("columns={columns}"), &sectors]);
    assert!(shear["alpha"].as_f64().unwrap() > 0.0);
    let coverage = answer(&["analyze", SAMPLE, "coverage"]);
    assert_eq!(coverage["months"].as_array().unwrap().len(), SAMPLE_MONTHS);
}

#[test]
fn wasp_tab_export_takes_the_site_from_the_preamble_or_the_request() {
    let fields = [
        format!("speed={SPEED_100}"),
        format!("direction={DIRECTION}"),
        format!("sectors={ROSE_SECTORS}"),
        format!("latitudeDeg={SITE_LATITUDE_DEG}"),
        format!("longitudeDeg={SITE_LONGITUDE_DEG}"),
    ];
    let site_line = |line: &[&str]| {
        let output = wind(line);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<String> = text.lines().map(String::from).collect();
        assert_eq!(lines[2], format!("{ROSE_SECTORS} 1.00 0.00"));
        assert_eq!(lines[3].split_whitespace().count(), ROSE_SECTORS);
        lines[1].clone()
    };
    let mut line = vec!["export", SAMPLE, "--wasp-tab", "-", "binWidthMS=1", &fields[0], &fields[1], &fields[2]];
    // The sample's logger preamble says "Lat: -5,1234; Lon: -36,4567".
    let from_preamble = format!("{SAMPLE_LATITUDE_DEG} {SAMPLE_LONGITUDE_DEG} {SPEED_100_HEIGHT_M}");
    assert_eq!(site_line(&line), from_preamble, "the height comes from the speed column");
    line.extend([fields[3].as_str(), &fields[4]]);
    assert_eq!(site_line(&line), format!("{SITE_LATITUDE_DEG} {SITE_LONGITUDE_DEG} {SPEED_100_HEIGHT_M}"), "the request wins");
}

#[test]
fn help_is_text_on_standard_output() {
    for line in [&[][..], &["--help"], &["analyze", "--help"]] {
        let output = wind(line);
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage: wind"));
    }
}

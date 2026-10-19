//! Reproducible recipes (ARQUITETURA C3): one analysis result with
//! everything it is a function of — the source file (by SHA-256), the import
//! configuration, the dataset's state (flags, rule evidence, value edits),
//! the analysis and its request, and the core's version. [`replay`] reads
//! the source again, rebuilds the state, runs the same call and compares the
//! answer number by number: the core is deterministic bit for bit (P11), so
//! the same source and version give the same numbers, in the browser or on
//! the command line.
use crate::analyses::{self, Prepared};
use crate::requests::{finish_import, ImportConfigDto, RestoreDto};
use crate::ErrorDto;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use wind_core::filter::FilterView;
use wind_core::hash::sha256_hex;
use wind_core::import;
use wind_core::series::DayStats;

/// Version of the recipe layout; readers refuse others.
pub const RECIPE_FORMAT: u32 = 1;
/// Code of an [`ErrorDto`] for a recipe of another layout.
pub const OTHER_RECIPE_FORMAT: &str = "other_recipe_format";
/// Differences a replay lists; past this it only counts them.
pub const MAX_LISTED_DIFFERENCES: usize = 20;

/// The source file a recipe was made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeSourceDto {
    /// File name, the dataset's name.
    pub name: String,
    /// Length, bytes.
    pub bytes: usize,
    /// SHA-256 of the file, 64 lowercase hex digits.
    pub sha256: String,
}

/// One analysis result and what it is a function of.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipeDto {
    /// [`RECIPE_FORMAT`].
    pub format: u32,
    /// The core version that computed `result`.
    pub core_version: String,
    /// The source file.
    pub source: RecipeSourceDto,
    /// How the file was read.
    pub config: ImportConfigDto,
    /// Flags, segments, rule evidence and value edits.
    pub restore: RestoreDto,
    /// The analysis, as `wind analyze` names it ([`analyses::ANALYSIS_NAMES`]).
    pub analysis: String,
    /// Its request, as sent.
    #[ts(type = "unknown")]
    pub request: Value,
    /// Its answer.
    #[ts(type = "unknown")]
    pub result: Value,
}

/// Any analysis by name, as [`analyses::run`] answers it: what the
/// browser sends to export a recipe.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyzeRequest {
    /// The dataset.
    pub dataset: crate::DatasetId,
    /// One of [`analyses::ANALYSIS_NAMES`].
    pub analysis: String,
    /// The analysis's own request.
    #[ts(type = "unknown")]
    pub request: Value,
}

/// What a replay found.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplayDto {
    /// The file given has the recipe's SHA-256.
    pub source_matches: bool,
    /// This core is the version that made the recipe.
    pub version_matches: bool,
    /// The answer equals the recipe's, number by number.
    pub result_matches: bool,
    /// This core's version.
    pub core_version: String,
    /// JSON paths where the answers differ, the first
    /// [`MAX_LISTED_DIFFERENCES`] of them.
    pub differences: Vec<String>,
    /// How many paths differ in all.
    pub difference_count: usize,
    /// This core's answer.
    #[ts(type = "unknown")]
    pub result: Value,
}

impl RecipeDto {
    /// The recipe of `result`, made with this core.
    pub fn new(source_name: &str, source: &[u8], config: ImportConfigDto, restore: RestoreDto, analysis: &str, request: Value, result: Value) -> Self {
        RecipeDto {
            format: RECIPE_FORMAT,
            core_version: wind_core::VERSION.to_string(),
            source: RecipeSourceDto { name: source_name.to_string(), bytes: source.len(), sha256: sha256_hex(source) },
            config,
            restore,
            analysis: analysis.to_string(),
            request,
            result,
        }
    }
}

/// Replays `recipe` on `source`: reads it with the recipe's configuration,
/// restores the state, runs the analysis and compares. A file with another
/// hash is still replayed, and reported.
///
/// # Errors
/// [`OTHER_RECIPE_FORMAT`], the import's or the restore's error, or the
/// analysis's.
pub fn replay(recipe: RecipeDto, source: &[u8]) -> Result<ReplayDto, ErrorDto> {
    if recipe.format != RECIPE_FORMAT {
        return Err(ErrorDto { code: OTHER_RECIPE_FORMAT.into(), message: format!("recipe format {} (this version reads {RECIPE_FORMAT})", recipe.format) });
    }
    let core_error = |error: wind_core::error::CoreError| ErrorDto::from(&error);
    let text = import::decode_text(source);
    let (mut data, _) = import::import_text(&recipe.source.name, &text, &recipe.config.into_core()).map_err(core_error)?;
    finish_import(&mut data, Some(recipe.restore), None).map_err(core_error)?;
    let view = FilterView::new(&data);
    let stats = DayStats::new(&data, &view);
    let result = analyses::run(Prepared { data: &data, view: &view, stats: &stats }, &recipe.analysis, recipe.request)?;
    let mut differences = Vec::new();
    let difference_count = compare(&recipe.result, &result, String::new(), &mut differences);
    Ok(ReplayDto {
        source_matches: sha256_hex(source) == recipe.source.sha256,
        version_matches: recipe.core_version == wind_core::VERSION,
        result_matches: difference_count == 0,
        core_version: wind_core::VERSION.to_string(),
        differences,
        difference_count,
        result,
    })
}

/// Counts the JSON paths where `expected` and `found` differ, listing the
/// first [`MAX_LISTED_DIFFERENCES`]. Numbers compare exactly by value
/// (`1` and `1.0` are equal; any other gap is a difference), so a replay
/// that is not bit for bit is caught. The counts in answers stay far below
/// 2⁵³, where every integer is an exact `f64`.
fn compare(expected: &Value, found: &Value, path: String, differences: &mut Vec<String>) -> usize {
    match (expected, found) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<&String> = left.keys().chain(right.keys()).collect();
            keys.sort_unstable();
            keys.dedup();
            keys.into_iter()
                .map(|key| compare(left.get(key).unwrap_or(&Value::Null), right.get(key).unwrap_or(&Value::Null), format!("{path}/{key}"), differences))
                .sum()
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            left.iter().zip(right).enumerate().map(|(index, (left, right))| compare(left, right, format!("{path}/{index}"), differences)).sum()
        }
        // By value: a browser writes 1.0 as 1, which reads back as an integer.
        (Value::Number(left), Value::Number(right)) if left.as_f64() == right.as_f64() => 0,
        _ if expected == found => 0,
        _ => {
            if differences.len() < MAX_LISTED_DIFFERENCES {
                differences.push(if path.is_empty() { "/".into() } else { path });
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Two days of ten-minute speeds and directions.
    fn source() -> Vec<u8> {
        let mut csv = String::from("t,ws,dir\n");
        for step in 0..288 {
            csv += &format!("2023-01-{:02} {:02}:{:02},{},{}\n", 1 + step / 144, (step % 144) / 6, (step % 6) * 10, 5 + step % 9, (step * 37) % 360);
        }
        csv.into_bytes()
    }

    fn recipe_of(bytes: &[u8], analysis: &str, request: Value) -> RecipeDto {
        let config = ImportConfigDto::default();
        let (mut data, _) = import::import_text("torre.csv", &import::decode_text(bytes), &config.clone().into_core()).unwrap();
        finish_import(&mut data, None, None).unwrap();
        let (view, restore) = (FilterView::new(&data), RestoreDto::new(&data));
        let stats = DayStats::new(&data, &view);
        let result = analyses::run(Prepared { data: &data, view: &view, stats: &stats }, analysis, request.clone()).unwrap();
        RecipeDto::new("torre.csv", bytes, config, restore, analysis, request, result)
    }

    #[test]
    fn a_recipe_replays_to_the_same_numbers_through_json() {
        let bytes = source();
        let recipe = recipe_of(&bytes, "histogram", json!({"dataset": 0, "column": "ws", "binWidth": 1, "weibull": "max_likelihood"}));
        let written = serde_json::to_string(&recipe).unwrap();
        let replayed = replay(serde_json::from_str(&written).unwrap(), &bytes).unwrap();
        assert!(replayed.source_matches && replayed.version_matches && replayed.result_matches, "{replayed:?}");
        assert_eq!(replayed.difference_count, 0);
    }

    #[test]
    fn a_whole_number_written_without_its_point_is_the_same_number() {
        let mut differences = Vec::new();
        assert_eq!(compare(&json!({"recovery": [1, 0.5]}), &json!({"recovery": [1.0, 0.5]}), String::new(), &mut differences), 0);
        assert_eq!(compare(&json!([1]), &json!([1.000_000_000_000_000_2]), String::new(), &mut differences), 1, "one unit in the last place is a difference");
    }

    #[test]
    fn numbers_read_back_bit_for_bit() {
        // Without serde_json's float_roundtrip a parsed number can miss by
        // one unit in the last place, and every replay would differ.
        let mut random = wind_core::bootstrap::SplitMix64::new(1);
        for _ in 0..10_000 {
            let value = f64::from_bits(random.next_u64());
            if value.is_finite() {
                let read: f64 = serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
                assert_eq!(read.to_bits(), value.to_bits(), "{value:e}");
            }
        }
    }

    #[test]
    fn another_file_or_result_is_reported() {
        let bytes = source();
        let mut recipe = recipe_of(&bytes, "diurnal", json!({"dataset": 0, "column": "ws"}));
        recipe.result["all"][3] = json!(99.0);
        let mut other = bytes.clone();
        other.extend_from_slice(b"\n");
        let replayed = replay(recipe, &other).unwrap();
        assert!(!replayed.source_matches, "one more byte, another hash");
        assert_eq!((replayed.result_matches, replayed.differences.as_slice()), (false, ["/all/3".to_string()].as_slice()));
        let unknown = recipe_of(&bytes, "diurnal", json!({"dataset": 0, "column": "ws"}));
        let renamed = RecipeDto { analysis: "spectrum".into(), ..unknown };
        assert_eq!(replay(renamed, &bytes).unwrap_err().code, analyses::UNKNOWN_ANALYSIS);
    }
}

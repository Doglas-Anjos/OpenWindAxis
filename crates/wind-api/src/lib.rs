//! The wire contract between `wind-core` and its callers (browser, CLI,
//! server), per ARQUITETURA §3. Every call takes one request DTO and returns
//! one response DTO; the domain types never cross the boundary.
//!
//! Wire rules: fields in camelCase, enum values in snake_case, units in field
//! names (`heightM`, `startMs`), missing numbers as explicit `null`, datasets
//! by a stable [`DatasetId`], columns by their unique id string. TypeScript
//! types are generated from these definitions by `cargo test -p wind-api`.
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::CoreError;
use wind_core::model;
use wind_core::weibull;

pub mod analyses;
pub mod append;
pub mod calculated;
pub mod distortion;
pub mod distribution;
pub mod epe;
pub mod extremes;
pub mod filter;
pub mod flag_statistics;
pub mod mcp;
pub mod openwind;
pub mod means;
pub mod ops;
pub mod project;
pub mod recipe;
pub mod requests;
pub mod responses;
pub mod rules;
pub mod scatter;
pub mod shading;
pub mod shear_views;
pub mod turbulence;
pub mod wasp;
pub mod workflow;

/// Version of this contract. Adding an optional field keeps it; removing or
/// renaming a field, or changing its meaning, bumps it. The UI checks it
/// when it starts.
pub const API_VERSION: u32 = 1;

/// A dataset in a session. Assigned at import and never reused, so removing
/// one dataset never changes another's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct DatasetId(pub u32);

/// A failure as the caller receives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    /// Stable snake_case identifier, for translating the message.
    pub code: String,
    /// English description, for logs and as a fallback.
    pub message: String,
}

/// Code of an [`ErrorDto`] for a request that is not valid JSON for its type.
pub const BAD_REQUEST: &str = "bad_request";

impl ErrorDto {
    /// A request that did not parse.
    pub fn bad_request(detail: impl std::fmt::Display) -> Self {
        ErrorDto { code: BAD_REQUEST.into(), message: format!("invalid request: {detail}") }
    }

    /// As JSON, for transports that carry errors as text.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("two strings always serialize")
    }
}

impl From<&CoreError> for ErrorDto {
    fn from(error: &CoreError) -> Self {
        ErrorDto { code: error.code().into(), message: error.to_string() }
    }
}

/// What a column measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    /// Horizontal wind speed, m/s.
    WindSpeed,
    /// Vertical wind speed (sonic, lidar), m/s.
    VerticalWindSpeed,
    /// Wind direction, degrees clockwise from north.
    WindDirection,
    /// Air temperature, °C or K.
    Temperature,
    /// Air pressure, hPa, kPa or Pa.
    Pressure,
    /// Relative humidity, %.
    RelativeHumidity,
    /// Solar irradiance, W/m².
    SolarRadiation,
    /// Logger battery voltage, V.
    BatteryVoltage,
    /// Turbine power output, kW.
    TurbineOutput,
    /// A sensor's own quality or availability figure.
    Quality,
    /// Anything else.
    Other,
    /// σ/U of a speed column, dimensionless (derived).
    TurbulenceIntensity,
    /// ½·ρ·U³, W/m² (derived).
    PowerDensity,
    /// kg/m³ (derived).
    AirDensity,
}

/// Which statistic of each step a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnSubtype {
    /// Mean over the step.
    Mean,
    /// Minimum within the step.
    Min,
    /// Maximum within the step.
    Max,
    /// Highest short gust within the step.
    Gust,
    /// Standard deviation within the step.
    StdDev,
}

/// Which instant of each interval a timestamp marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TimestampMeaning {
    #[default]
    /// The timestamp is the interval's start.
    Start,
    /// The timestamp is the interval's middle.
    Middle,
    /// The timestamp is the interval's end.
    End,
}

/// How a Weibull distribution is fitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WeibullMethod {
    /// Maximum likelihood (Stevens & Smulders 1979).
    MaxLikelihood,
    /// Least squares on the linearized CDF.
    LeastSquares,
    /// Matches the mean cubed speed and the share of samples above the mean (WAsP).
    Wasp,
    /// Matches the mean and the mean cubed speed (Openwind).
    Openwind,
}

/// How a shear profile averages each height's speed: `ap_mean` (simple
/// mean), `ap_momm` (mean of monthly means, free of seasonal bias), or one
/// fit per step and their mean (`ts_mean`) or median (`ts_median`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ShearAggregation {
    /// Simple mean of the concurrent steps.
    ApMean,
    /// Mean of monthly means, free of seasonal bias: the default.
    ApMomm,
    /// Mean of the step-by-step parameters.
    TsMean,
    /// Median of the step-by-step parameters.
    TsMedian,
}

/// Two-way conversions between a wire enum and its domain twin, variant by
/// variant, so the two can never silently drift apart.
macro_rules! mirror_enum {
    ($wire:ident <=> $domain:path { $($variant:ident),+ $(,)? }) => {
        impl From<$wire> for $domain {
            fn from(value: $wire) -> Self {
                match value { $($wire::$variant => <$domain>::$variant),+ }
            }
        }
        impl From<$domain> for $wire {
            fn from(value: $domain) -> Self {
                match value { $(<$domain>::$variant => $wire::$variant),+ }
            }
        }
    };
}

mirror_enum!(ColumnKind <=> model::ColumnType {
    WindSpeed, VerticalWindSpeed, WindDirection, Temperature, Pressure, RelativeHumidity,
    SolarRadiation, BatteryVoltage, TurbineOutput, Quality, Other,
    TurbulenceIntensity, PowerDensity, AirDensity,
});
mirror_enum!(ColumnSubtype <=> model::Subtype { Mean, Min, Max, Gust, StdDev });
mirror_enum!(TimestampMeaning <=> model::TimeStampMeaning { Start, Middle, End });
mirror_enum!(WeibullMethod <=> weibull::Method { MaxLikelihood, LeastSquares, Wasp, Openwind });
mirror_enum!(ShearAggregation <=> wind_core::analysis::ShearAggregation { ApMean, ApMomm, TsMean, TsMedian });

/// `Some(value)` for a finite number, `None` (JSON `null`) for NaN or ±∞.
pub(crate) fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

/// [`finite`] over a series.
pub(crate) fn finite_all(values: &[f64]) -> Vec<Option<f64>> {
    values.iter().copied().map(finite).collect()
}

#[cfg(test)]
mod typescript {
    //! Writes the TypeScript bindings the web UI imports. Run
    //! `cargo test -p wind-api` after changing any DTO and commit the result.
    use super::*;
    use crate::project::*;
    use crate::requests::*;
    use crate::responses::*;
    use std::path::PathBuf;

    /// Where the web UI imports the generated types from.
    fn bindings_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/src/api/generated")
    }

    #[test]
    fn export_typescript_bindings() {
        // JSON numbers, not bigint: every integer on the wire fits in 2^53.
        let config = ts_rs::Config::new().with_large_int("number").with_out_dir(bindings_dir());
        macro_rules! export {
            ($($dto:ty),+ $(,)?) => { $( <$dto>::export_all(&config).expect(stringify!($dto)); )+ };
        }
        export!(
            ErrorDto, ImportRequest, SeriesRequest, FlagRequest, UnflagRequest, DatasetRequest, HistogramRequest, DefineFlagsRequest,
            WindRoseRequest, DiurnalRequest, ShearRequest, ExportCsvRequest, ImportResultDto, EditStateDto,
            CoverageDto, HistogramDto, WindRoseDto, DiurnalDto, ShearDto, ProjectDto,
            crate::rules::FlagByRulesRequest, crate::rules::FlagByRulesDto,
            crate::flag_statistics::FlagStatisticsDto, crate::flag_statistics::RemoveFlagRequest,
            CoverageRequest, MonthlyMeansRequest, MonthlyMeansDto,
        );
        export!(crate::ops::OpDto, crate::ops::ApplyOpRequest, crate::ops::EditDto, crate::ops::SelectionRequest, crate::ops::SelectionCountDto);
        export!(crate::wasp::ExportWaspTabRequest);
        export!(crate::epe::ExportEpeRequest, crate::epe::EpeFileDto);
        export!(crate::openwind::ExportOpenwindRequest, crate::openwind::OpenwindFileDto, crate::openwind::OpenwindMeansDto);
        export!(crate::turbulence::TurbulenceRequest, crate::turbulence::TurbulenceDto);
        export!(crate::extremes::ExtremesRequest, crate::extremes::ExtremesDto);
        export!(crate::means::PeriodMeansRequest, crate::means::PeriodMeansDto, crate::means::MonthlyTableRequest, crate::means::MonthlyTableDto);
        export!(crate::scatter::ScatterRequest, crate::scatter::ScatterDto);
        export!(crate::distribution::SpeedDistributionRequest, crate::distribution::SpeedDistributionDto);
        export!(crate::distortion::TowerDistortionRequest, crate::distortion::TowerDistortionDto);
        export!(crate::distortion::DistortionDetailRequest, crate::distortion::DistortionDetailDto);
        export!(crate::shading::TowerShadingRequest, crate::shading::TowerShadingDto);
        export!(crate::calculated::ExpressionCheckRequest, crate::calculated::ExpressionCheckDto);
        export!(
            crate::shear_views::ShearViewRequest, crate::shear_views::ShearViewDto,
            crate::shear_views::ShearFrequencyRequest, crate::shear_views::ShearFrequencyDto,
        );
        export!(crate::mcp::McpRequest, crate::mcp::McpSeriesRequest, crate::mcp::McpDto);
        export!(crate::mcp::TimeOffsetRequest, crate::mcp::TimeOffsetDto);
        export!(crate::workflow::WorkflowDto, crate::workflow::WorkflowRunRequest, crate::workflow::WorkflowOutcomeDto);
        export!(crate::recipe::RecipeDto, crate::recipe::ReplayDto, crate::recipe::AnalyzeRequest);
        let (recipe_format, core_version) = (crate::recipe::RECIPE_FORMAT, wind_core::VERSION);
        let workflow_format = crate::workflow::WORKFLOW_FORMAT;
        let (power_format, toa5_format) = (wind_core::import::POWER_FORMAT, wind_core::import::TOA5_FORMAT);
        let (ammonit_format, sympro_format) = (wind_core::import::AMMONIT_FORMAT, wind_core::import::SYMPRO_FORMAT);
        let version = format!(
            "// Generated by wind-api; do not edit.\nexport const API_VERSION = {API_VERSION};\n\
             export const PROJECT_FORMAT = {PROJECT_FORMAT};\nexport const PROJECT_MAGIC = '{PROJECT_MAGIC}';\n\
             export const RECIPE_FORMAT = {recipe_format};\nexport const CORE_VERSION = '{core_version}';\n\
             export const WORKFLOW_FORMAT = {workflow_format};\n\
             export const POWER_FORMAT = '{power_format}';\nexport const TOA5_FORMAT = '{toa5_format}';\n\
             export const AMMONIT_FORMAT = '{ammonit_format}';\nexport const SYMPRO_FORMAT = '{sympro_format}';\n"
        );
        std::fs::write(bindings_dir().join("version.ts"), version).expect("write version.ts");
    }
}

#[cfg(test)]
mod contract {
    use super::*;
    use crate::requests::*;
    use crate::responses::*;

    #[test]
    fn requests_reject_unknown_fields() {
        let typo = r#"{"dataset": 1, "column": "ws", "binWidht": 1}"#;
        let error = serde_json::from_str::<HistogramRequest>(typo).unwrap_err();
        assert!(error.to_string().contains("binWidht"), "{error}");
    }

    #[test]
    fn primary_columns_follow_the_configuration() {
        let csv = "t,Vel 80m Média,Vel 100m Média,Dir 98m\n2023-01-01 00:00,5,6,180\n2023-01-01 00:10,5,6,190\n";
        let primary_of = |config: &str| {
            let config: ImportConfigDto = serde_json::from_str(config).unwrap();
            let (mut data, _) = wind_core::import::import_text("torre", csv, &config.into_core()).unwrap();
            finish_import(&mut data, None, None).unwrap();
            DatasetDto::new(DatasetId(0), &data).primary.into_iter().map(|primary| primary.column).collect::<Vec<_>>()
        };
        assert_eq!(primary_of(r#"{"columns": []}"#), ["Vel 100m Média", "Dir 98m"], "by default the highest speed");
        let speed = |choice: &str| primary_of(&format!(r#"{{"columns": [], "primary": [{{"kind": "wind_speed", "choice": {choice}}}]}}"#))[0].clone();
        assert_eq!(speed(r#"{"by": "lowest"}"#), "Vel 80m Média");
        assert_eq!(speed(r#"{"by": "nearest", "heightM": 85}"#), "Vel 80m Média");
        assert_eq!(speed(r#"{"by": "column", "column": "Vel 100m Média"}"#), "Vel 100m Média");
    }

    #[test]
    fn flags_on_synthesized_columns_and_calculated_columns_survive_a_reopen_and_a_reread() {
        let csv = "t,Vel 20m,Vel 40m\n2023-01-01 00:00,5,6\n2023-01-01 00:10,6,7\n2023-01-01 00:20,7,8\n";
        let config: ImportConfigDto = serde_json::from_str(
            r#"{"columns": [], "extrapolations": [{"lower": "Vel 20m", "upper": "Vel 40m", "heightM": 80}],
                "calculated": [{"id": "Média", "calculation": {"type": "combine", "method": "average", "columns": ["Vel 20m", "Vel 40m"]}}]}"#,
        )
        .unwrap();
        let read = || wind_core::import::import_text("torre", csv, &config.clone().into_core()).unwrap().0;
        let mut first = read();
        finish_import(&mut first, None, None).unwrap();
        let index = |data: &wind_core::model::Dataset, id: &str| data.columns.iter().position(|column| column.id == id).unwrap();
        let synthesized = index(&first, "Vel 80 m extrapolada");
        wind_core::ops::Op::Flag { columns: vec![synthesized, index(&first, "Vel 40m")], flag: 0, steps: 0..1 }.apply(&mut first).unwrap();
        assert_eq!(first.columns[index(&first, "Média")].values[0], 5.0, "the average takes Vel 20m alone where Vel 40m is flagged");
        let flagged = |data: &wind_core::model::Dataset| data.segments.iter().any(|segment| segment.column == index(data, "Vel 80 m extrapolada"));
        // A saved project reopened: the flag on the synthesized column comes back, and the calculated column reads it.
        let mut reopened = read();
        finish_import(&mut reopened, Some(RestoreDto::new(&first)), None).unwrap();
        assert!(flagged(&reopened));
        assert_eq!(reopened.columns[index(&reopened, "Média")].values[0], 5.0);
        // The file read again in place: likewise.
        let mut reread = read();
        finish_import(&mut reread, None, Some(&first)).unwrap();
        assert!(flagged(&reread));
        assert_eq!(reread.columns[index(&reread, "Média")].values[0], 5.0);
    }

    #[test]
    fn a_displacement_height_reaches_the_shear() {
        let csv = "t,Vel 20m,Vel 40m\n2023-01-01 00:00,5,6\n2023-01-01 00:10,6,7\n";
        let shear_with = |config: &str| {
            let config: ImportConfigDto = serde_json::from_str(config).unwrap();
            let (mut data, _) = wind_core::import::import_text("torre", csv, &config.into_core())?;
            finish_import(&mut data, None, None).unwrap();
            let view = wind_core::filter::FilterView::new(&data);
            wind_core::analysis::shear(&data, &view, &[0, 1], None, 12, Default::default()).map(ShearDto::from)
        };
        assert_eq!(shear_with(r#"{"columns": []}"#).unwrap().displacement_m, 0.0);
        let by_month = shear_with(r#"{"columns": [], "displacement": {"by": "month", "heightsM": [5,5,5,5,5,5,5,5,5,5,5,5]}}"#).unwrap();
        assert!((by_month.displacement_m - 5.0).abs() < 1e-12, "the mean of the month heights, weighted like the speeds");
        let negative = shear_with(r#"{"columns": [], "displacement": {"by": "constant", "heightM": -1}}"#).unwrap_err();
        assert_eq!(ErrorDto::from(&negative).code, "displacement_height", "refused at import");
    }

    #[test]
    fn flag_definitions_say_what_each_flag_was() {
        let request: DefineFlagsRequest = serde_json::from_str(
            r##"{"dataset": 0, "flags": [
                {"flag": {"name": "Gelo", "color": "#2a9fc0", "include": false}, "was": 2},
                {"flag": {"name": "Suspeito", "color": "#888888", "include": true, "notes": "a conferir"}}]}"##,
        )
        .unwrap();
        let flags = request.into_core();
        assert_eq!((flags[0].0.notes.as_str(), flags[0].1), ("", Some(2)), "a flag saved before notes existed has none");
        assert_eq!((flags[1].0.notes.as_str(), flags[1].1), ("a conferir", None), "no former index: a new flag");
    }

    #[test]
    fn height_corrections_tell_absent_from_null() {
        let parse = |json: &str| serde_json::from_str::<ColumnCorrectionDto>(json).unwrap().height_m;
        assert_eq!(parse(r#"{"fileColumn": 1}"#), None, "absent: keep the guess");
        assert_eq!(parse(r#"{"fileColumn": 1, "heightM": null}"#), Some(None), "null: clear the height");
        assert_eq!(parse(r#"{"fileColumn": 1, "heightM": 60}"#), Some(Some(60.0)));
    }

    #[test]
    fn undefined_numbers_travel_as_null_and_names_as_camel_case() {
        let diurnal = wind_core::analysis::Diurnal { all: vec![f64::NAN, 2.5], by_month: vec![vec![f64::INFINITY]] };
        let json = serde_json::to_string(&DiurnalDto::from(diurnal)).unwrap();
        assert_eq!(json, r#"{"all":[null,2.5],"byMonth":[[null]]}"#);
    }

    #[test]
    fn core_errors_keep_their_code() {
        let dto = ErrorDto::from(&CoreError::UnknownColumn { id: "ws80".into() });
        assert_eq!(dto.code, "unknown_column");
        assert!(dto.message.contains("ws80"));
    }

    #[test]
    fn a_saved_project_manifest_reads_back() {
        let manifest = r##"{"format": 1, "datasets": [{
            "sourceName": "torre.csv", "sourceBytes": 1234,
            "config": {"delimiter": ";", "columns": [{"fileColumn": 2, "heightM": null}]},
            "restore": {
                "flags": [{"name": "Gelo", "color": "#2a9fc0", "include": false}],
                "segments": [{"column": "ws", "flag": 0, "startStep": 3, "endStep": 9}]
            }
        }]}"##;
        let project: crate::project::ProjectDto = serde_json::from_str(manifest).unwrap();
        let saved = project.datasets.into_iter().next().unwrap();
        assert_eq!((saved.source_name.as_str(), saved.source_bytes), ("torre.csv", 1234));

        let column = |id: &str| model::DataColumn {
            id: id.into(),
            kind: model::ColumnType::WindSpeed,
            subtype: model::Subtype::Mean,
            units: String::new(),
            height_m: None,
            parent: None,
            values: vec![0.0; 10].into(),
            derived: None,
        };
        let mut dataset = model::Dataset {
            name: "torre.csv".into(),
            start: 0,
            step_s: 600,
            len: 10,
            timestamp_meaning: model::TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![column("dir"), column("ws")],
            flags: Vec::new(),
            segments: Vec::new(),
            evidence: Default::default(),
            edits: Vec::new(),
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            displacement: None,
        };
        let (flags, segments) = saved.restore.clone().into_core(&dataset).unwrap();
        assert_eq!(segments, [model::Segment { column: 1, flag: 0, start: 3, end: 9 }], "column resolved by id");
        wind_core::flags::restore(&mut dataset, flags, segments).unwrap();
        assert_eq!(dataset.flags[0].name, "Gelo");

        dataset.columns.pop();
        let error = saved.restore.into_core(&dataset).unwrap_err();
        assert_eq!(error, CoreError::UnknownColumn { id: "ws".into() });
    }

    #[test]
    fn enum_values_match_the_domain_spelling() {
        // The wire spelling of every mirrored enum is the domain's serde spelling.
        let wire = serde_json::to_string(&ColumnKind::from(model::ColumnType::RelativeHumidity)).unwrap();
        let domain = serde_json::to_string(&model::ColumnType::RelativeHumidity).unwrap();
        assert_eq!(wire, domain);
    }

    #[test]
    fn derived_columns_name_their_measured_sources() {
        let csv = "t,WS80,WS80_SD,Temp 80m\n2014-01-01 00:00,8,0.8,15\n2014-01-01 00:10,9,0.9,16\n";
        let config: ImportConfigDto = serde_json::from_str(r#"{"elevationM": 120}"#).unwrap();
        let (mut dataset, _) = wind_core::import::import_text("t", csv, &config.into_core()).unwrap();
        wind_core::derived::append(&mut dataset);
        let json = serde_json::to_value(DatasetDto::new(DatasetId(0), &dataset)).unwrap();
        let columns = json["columns"].as_array().unwrap();
        assert!(columns[0].get("derivedFrom").is_none(), "a measured column's JSON is unchanged");
        let power = columns.iter().find(|column| column["id"] == "WS80 WPD").unwrap();
        assert_eq!(power["derivedFrom"], serde_json::json!(["WS80", "Temp 80m"]), "through the air density");
        assert_eq!(power["kind"], "power_density");
        let export: ExportCsvRequest = serde_json::from_str(r#"{"dataset": 0, "delimiter": ",", "includeDerived": true}"#).unwrap();
        assert!(export.options().include_derived);
    }
}

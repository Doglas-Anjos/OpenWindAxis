//! What callers send. Every request rejects unknown fields, so a typo in the
//! UI fails on the first call instead of being silently ignored.
use crate::filter::FilterDto;
use crate::ops::EditDto;
use crate::responses::{FlagDto, SegmentDto};
use crate::rules::EvidenceDto;
use crate::{ColumnKind, ColumnSubtype, DatasetId, ShearAggregation, TimestampMeaning, WeibullMethod};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use wind_core::displacement::Displacement;
use wind_core::error::{CoreError, CoreResult};
use wind_core::export::CsvExportOptions;
use wind_core::import::{ColumnOverride, ImportConfig, DEFAULT_MISSING_TOKENS};
use wind_core::model::{Dataset, ExtrapolateFrom, Extrapolation, Flag, PrimaryChoice, Segment};

/// Speed classes of a wind rose when the request names none, m/s.
pub const DEFAULT_ROSE_SPEED_EDGES: [f64; 5] = [3.0, 6.0, 9.0, 12.0, 15.0];

/// Import a delimited text file. The file's bytes travel next to this
/// request (as a byte array), not inside it.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportRequest {
    /// Display name, usually the file name.
    pub name: String,
    /// How to read the file; omitted fields are detected.
    #[serde(default)]
    #[ts(optional)]
    pub config: Option<ImportConfigDto>,
    /// Re-import into this dataset, keeping its flags, instead of adding one.
    #[serde(default)]
    #[ts(optional)]
    pub replace: Option<DatasetId>,
    /// Flags saved with a project, put back after reading; they win over the
    /// flags `replace` carries over.
    #[serde(default)]
    #[ts(optional)]
    pub restore: Option<RestoreDto>,
    /// Files appended to the first one, in order; their bytes follow its
    /// bytes in the buffer, and each is read with the same configuration
    /// (Arquivo › Anexar dados, [`crate::append`]).
    #[serde(default)]
    #[ts(optional)]
    pub appended: Option<Vec<crate::append::AppendedSourceDto>>,
}

/// Finishes an import as every adapter does: puts back the flags a project
/// saved (`restore`), then adds the derived columns. In that order, so a
/// derived column never carries flags of its own (it inherits its sources').
/// A re-import carries its old flags over ([`wind_core::flags::carry_over`])
/// before calling this.
///
/// # Errors
/// Those of [`RestoreDto::put_back`]; drop `dataset` then.
///
/// The order matters: the value edits first (a re-read file or a saved
/// project replays them, and a time shift moves the clock the derived
/// columns' month and hour tables read), then the derived columns, then the
/// flags (a synthesized column can carry one of its own), and last the
/// calculated columns, which read their sources under those flags.
/// `replacing` is the dataset a re-read file replaces: its edits and flags
/// carry over.
pub fn finish_import(dataset: &mut Dataset, restore: Option<RestoreDto>, replacing: Option<&Dataset>) -> CoreResult<()> {
    if let Some(old) = replacing {
        wind_core::edits::carry_over(old, dataset)?;
    }
    let restore = match restore {
        Some(mut restore) => {
            restore.replay_edits(dataset)?;
            Some(restore)
        }
        None => None,
    };
    wind_core::derived::append(dataset);
    if let Some(old) = replacing {
        wind_core::flags::carry_over(old, dataset);
    }
    if let Some(restore) = restore {
        restore.put_back(dataset)?;
    }
    wind_core::calculated::refresh(dataset);
    Ok(())
}

/// A dataset's flags and segments as a project saved them.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreDto {
    /// The dataset's flags, in order.
    pub flags: Vec<FlagDto>,
    /// Flagged stretches, columns by id.
    pub segments: Vec<SegmentDto>,
    /// Why flag rules flagged what they flagged; absent in projects saved
    /// before flag rules existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub evidence: Option<Vec<EvidenceDto>>,
    /// The value edits in effect, oldest first, replayed after the flags;
    /// absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub edits: Option<Vec<EditDto>>,
}

impl RestoreDto {
    /// The core's flags and segments, with column ids resolved in `dataset`.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a segment on a column `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<(Vec<Flag>, Vec<Segment>)> {
        let segments = self
            .segments
            .into_iter()
            .map(|segment| {
                let column = dataset
                    .columns
                    .iter()
                    .position(|column| column.id == segment.column)
                    .ok_or(CoreError::UnknownColumn { id: segment.column })?;
                Ok(Segment { column, flag: segment.flag, start: segment.start_step, end: segment.end_step })
            })
            .collect::<CoreResult<_>>()?;
        Ok((self.flags.into_iter().map(Flag::from).collect(), segments))
    }
}

/// How to read a file; every field left out is detected.
#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ImportConfigDto {
    /// One-character field delimiter.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<char>,
    /// Values written as "5,25".
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimal_comma: Option<bool>,
    /// Rows before the header row (logger preamble).
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_rows: Option<usize>,
    /// File columns holding the timestamp; two = separate date and time.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_columns: Option<Vec<usize>>,
    /// chrono format of the timestamps, e.g. "%d/%m/%Y %H:%M".
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_format: Option<String>,
    /// Which instant of each interval the file's timestamps mark.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_meaning: Option<TimestampMeaning>,
    /// Time step in seconds.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_s: Option<i64>,
    /// Tokens read as missing; omitted = the defaults ("", "NaN", "-999", ...).
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<Vec<String>>,
    /// Corrections to the detected columns.
    pub columns: Vec<ColumnCorrectionDto>,
    /// Site elevation above sea level, m (−500 to 9000). With a temperature
    /// column and no pressure column, the air density needs it.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevation_m: Option<f64>,
    /// Site latitude, degrees north; omitted = from the logger preamble.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    /// Site longitude, degrees east; omitted = from the logger preamble.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    /// Speeds to extrapolate to other heights (Revisar › Extrapolação
    /// vertical), each a derived column; omitted = none.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extrapolations: Option<Vec<ExtrapolationDto>>,
    /// The mean speed below which TI is undefined, m/s (the IEC cut, 4);
    /// omitted = the original's rule, σ/U from 0.01 m/s and 0 below.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ti_cutoff: Option<f64>,
    /// Calculated columns (Configurar dataset › Adicionar coluna), in
    /// order, after every other column; omitted = none.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculated: Option<Vec<crate::calculated::CalculatedColumnDto>>,
    /// How the primary column of each type is picked (Configurar dataset ›
    /// Colunas primárias); types left out, or all when omitted, take the
    /// default: the highest, and for directions the nearest to the primary
    /// speed.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<Vec<PrimaryColumnDto>>,
    /// The displacement height (Configurar dataset › Altura de
    /// deslocamento): the shear laws use heights less it. Omitted = none.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displacement: Option<DisplacementDto>,
    /// The logger clock's offset from UTC, minutes (−180 = UTC−3); omitted
    /// = unknown, or as a recognised file format states it.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_offset_min: Option<i32>,
}

/// The displacement height, m, tagged by `by`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(tag = "by", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum DisplacementDto {
    /// The same in every step.
    Constant {
        /// The height, m.
        height_m: f64,
    },
    /// One per calendar month.
    Month {
        /// Twelve heights, January first, m.
        heights_m: Vec<f64>,
    },
    /// One per sector of a direction column, sector 0 centred on north.
    Sector {
        /// Id of the direction column.
        direction: String,
        /// Height per sector, clockwise from north, m.
        heights_m: Vec<f64>,
        /// Height where the direction is missing, m.
        backup_m: f64,
    },
}

impl From<DisplacementDto> for Displacement {
    fn from(displacement: DisplacementDto) -> Self {
        match displacement {
            DisplacementDto::Constant { height_m } => Displacement::Constant(height_m),
            DisplacementDto::Month { heights_m } => Displacement::Monthly(heights_m),
            DisplacementDto::Sector { direction, heights_m, backup_m } => Displacement::BySector { direction, heights: heights_m, backup: backup_m },
        }
    }
}

/// How the primary column of one type is picked.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrimaryColumnDto {
    /// The type.
    pub kind: ColumnKind,
    /// How.
    pub choice: PrimaryChoiceDto,
}

/// A column by id, or a criterion on height (nearest-in-height rule:
/// sensors within the colocation threshold tie, and the one with the most
/// data wins), tagged by `by`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(tag = "by", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum PrimaryChoiceDto {
    /// This column.
    Column {
        /// Its id.
        column: String,
    },
    /// The highest.
    Highest,
    /// The lowest.
    Lowest,
    /// The nearest to a height.
    Nearest {
        /// The height, m.
        height_m: f64,
    },
}

impl From<PrimaryChoiceDto> for PrimaryChoice {
    fn from(choice: PrimaryChoiceDto) -> Self {
        match choice {
            PrimaryChoiceDto::Column { column } => PrimaryChoice::Column(column),
            PrimaryChoiceDto::Highest => PrimaryChoice::Highest,
            PrimaryChoiceDto::Lowest => PrimaryChoice::Lowest,
            PrimaryChoiceDto::Nearest { height_m } => PrimaryChoice::NearestTo(height_m),
        }
    }
}

/// Columns synthesized at another height (Revisar › Extrapolação vertical;
/// [`wind_core::extrapolation`]): the speed by the power law, its SD, and
/// optionally the direction and the temperature.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtrapolationDto {
    /// Id of the lower measured mean speed column (the two-sensor form;
    /// with `sensors`, its lowest).
    pub lower: String,
    /// Id of the upper measured mean speed column (with `sensors`, its highest).
    pub upper: String,
    /// The height to extrapolate to, m.
    pub height_m: f64,
    /// Use only the month × hour table of shear exponents; omitted = false
    /// (each step's own exponent where the speeds allow it).
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_only: Option<bool>,
    /// The speed columns each step fits; omitted = `lower` and `upper`.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensors: Option<Vec<String>>,
    /// Where the profile passes; omitted = the sensor nearest the height.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<ExtrapolateFromDto>,
    /// `[min, max]` of a step's own exponent; omitted = unrestricted.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_range: Option<(f64, f64)>,
    /// Direction columns for a direction at the height (by the veer).
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vanes: Option<Vec<String>>,
    /// Temperature columns for a temperature at the height (by the gradient).
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermometers: Option<Vec<String>>,
}

/// Where an extrapolated profile passes ("Extrapolar a partir de").
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtrapolateFromDto {
    /// The fitted sensor nearest the height.
    Nearest,
    /// Each step's line of best fit.
    BestFit,
    /// This speed column.
    Sensor {
        /// Its id.
        column: String,
    },
}

impl From<ExtrapolationDto> for Extrapolation {
    fn from(spec: ExtrapolationDto) -> Self {
        Extrapolation {
            sensors: spec.sensors.unwrap_or_default(),
            lower: spec.lower,
            upper: spec.upper,
            height_m: spec.height_m,
            table_only: spec.table_only.unwrap_or(false),
            from: match spec.from {
                None | Some(ExtrapolateFromDto::Nearest) => ExtrapolateFrom::Nearest,
                Some(ExtrapolateFromDto::BestFit) => ExtrapolateFrom::BestFit,
                Some(ExtrapolateFromDto::Sensor { column }) => ExtrapolateFrom::Sensor(column),
            },
            alpha_range: spec.alpha_range,
            vanes: spec.vanes.unwrap_or_default(),
            thermometers: spec.thermometers.unwrap_or_default(),
        }
    }
}

/// The user's correction for one file column.
#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColumnCorrectionDto {
    /// File column index (0-based, counting the timestamp columns).
    pub file_column: usize,
    /// Leave the column out.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<bool>,
    /// Type instead of the guess.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ColumnKind>,
    /// Subtype instead of the guess.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<ColumnSubtype>,
    /// Height in metres: omitted = keep the guess, `null` = no height.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_m: Option<Option<f64>>,
    /// Units instead of the type's default.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Column id instead of the header's name.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// File column of the mean this SD / min / max / gust column belongs to,
    /// instead of the nearest mean of its type and height.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_file_column: Option<usize>,
    /// Bearing of the sensor's boom, degrees clockwise from north (the
    /// original's Calibração › Boom orientation): the sensor is shaded with
    /// the wind from the opposite side. Kept with the configuration for the
    /// tower shading and distortion windows; the import does not read it.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boom_deg: Option<f64>,
}

/// Deserializes a field that is present, even as `null`, into `Some`, so an
/// absent field stays `None`: tells "no correction" from "clear the value".
fn present<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl ImportConfigDto {
    /// The core's import configuration.
    pub fn into_core(self) -> ImportConfig {
        ImportConfig {
            delimiter: self.delimiter,
            decimal_comma: self.decimal_comma,
            skip_rows: self.skip_rows,
            time_columns: self.time_columns,
            time_format: self.time_format.filter(|format| !format.trim().is_empty()),
            timestamp_meaning: self.timestamp_meaning.unwrap_or_default().into(),
            step_s: self.step_s,
            missing: self
                .missing
                .unwrap_or_else(|| DEFAULT_MISSING_TOKENS.iter().map(|token| token.to_string()).collect()),
            columns: self.columns.into_iter().map(ColumnCorrectionDto::into_core).collect(),
            elevation_m: self.elevation_m,
            latitude: self.latitude,
            longitude: self.longitude,
            extrapolations: self
                .extrapolations
                .unwrap_or_default()
                .into_iter()
                .map(Extrapolation::from)
                .collect(),
            calculated: self.calculated.unwrap_or_default().into_iter().map(Into::into).collect(),
            ti_cutoff_m_s: self.ti_cutoff.filter(|cutoff| *cutoff > 0.0),
            primary: self.primary.unwrap_or_default().into_iter().map(|primary| (primary.kind.into(), primary.choice.into())).collect(),
            displacement: self.displacement.map(Into::into),
            utc_offset_min: self.utc_offset_min,
        }
    }
}

impl ColumnCorrectionDto {
    fn into_core(self) -> ColumnOverride {
        ColumnOverride {
            index: self.file_column,
            skip: self.skip.unwrap_or(false),
            kind: self.kind.map(Into::into),
            subtype: self.subtype.map(Into::into),
            height_m: self.height_m,
            units: self.units,
            name: self.name,
            parent: self.parent_file_column,
        }
    }
}

/// A decimated series for a chart over a time window.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeriesRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// Window start, epoch ms.
    pub from_ms: f64,
    /// Window end, epoch ms.
    pub to_ms: f64,
    /// At most two points (min and max) per bucket come back.
    pub buckets: u32,
}

/// Flag the steps between two instants on some columns.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column ids; their SD / min / max / gust children follow.
    pub columns: Vec<String>,
    /// Index into the dataset's flags.
    pub flag: usize,
    /// Window start, epoch ms.
    pub from_ms: f64,
    /// Window end, epoch ms.
    pub to_ms: f64,
}

/// Replace a dataset's flags ("Definir flags", ESPEC-INTERFACE §5.2): edit,
/// add, reorder or leave out. Segments and evidence follow their flag; those
/// of a flag left out go with it, and the undo history restarts then.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefineFlagsRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Its flags from now on, in order.
    pub flags: Vec<FlagDefinitionDto>,
}

/// One flag of a [`DefineFlagsRequest`].
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagDefinitionDto {
    /// Name, colour, treatment and notes.
    pub flag: FlagDto,
    /// The index it had; omitted = a new flag.
    #[serde(default)]
    #[ts(optional)]
    pub was: Option<usize>,
}

impl DefineFlagsRequest {
    /// The flags as `wind_core::flags::redefine` takes them.
    pub fn into_core(self) -> Vec<(Flag, Option<usize>)> {
        self.flags.into_iter().map(|definition| (definition.flag.into(), definition.was)).collect()
    }
}

/// Remove every flag from the steps between two instants on some columns.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnflagRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column ids; their SD / min / max / gust children follow.
    pub columns: Vec<String>,
    /// Window start, epoch ms.
    pub from_ms: f64,
    /// Window end, epoch ms.
    pub to_ms: f64,
    /// Only this flag (index into the dataset's flags); omitted = every flag.
    #[serde(default)]
    #[ts(optional)]
    pub flag: Option<usize>,
}

/// A request about one whole dataset: coverage, metadata export, removal.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetRequest {
    /// The dataset.
    pub dataset: DatasetId,
}

/// Histogram of one column, with an optional Weibull curve.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistogramRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// Bin width in the column's units; must be > 0.
    pub bin_width: f64,
    /// Weibull fit to overlay; omitted = bars only.
    #[serde(default)]
    #[ts(optional)]
    pub weibull: Option<WeibullMethod>,
    /// Also the 95 % intervals of the fit's k and A, by day-block bootstrap:
    /// hundreds of milliseconds on a long record, so only when asked.
    #[serde(default)]
    #[ts(optional)]
    pub confidence: Option<bool>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// Data recovery by month.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoverageRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// Wind rose of a direction column.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindRoseRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Direction column id.
    pub direction: String,
    /// Speed column id, for speed classes, mean speed and energy.
    #[serde(default)]
    #[ts(optional)]
    pub speed: Option<String>,
    /// Number of direction sectors, 1..=360.
    pub sectors: usize,
    /// Upper edges of the speed classes, m/s; omitted = 3, 6, 9, 12, 15.
    #[serde(default)]
    #[ts(optional)]
    pub speed_edges: Option<Vec<f64>>,
    /// Calm threshold, m/s: steps below it are calms, with no direction
    /// (only with `speed`); omitted = 0, no calms.
    #[serde(default)]
    #[ts(optional)]
    pub calm_threshold: Option<f64>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// Mean by hour of day.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiurnalRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// Mean by calendar month.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonthlyMeansRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// Wind shear between speed columns.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShearRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Speed column ids, at two or more heights.
    pub columns: Vec<String>,
    /// Direction column id, for shear by sector.
    #[serde(default)]
    #[ts(optional)]
    pub direction: Option<String>,
    /// Number of direction sectors, 1..=360 (used with `direction`).
    pub sectors: usize,
    /// How each height's speed is averaged; omitted = AP-MoMM, the
    /// reference product's default.
    #[serde(default)]
    #[ts(optional)]
    pub aggregation: Option<ShearAggregation>,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// The dataset as CSV text.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportCsvRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// One-character field separator.
    pub delimiter: char,
    /// Write 5,25 instead of 5.25 (needs a delimiter other than ',').
    #[serde(default)]
    #[ts(optional)]
    pub decimal_comma: Option<bool>,
    /// Written for missing values; omitted = empty field.
    #[serde(default)]
    #[ts(optional)]
    pub missing: Option<String>,
    /// Instant each timestamp marks; omitted = as in the imported file.
    #[serde(default)]
    #[ts(optional)]
    pub timestamp_meaning: Option<TimestampMeaning>,
    /// strftime layout; omitted = "%Y-%m-%d %H:%M:%S".
    #[serde(default)]
    #[ts(optional)]
    pub time_format: Option<String>,
    /// Also write the derived columns (TI, WPD, air density); omitted = no.
    #[serde(default)]
    #[ts(optional)]
    pub include_derived: Option<bool>,
}

impl ExportCsvRequest {
    /// The core's export options.
    pub fn options(&self) -> CsvExportOptions {
        let defaults = CsvExportOptions::default();
        CsvExportOptions {
            delimiter: self.delimiter,
            decimal_comma: self.decimal_comma.unwrap_or(defaults.decimal_comma),
            missing: self.missing.clone().unwrap_or(defaults.missing),
            timestamp_meaning: self.timestamp_meaning.map(Into::into),
            time_format: self.time_format.clone().unwrap_or(defaults.time_format),
            include_derived: self.include_derived.unwrap_or(defaults.include_derived),
        }
    }
}

//! What callers get back. Numbers that may be undefined are `Option` (JSON
//! `null`), never NaN; times are epoch milliseconds; per-column arrays are
//! aligned with `DatasetDto::columns`.
use crate::{finite, finite_all, ColumnKind, ColumnSubtype, DatasetId, ShearAggregation, TimestampMeaning};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::analysis::{Coverage, Diurnal, Histogram, Rose, Shear, WeibullFit};
use wind_core::derived;
use wind_core::history::History;
use wind_core::import::ImportReport;
use wind_core::model::{ColumnType, DataColumn, Dataset, Flag, Segment};
use wind_core::primary;
use wind_core::series::ColumnSummary;
use wind_core::time::Month;

/// Milliseconds in a second.
const MS_PER_SECOND: f64 = 1000.0;

/// A dataset's metadata (the values travel separately, as series).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DatasetDto {
    /// Stable id within the session.
    pub id: DatasetId,
    /// Display name, usually the file name.
    pub name: String,
    /// Start of the first step, epoch ms on the logger's clock.
    pub start_ms: f64,
    /// Length of every step, seconds.
    pub step_s: i64,
    /// Number of steps.
    pub step_count: usize,
    /// What the file's timestamps meant at import.
    pub timestamp_meaning: TimestampMeaning,
    /// In dataset order.
    pub columns: Vec<ColumnDto>,
    /// QC categories; a flag is referred to by its index here.
    pub flags: Vec<FlagDto>,
    /// Flagged stretches, in canonical order.
    pub segments: Vec<SegmentDto>,
    /// Site latitude, degrees north; `null` when unknown.
    pub latitude: Option<f64>,
    /// Site longitude, degrees east; `null` when unknown.
    pub longitude: Option<f64>,
    /// Site elevation above sea level, m; `null` when unknown.
    pub elevation_m: Option<f64>,
    /// The logger clock's offset from UTC, minutes; `null` when unknown.
    pub utc_offset_min: Option<i32>,
    /// The primary column of each type the dataset measures, in column order
    /// of the types (Colunas primárias).
    pub primary: Vec<PrimaryOfKindDto>,
}

/// The primary column of one type.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryOfKindDto {
    /// The type.
    pub kind: ColumnKind,
    /// The column's id.
    pub column: String,
}

/// The primary column of every type `dataset` measures.
fn primary_columns(dataset: &Dataset) -> Vec<PrimaryOfKindDto> {
    let mut kinds: Vec<ColumnType> = Vec::new();
    for column in dataset.columns.iter().filter(|column| column.derived.as_ref().map_or(true, |derived| derived.is_measurement())) {
        if !kinds.contains(&column.kind) {
            kinds.push(column.kind);
        }
    }
    kinds
        .into_iter()
        .filter_map(|kind| primary::primary(dataset, kind).map(|index| PrimaryOfKindDto { kind: kind.into(), column: dataset.columns[index].id.clone() }))
        .collect()
}

impl DatasetDto {
    /// The DTO of `dataset`, known by `id`.
    pub fn new(id: DatasetId, dataset: &Dataset) -> Self {
        DatasetDto {
            id,
            name: dataset.name.clone(),
            start_ms: dataset.start as f64 * MS_PER_SECOND,
            step_s: dataset.step_s,
            step_count: dataset.len,
            timestamp_meaning: dataset.timestamp_meaning.into(),
            columns: dataset.columns.iter().map(|column| ColumnDto::new(dataset, column)).collect(),
            flags: dataset.flags.iter().map(FlagDto::from).collect(),
            segments: segments(dataset),
            latitude: dataset.latitude,
            longitude: dataset.longitude,
            elevation_m: dataset.elevation_m,
            utc_offset_min: dataset.utc_offset_min,
            primary: primary_columns(dataset),
        }
    }
}

/// One data column.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDto {
    /// Unique in the dataset; how requests refer to the column.
    pub id: String,
    /// What the column measures.
    pub kind: ColumnKind,
    /// Statistic over each step.
    pub subtype: ColumnSubtype,
    /// Units as read or corrected; empty when unknown.
    pub units: String,
    /// Measurement height above ground, m.
    pub height_m: Option<f64>,
    /// Id of the mean column an SD / min / max / gust column belongs to.
    pub parent: Option<String>,
    /// Only on derived columns (TI, WPD, air density): ids of the measured
    /// columns it is computed from, whose flags it inherits; it takes none of
    /// its own. Derived columns come after the file's columns and have no
    /// entry in the import report's per-column arrays.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub derived_from: Option<Vec<String>>,
}

impl ColumnDto {
    fn new(dataset: &Dataset, column: &DataColumn) -> Self {
        ColumnDto {
            id: column.id.clone(),
            kind: column.kind.into(),
            subtype: column.subtype.into(),
            units: column.units.clone(),
            height_m: column.height_m,
            parent: column.parent.and_then(|parent| dataset.columns.get(parent)).map(|parent| parent.id.clone()),
            derived_from: column.derived.as_ref().map(|derived| {
                derived::measured_sources(dataset, &derived).into_iter().map(|source| dataset.columns[source].id.clone()).collect()
            }),
        }
    }
}

/// A QC category. Also read back from a saved project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagDto {
    /// Shown in the UI; unique within the dataset.
    pub name: String,
    /// CSS colour.
    pub color: String,
    /// Whether analyses keep data under this flag by default.
    pub include: bool,
    /// What the flag is for; empty in projects saved before notes existed.
    #[serde(default)]
    pub notes: String,
}

impl From<&Flag> for FlagDto {
    fn from(flag: &Flag) -> Self {
        FlagDto { name: flag.name.clone(), color: flag.color.clone(), include: flag.include, notes: flag.notes.clone() }
    }
}

impl From<FlagDto> for Flag {
    fn from(flag: FlagDto) -> Self {
        Flag { name: flag.name, color: flag.color, include: flag.include, notes: flag.notes }
    }
}

/// Steps `startStep..endStep` of a column carry a flag. Also read back from a
/// saved project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SegmentDto {
    /// Column id.
    pub column: String,
    /// Index into `DatasetDto::flags`.
    pub flag: usize,
    /// First flagged step.
    pub start_step: usize,
    /// One past the last flagged step.
    pub end_step: usize,
}

/// The segments of `dataset`, with columns by id.
pub(crate) fn segments(dataset: &Dataset) -> Vec<SegmentDto> {
    let to_dto = |segment: &Segment| SegmentDto {
        column: dataset.columns[segment.column].id.clone(),
        flag: segment.flag,
        start_step: segment.start,
        end_step: segment.end,
    };
    dataset.segments.iter().map(to_dto).collect()
}

/// One column over the whole record, after default filtering.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSummaryDto {
    /// Column id.
    pub column: String,
    /// Steps with a value that survives filtering.
    pub valid: usize,
    /// Share of the record's steps with a valid value, 0..1.
    pub recovery: f64,
    /// Vector mean for directions; `null` without data.
    pub mean: Option<f64>,
    /// Smallest valid value; `null` without any.
    pub min: Option<f64>,
    /// Largest valid value; `null` without any.
    pub max: Option<f64>,
}

/// Summaries of every column of `dataset`, in column order.
pub fn summaries(dataset: &Dataset, summaries: &[ColumnSummary]) -> Vec<ColumnSummaryDto> {
    dataset
        .columns
        .iter()
        .zip(summaries)
        .map(|(column, summary)| ColumnSummaryDto {
            column: column.id.clone(),
            valid: summary.valid,
            recovery: summary.recovery,
            mean: finite(summary.mean),
            min: finite(summary.min),
            max: finite(summary.max),
        })
        .collect()
}

/// What the import detected and every anomaly it met.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportReportDto {
    /// Field delimiter, detected or given.
    pub delimiter: char,
    /// Whether values were read with a decimal comma.
    pub decimal_comma: bool,
    /// Record index of the header row, if the file has one.
    pub header_row: Option<usize>,
    /// File columns holding the timestamp.
    pub time_columns: Vec<usize>,
    /// chrono format the timestamps were read with.
    pub time_format: String,
    /// Time step, s.
    pub step_s: i64,
    /// Non-blank data rows.
    pub rows_read: usize,
    /// Rows dropped for an unparsable timestamp.
    pub rows_bad_time: usize,
    /// Rows dropped because their step was already filled.
    pub duplicates: usize,
    /// Rows not on a whole step from the start.
    pub off_grid: usize,
    /// Steps with no row.
    pub missing_steps: usize,
    /// Per dataset column: fields that were not numbers.
    pub unparsed_by_column: Vec<usize>,
    /// Per dataset column: the file column it came from (for corrections).
    pub file_column_by_column: Vec<usize>,
    /// The file format recognised and rewritten before reading ("NASA
    /// POWER", "Campbell TOA5"); omitted for a plain delimited file.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

impl From<ImportReport> for ImportReportDto {
    fn from(report: ImportReport) -> Self {
        ImportReportDto {
            delimiter: report.delimiter,
            decimal_comma: report.decimal_comma,
            header_row: report.header_row,
            time_columns: report.time_columns,
            time_format: report.time_format,
            step_s: report.step_s,
            rows_read: report.rows_read,
            rows_bad_time: report.rows_bad_time,
            duplicates: report.duplicates,
            off_grid: report.off_grid,
            missing_steps: report.missing_steps,
            unparsed_by_column: report.unparsed,
            file_column_by_column: report.source_index,
            format: report.format,
        }
    }
}

/// Result of an import.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportResultDto {
    /// The dataset as imported.
    pub dataset: DatasetDto,
    /// How the file was read.
    pub report: ImportReportDto,
    /// Per-column summaries under default flag filtering.
    pub summaries: Vec<ColumnSummaryDto>,
    /// What became of each appended file, in order; empty without any.
    pub appended: Vec<crate::append::AppendStatusDto>,
}

/// Flags and summaries after an edit, an undo or a redo.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EditStateDto {
    /// Start of the first step after the edit, epoch ms: a time shift moves it.
    pub start_ms: f64,
    /// The flags after the edit: a reconstruction adds its own.
    pub flags: Vec<FlagDto>,
    /// Flagged stretches after the edit.
    pub segments: Vec<SegmentDto>,
    /// Per-column summaries after the edit.
    pub summaries: Vec<ColumnSummaryDto>,
    /// Whether `undo` has an edit to take back.
    pub can_undo: bool,
    /// Whether `redo` has an undone edit to reapply.
    pub can_redo: bool,
    /// Whether the edit (or the undo or redo) changed values, not only
    /// flags: charts of the values must be drawn again.
    pub values_changed: bool,
}

impl EditStateDto {
    /// The state of `dataset` after an edit, with what its history can undo and redo.
    pub fn new(dataset: &Dataset, column_summaries: &[ColumnSummary], history: &History) -> Self {
        EditStateDto {
            start_ms: dataset.start as f64 * MS_PER_SECOND,
            flags: dataset.flags.iter().map(FlagDto::from).collect(),
            segments: segments(dataset),
            summaries: summaries(dataset, column_summaries),
            can_undo: history.can_undo(),
            can_redo: history.can_redo(),
            values_changed: false,
        }
    }
}

/// A calendar month of the record.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MonthDto {
    /// Calendar year.
    pub year: i32,
    /// 1 = January.
    pub month: u32,
    /// First step of the month.
    pub start_step: usize,
    /// One past its last step.
    pub end_step: usize,
}

impl From<&Month> for MonthDto {
    fn from(month: &Month) -> Self {
        MonthDto { year: month.year, month: month.month, start_step: month.step_start, end_step: month.step_end }
    }
}

/// Data recovery per column, by month and over the record.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDto {
    /// Calendar months of the record, in order.
    pub months: Vec<MonthDto>,
    /// `[column][month]`, 0..1; `null` for a month without steps.
    pub recovery: Vec<Vec<Option<f64>>>,
    /// Per column, over the whole record.
    pub total: Vec<Option<f64>>,
}

impl From<Coverage> for CoverageDto {
    fn from(coverage: Coverage) -> Self {
        CoverageDto {
            months: coverage.months.iter().map(MonthDto::from).collect(),
            recovery: coverage.recovery.iter().map(|monthly| finite_all(monthly)).collect(),
            total: finite_all(&coverage.total),
        }
    }
}

/// Distribution of one column.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HistogramDto {
    /// Samples that passed the filter.
    pub samples: usize,
    /// Mean of the samples; `null` without any.
    pub mean: Option<f64>,
    /// Bin `i` is `[binStart + i·binWidth, binStart + (i+1)·binWidth)`.
    pub bin_start: f64,
    /// Bin width, in the column units.
    pub bin_width: f64,
    /// Share of samples per bin, 0..1.
    pub frequency: Vec<f64>,
    /// Mean wind power density ½·ρ·u³, W/m², with the measured air density
    /// per step where the dataset has one, else 1.225 kg/m³.
    pub power_density_w_m2: Option<f64>,
    /// Mean air density used for the power density, kg/m³.
    pub air_density_kg_m3: f64,
    /// Whether that density came (at least partly) from an air density column.
    pub density_measured: bool,
    /// mean(u³) / mean(u)³.
    pub energy_pattern_factor: Option<f64>,
    /// The fitted curve, when one was asked for.
    pub weibull: Option<WeibullFitDto>,
}

/// A 95 % confidence interval, by day-block bootstrap.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IntervalDto {
    /// Lower end.
    pub low: f64,
    /// Upper end.
    pub high: f64,
}

impl From<wind_core::bootstrap::Interval> for IntervalDto {
    fn from(interval: wind_core::bootstrap::Interval) -> Self {
        IntervalDto { low: interval.low, high: interval.high }
    }
}

/// A fitted Weibull curve.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WeibullFitDto {
    /// Shape parameter k.
    pub k: f64,
    /// Scale parameter A, in the column's units.
    pub a: f64,
    /// Mean of the fitted distribution.
    pub mean: f64,
    /// Fitted share per bin, aligned with `HistogramDto::frequency`.
    pub frequency: Vec<f64>,
    /// Interval of `k`, when the request asked for `confidence` and the
    /// record has at least two days of data.
    pub k_interval: Option<IntervalDto>,
    /// Interval of `a`, likewise.
    pub a_interval: Option<IntervalDto>,
}

impl From<WeibullFit> for WeibullFitDto {
    fn from(fit: WeibullFit) -> Self {
        WeibullFitDto { k: fit.k, a: fit.a, mean: fit.mean, frequency: fit.frequency, k_interval: None, a_interval: None }
    }
}

impl From<Histogram> for HistogramDto {
    fn from(histogram: Histogram) -> Self {
        HistogramDto {
            samples: histogram.samples,
            mean: finite(histogram.mean),
            bin_start: histogram.start,
            bin_width: histogram.width,
            frequency: histogram.frequency,
            power_density_w_m2: finite(histogram.power_density),
            air_density_kg_m3: histogram.air_density,
            density_measured: histogram.density_measured,
            energy_pattern_factor: finite(histogram.energy_pattern_factor),
            weibull: histogram.weibull.map(Into::into),
        }
    }
}

/// Frequency, speed and energy by direction sector; sector 0 is centred on
/// north, counting clockwise.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WindRoseDto {
    /// Steps with a valid direction (and speed, when one was given), calms
    /// included.
    pub samples: usize,
    /// Share of `samples` per sector; with `calm`, they add up to 1.
    pub frequency: Vec<Option<f64>>,
    /// Share of `samples` below the calm threshold, which have no direction.
    pub calm: Option<f64>,
    /// m/s; `null` without a speed column or samples in the sector.
    pub mean_speed: Vec<Option<f64>>,
    /// Share of the total energy Σ½·ρ·u³, with the air density as in the
    /// histogram.
    pub energy: Vec<Option<f64>>,
    /// Upper edges of the speed classes; one open-ended class follows.
    pub speed_edges: Vec<f64>,
    /// `[class][sector]`, share of all samples.
    pub frequency_by_speed: Vec<Vec<Option<f64>>>,
}

impl From<Rose> for WindRoseDto {
    fn from(rose: Rose) -> Self {
        WindRoseDto {
            samples: rose.samples,
            frequency: finite_all(&rose.frequency),
            calm: finite(rose.calm),
            mean_speed: finite_all(&rose.mean_speed),
            energy: finite_all(&rose.energy),
            speed_edges: rose.speed_edges,
            frequency_by_speed: rose.frequency_by_speed.iter().map(|row| finite_all(row)).collect(),
        }
    }
}

/// Mean by hour of day.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiurnalDto {
    /// 24 hourly means over the record.
    pub all: Vec<Option<f64>>,
    /// `[month 0..12][hour]`, January first.
    pub by_month: Vec<Vec<Option<f64>>>,
}

impl From<Diurnal> for DiurnalDto {
    fn from(diurnal: Diurnal) -> Self {
        DiurnalDto { all: finite_all(&diurnal.all), by_month: diurnal.by_month.iter().map(|hours| finite_all(hours)).collect() }
    }
}

/// Mean by calendar month, over every year of the record.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyMeansDto {
    /// January first; `null` for a calendar month without data.
    pub means: Vec<Option<f64>>,
    /// Valid steps per calendar month.
    pub samples: Vec<usize>,
}

impl From<wind_core::analysis::MonthlyMeans> for MonthlyMeansDto {
    fn from(monthly: wind_core::analysis::MonthlyMeans) -> Self {
        MonthlyMeansDto { means: finite_all(&monthly.means), samples: monthly.samples }
    }
}

/// Power-law and log-law shear of a set of speed columns.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShearDto {
    /// Height of each column, in request order.
    pub heights_m: Vec<f64>,
    /// Speed per column over the concurrent steps, averaged by
    /// `aggregation`, m/s: the profile the curves are fitted to.
    pub mean_speeds: Vec<f64>,
    /// How each height's speed was averaged.
    pub aggregation: ShearAggregation,
    /// Steps where every column has data.
    pub samples: usize,
    /// Power-law exponent α of the mean profile.
    pub alpha: Option<f64>,
    /// 95 % interval of `alpha`; `null` with fewer than two days of
    /// concurrent data.
    pub alpha_interval: Option<IntervalDto>,
    /// Log-law roughness length z₀, m; `null` unless speed grows with height.
    pub roughness_length_m: Option<f64>,
    /// Exponent per hour of day, 0–23.
    pub alpha_by_hour: Vec<Option<f64>>,
    /// Empty without a direction column.
    pub alpha_by_sector: Vec<Option<f64>>,
    /// The displacement height the fits took off the heights, averaged over
    /// the concurrent steps, m; 0 without one.
    pub displacement_m: f64,
}

impl From<Shear> for ShearDto {
    fn from(shear: Shear) -> Self {
        ShearDto {
            heights_m: shear.heights,
            mean_speeds: shear.means,
            aggregation: shear.aggregation.into(),
            samples: shear.samples,
            alpha: finite(shear.alpha),
            alpha_interval: shear.alpha_interval.map(Into::into),
            roughness_length_m: finite(shear.z0),
            alpha_by_hour: finite_all(&shear.alpha_by_hour),
            alpha_by_sector: finite_all(&shear.alpha_by_sector),
            displacement_m: shear.displacement_m,
        }
    }
}

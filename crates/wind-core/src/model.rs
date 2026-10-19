//! Domain model: a dataset is a regular time grid holding data columns.
//! Concepts follow publicly documented wind-resource practice (column type,
//! subtype, measurement height, time step); nothing is copied from any product.
//!
//! Field names are Rust names; `#[serde(rename)]` keeps the JSON the web UI
//! reads today until the DTO layer (ARQUITETURA §3) takes over the wire.
use crate::derived::Derived;
use crate::displacement::Displacement;
use crate::edits::Edit;
use crate::rules::Evidence;
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::sync::Arc;

/// What a column measures. Open-ended in practice; `Other` covers the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
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
    /// A sensor's own quality or availability figure (lidar CNR, ...).
    Quality,
    /// Anything else.
    Other,
    /// σ/U of a speed column, dimensionless ([`crate::derived`]).
    TurbulenceIntensity,
    /// ½·ρ·U³, W/m² ([`crate::derived`]).
    PowerDensity,
    /// Mass of air per volume, kg/m³ ([`crate::derived`]).
    AirDensity,
}

impl ColumnType {
    /// Units a new column of this type gets unless the file or user says otherwise.
    pub fn default_units(self) -> &'static str {
        use ColumnType::*;
        match self {
            WindSpeed | VerticalWindSpeed => "m/s",
            WindDirection => "°",
            Temperature => "°C",
            Pressure => "hPa",
            RelativeHumidity => "%",
            SolarRadiation => "W/m²",
            BatteryVoltage => "V",
            TurbineOutput => "kW",
            Quality | Other | TurbulenceIntensity => "",
            PowerDensity => "W/m²",
            AirDensity => "kg/m³",
        }
    }
}

/// Statistic a column holds over each time step (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subtype {
    /// Mean over the step.
    #[default]
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

/// Which instant of the interval a file's timestamp marks. Internally every
/// step is stored by its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeStampMeaning {
    /// The timestamp is the interval's start.
    #[default]
    Start,
    /// The timestamp is the interval's middle.
    Middle,
    /// The timestamp is the interval's end.
    End,
}

impl TimeStampMeaning {
    /// Seconds to add to a file timestamp to get the start of its step
    /// (0, −step/2 or −step).
    pub fn shift(self, step_s: i64) -> i64 {
        match self {
            TimeStampMeaning::Start => 0,
            TimeStampMeaning::Middle => -step_s / 2,
            TimeStampMeaning::End => -step_s,
        }
    }
}

/// One measured or derived series on the dataset's time grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataColumn {
    /// Name from the file, unique within the dataset.
    pub id: String,
    /// What the column measures.
    #[serde(rename = "ctype")]
    pub kind: ColumnType,
    /// Which statistic of each step it holds.
    pub subtype: Subtype,
    /// Free-text units, e.g. "m/s".
    pub units: String,
    /// Measurement height above ground, metres.
    pub height_m: Option<f64>,
    /// Associated mean column, for SD / min / max / gust children.
    pub parent: Option<usize>,
    /// One value per time step; NaN = missing. Shared, not copied, by the
    /// undo history: an edit replaces the whole column, so every revision
    /// that did not change it holds the same allocation (ARQUITETURA §2.3).
    #[serde(skip)]
    pub values: Arc<[f64]>,
    /// How the column was computed from others; `None` for a column read
    /// from the file. Derived columns are rebuilt on every import.
    #[serde(skip)]
    pub derived: Option<Derived>,
}

impl DataColumn {
    /// Direction angles average as vectors; their SD is a plain number.
    pub fn is_angle(&self) -> bool {
        self.kind == ColumnType::WindDirection && self.subtype != Subtype::StdDev
    }
}

/// How a dataset's primary column of one type is picked (Colunas
/// primárias; [`crate::primary`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrimaryChoice {
    /// This column, by id.
    Column(String),
    /// The highest.
    Highest,
    /// The lowest.
    Lowest,
    /// The nearest to this height, m.
    NearestTo(f64),
}

/// A QC category that marks stretches of data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flag {
    /// Shown in the UI; unique within the dataset.
    pub name: String,
    /// CSS colour used to draw flagged stretches.
    pub color: String,
    /// Default filtering keeps data under this flag; otherwise every analysis
    /// reads it as missing.
    pub include: bool,
    /// What the flag is for, as the user wrote it.
    #[serde(default)]
    pub notes: String,
}

/// Steps `start..end` of column `column` carry flag `flag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// Index into `Dataset::columns`.
    #[serde(rename = "col")]
    pub column: usize,
    /// Index into `Dataset::flags`.
    pub flag: usize,
    /// First flagged step.
    #[serde(rename = "i0")]
    pub start: usize,
    /// One past the last flagged step.
    #[serde(rename = "i1")]
    pub end: usize,
}

impl Segment {
    /// The flagged steps.
    pub fn steps(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// Columns synthesized at another height (Revisar › Extrapolação vertical;
/// [`crate::extrapolation`]): the speed by the power law fitted to the
/// measured speeds in each step, its SD when they have one, and optionally
/// the direction by the veer and the temperature by the gradient.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Extrapolation {
    /// Ids of the speed columns each step fits ("Calcular a partir de");
    /// empty = `lower` and `upper`, the two-sensor form that came first.
    #[serde(default)]
    pub sensors: Vec<String>,
    /// Id of the lower speed column of the two-sensor form.
    #[serde(default)]
    pub lower: String,
    /// Id of the upper speed column of the two-sensor form.
    #[serde(default)]
    pub upper: String,
    /// The height to extrapolate to, m.
    pub height_m: f64,
    /// Use only the month × hour table of exponents, never the step's own.
    #[serde(default)]
    pub table_only: bool,
    /// Where the synthesized profile passes.
    #[serde(default)]
    pub from: ExtrapolateFrom,
    /// Limits of a step's own power-law exponent; beyond them, the limit.
    #[serde(default)]
    pub alpha_range: Option<(f64, f64)>,
    /// Ids of the direction columns for a direction at the height; empty = none.
    #[serde(default)]
    pub vanes: Vec<String>,
    /// Ids of the temperature columns for a temperature at the height; empty = none.
    #[serde(default)]
    pub thermometers: Vec<String>,
}

/// Where an extrapolated profile passes ("Extrapolar a partir de").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum ExtrapolateFrom {
    /// Through the fitted sensor nearest the target height.
    #[default]
    Nearest,
    /// Along each step's line of best fit (the nearest sensor's where a step has no fit).
    BestFit,
    /// Through this speed column, by id; the SD through its SD.
    Sensor(String),
}

/// A regular time grid of data columns, with its flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    /// Display name, usually the file name.
    pub name: String,
    /// Start of the first step, seconds since 1970-01-01 on the file's own
    /// clock (treated as UTC while the time zone is unknown).
    pub start: i64,
    /// Length of every step, seconds.
    pub step_s: i64,
    /// Number of steps.
    pub len: usize,
    /// What the file's timestamps meant at import; `start` is already
    /// shifted by it. Kept so a re-import can map rows by file timestamp.
    #[serde(rename = "tsm", default)]
    pub timestamp_meaning: TimeStampMeaning,
    /// Offset of the logger clock from UTC, when known.
    pub utc_offset_min: Option<i32>,
    /// Degrees north.
    pub latitude: Option<f64>,
    /// Degrees east.
    pub longitude: Option<f64>,
    /// Metres above sea level.
    pub elevation_m: Option<f64>,
    /// The data, all `len` long.
    pub columns: Vec<DataColumn>,
    /// QC categories in use.
    pub flags: Vec<Flag>,
    /// Flagged stretches, in canonical order: every edit leaves them as
    /// [`crate::flags::normalize`] does.
    pub segments: Vec<Segment>,
    /// Why flag rules flagged what they flagged, in canonical order
    /// ([`crate::rules`]). Shared, not copied, by the undo history while
    /// unchanged; it travels in project files as DTOs, not through serde here.
    #[serde(skip)]
    pub evidence: Arc<[Evidence]>,
    /// The vertical extrapolations asked for at import; [`crate::derived`]
    /// adds one column each.
    #[serde(skip)]
    pub extrapolations: Vec<Extrapolation>,
    /// The user's calculated columns, from the import configuration
    /// ([`crate::calculated`]); one derived column each, after the others.
    #[serde(skip)]
    pub calculated: Vec<crate::calculated::CalculatedColumn>,
    /// The mean speed below which TI is undefined, m/s; `None` = the
    /// original's rule (0 at or below 0.01 m/s; [`crate::derived`]).
    #[serde(skip)]
    pub ti_cutoff_m_s: Option<f64>,
    /// How the primary column of each type is picked, from the import
    /// configuration; types left out take the default ([`crate::primary`]).
    #[serde(skip)]
    pub primary: Vec<(ColumnType, PrimaryChoice)>,
    /// The displacement height, from the import configuration; `None` = the
    /// heights as measured ([`crate::displacement`]).
    #[serde(skip)]
    pub displacement: Option<Displacement>,
    /// The value edits in effect, oldest first ([`crate::edits`]): replayed
    /// on a re-import and saved in project files, so the edited values
    /// outlive the undo history.
    #[serde(skip)]
    pub edits: Vec<Edit>,
}

impl Dataset {
    /// Start of step `step`, seconds since 1970 on the dataset's clock.
    /// Defined for `step == len` too (the end of the record).
    pub fn time_at(&self, step: usize) -> i64 {
        self.start + step as i64 * self.step_s
    }

    /// The file timestamp of step `step`: its start, undone by the import's
    /// timestamp meaning.
    pub fn stamp_at(&self, step: usize) -> i64 {
        self.time_at(step) - self.timestamp_meaning.shift(self.step_s)
    }

    /// Index of the step containing `time`, clamped to `0..=len`.
    pub fn index_of(&self, time: i64) -> usize {
        if self.step_s <= 0 {
            return 0;
        }
        time.saturating_sub(self.start).div_euclid(self.step_s).clamp(0, self.len as i64) as usize
    }

    /// First step that starts at or after `time`, clamped to `0..=len`.
    pub fn first_step_from(&self, time: i64) -> usize {
        if self.step_s <= 0 {
            return 0;
        }
        let since_start = time.saturating_sub(self.start);
        let partial_step = since_start.rem_euclid(self.step_s) != 0;
        (since_start.div_euclid(self.step_s) + partial_step as i64).clamp(0, self.len as i64) as usize
    }

    /// Steps whose interval overlaps the instants `from..=to`: from the step
    /// containing `from` through the one containing `to`, clamped to the
    /// record. Empty when `to < from` or the window misses the record.
    ///
    /// ```
    /// # use wind_core::model::Dataset;
    /// # let dataset: Dataset = serde_json::from_str(r#"{"name":"d","start":0,"step_s":600,"len":10,
    /// #   "utc_offset_min":null,"latitude":null,"longitude":null,"elevation_m":null,
    /// #   "columns":[],"flags":[],"segments":[]}"#).unwrap();
    /// assert_eq!(dataset.steps_between(650, 1800), 1..4);
    /// assert_eq!(dataset.steps_between(-5000, -1), 0..0, "before the record");
    /// assert_eq!(dataset.steps_between(1800, 650), 0..0, "reversed");
    /// ```
    pub fn steps_between(&self, from: i64, to: i64) -> Range<usize> {
        if self.step_s <= 0 || to < from {
            return 0..0;
        }
        let first = self.index_of(from);
        let step_of_to = to.saturating_sub(self.start).div_euclid(self.step_s);
        let end = step_of_to.saturating_add(1).clamp(0, self.len as i64) as usize;
        first..end.max(first)
    }
}

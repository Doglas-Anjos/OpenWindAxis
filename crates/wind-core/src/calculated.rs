//! Calculated data columns (Configurar dataset › Adicionar coluna; manual
//! def-calculated-data-column.htm and its dlg-cc-*.htm pages; ARQUITETURA
//! 2026-10-18): columns the user defines from others, kept in the import
//! configuration like the extrapolations and rebuilt on every import.
//!
//! Unlike the automatic derived columns ([`crate::derived`]), a calculated
//! column is computed under default flag filtering, as the original's are: a
//! source value flagged to exclude counts as missing (an average of A and B
//! takes B alone where A is flagged), and the column inherits no flag and
//! takes none. So its values follow the flags: [`refresh`] computes them
//! again after the import's flags are in place and after every op; a column
//! whose values did not change keeps its allocation, so the undo history
//! holds only what changed.
//!
//! As the original computes them (its manual and its results):
//! - dew point (Magnus, 17.67 and 243.5 °C, the humidity held in
//!   [10⁻⁶, 1]);
//! - density-adjusted speed U·(ρ/ρ_target)^⅓; energy-weighted density
//!   ρ·WPD / mean WPD;
//! - date and time variables of the middle of the step: hour of day 1–24,
//!   day of the week 0 = Sunday, months 1–12, quarters 1–4.
//!
//! A definition that names a missing column, fits across heights with one
//! height, has an expression that does not parse or more than four columns
//! for it, or takes an id already used, is left out.
use crate::analysis::{veer_rate, RHO_STD};
use crate::derived::{self, Derived, CELSIUS_ZERO_K};
use crate::displacement;
use crate::expression::{Program, VARIABLES};
use crate::filter::FilterView;
use crate::model::{ColumnType, DataColumn, Dataset, Subtype};
use crate::time::SECONDS_PER_HOUR;
use crate::weibull::linfit;
use chrono::{DateTime, Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Magnus constants of the original's dew point: a, and b in °C.
const MAGNUS_A: f64 = 17.67;
const MAGNUS_B_C: f64 = 243.5;
/// The smallest relative humidity share the dew point takes (ln 0 is −∞).
const MIN_HUMIDITY_SHARE: f64 = 1e-6;
const PERCENT: f64 = 100.0;
/// Gradients and veer rates are per 100 m.
const PER_METRES: f64 = 100.0;
/// ½ of ½·ρ·U³.
const HALF: f64 = 0.5;
const CUBE_ROOT: f64 = 1.0 / 3.0;
const DAYS_PER_WEEK: u32 = 7;
const MONTHS_PER_QUARTER: u32 = 3;
/// The day of year after which a leap year's days shift by one (28 February).
const LAST_DAY_BEFORE_LEAP: u32 = 59;
/// A rotor-equivalent speed needs sensors covering this share of the disk.
const REWS_MIN_COVER: f64 = 0.999;
/// The original's "reconstruct and average" settings: 72 direction sectors,
/// a sector's line used from 20 points and an R² of 0.9.
const RECONSTRUCT_SECTORS: usize = 72;
const RECONSTRUCT_MIN_POINTS: usize = 20;
const RECONSTRUCT_MIN_R_SQUARED: f64 = 0.9;
/// Extraterrestrial horizontal radiation above which a step is in daylight, W/m².
const DAYLIGHT_MIN_W_M2: f64 = 0.001;
const DEGREES_PER_TURN: f64 = 360.0;
const HALF_TURN: f64 = DEGREES_PER_TURN / 2.0;
/// Within this many degrees of the best angle the primary boom keeps the step.
const BOOM_TIE_DEG: f64 = 1e-4;

/// How [`Calculation::Combine`] combines its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombineMethod {
    /// Mean of the values present (the vector mean for directions).
    Average,
    /// Median of the values present.
    Median,
    /// The first column with a value, in the order given.
    PrimaryBackup,
    /// Largest value present.
    Maximum,
    /// Smallest value present.
    Minimum,
    /// Sum of the values present.
    Sum,
    /// First minus second.
    Difference,
    /// |first − second|.
    AbsoluteDifference,
    /// First over second.
    Ratio,
    /// First times second.
    Product,
}

/// The period an [`Calculation::Accumulation`] restarts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccumulationPeriod {
    /// Every day.
    Daily,
    /// Every week, from `from` (0 = Sunday).
    Weekly {
        /// 0 = Sunday … 6 = Saturday.
        from: u32,
    },
    /// Every calendar month.
    Monthly,
    /// Every calendar year.
    Yearly,
}

/// One of the solar variables (Duffie and Beckman's formulas, as the
/// original uses them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolarVariable {
    /// Degrees from south, positive to the west.
    Azimuth,
    /// Degrees above the horizon.
    Altitude,
    /// Degrees from the vertical.
    Zenith,
    /// 1 while the sun is up during the step, else 0.
    Daylight,
    /// Clear-sky global horizontal radiation, W/m² (Hottel's beam
    /// transmittance below 2.5 km, Liu and Jordan's diffuse).
    ClearSky,
    /// Extraterrestrial normal radiation, W/m².
    Normal,
    /// Extraterrestrial horizontal radiation averaged over the step, W/m².
    Horizontal,
}

/// One of the date and time variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateTimeVariable {
    /// 1–24, the hour the step's middle falls in, counted from 1.
    HourOfDay,
    /// 0 = Sunday … 6 = Saturday.
    DayOfWeek,
    /// 0–364, a leap year's 29 February sharing 28 February's number.
    DayOfYear,
    /// 1–53, weeks starting on `from`, week 1 holding 1 January.
    WeekOfYear {
        /// 0 = Sunday … 6 = Saturday.
        from: u32,
    },
    /// Hours since midnight, with the fraction.
    TimeOfDay,
    /// 1–12.
    Month,
    /// 1–4.
    Quarter,
}

/// What a calculated column computes; columns are named by id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Calculation {
    /// Average, median, primary/backup, max, min, sum of `columns`, or the
    /// difference, absolute difference, ratio or product of the first two.
    Combine {
        /// How.
        method: CombineMethod,
        /// The columns, in order.
        columns: Vec<String>,
    },
    /// Average, median, primary/backup, max or min, or sum of every measured
    /// column of a type and subtype, whichever the file has at each import.
    CombineType {
        /// How (the pairwise methods take the first two).
        method: CombineMethod,
        /// The type.
        kind: ColumnType,
        /// The subtype.
        subtype: Subtype,
    },
    /// The power-law exponent (or, with `roughness`, the log-law roughness
    /// length, m) of each step's speeds.
    Shear {
        /// Mean speed columns at two or more heights.
        columns: Vec<String>,
        /// The roughness length rather than the exponent.
        roughness: bool,
    },
    /// The power-law exponent of each step's speed SDs.
    SpeedSdShear {
        /// Speed SD columns at two or more heights.
        columns: Vec<String>,
    },
    /// The veer rate of each step, °/100 m.
    Veer {
        /// Direction columns at two or more heights.
        columns: Vec<String>,
    },
    /// The temperature gradient of each step, per 100 m.
    TemperatureGradient {
        /// Temperature columns at two or more heights.
        columns: Vec<String>,
    },
    /// The speed giving the same power density at `target`.
    DensityAdjusted {
        /// Mean speed column.
        speed: String,
        /// Target air density, kg/m³.
        target: f64,
    },
    /// The air density weighted by the power density: ρ·WPD / mean WPD.
    EnergyWeightedDensity {
        /// Mean speed column for the power density.
        speed: String,
    },
    /// Mean of a window of `window` steps, centred or trailing.
    MovingAverage {
        /// The column.
        column: String,
        /// Steps in the window.
        window: usize,
        /// Centred on the step, else ending at it.
        centered: bool,
    },
    /// The value minus the previous step's.
    FirstDifference {
        /// The column.
        column: String,
    },
    /// The running sum of `factor` × the values, restarting every `period`.
    Accumulation {
        /// The column.
        column: String,
        /// When the sum restarts.
        period: AccumulationPeriod,
        /// Scale of each value (1/6 turns 10-min kW into kWh).
        factor: f64,
    },
    /// c₀ + c₁x + c₂x² + … of a column.
    Polynomial {
        /// The column x.
        column: String,
        /// c₀, c₁, …
        coefficients: Vec<f64>,
    },
    /// The piecewise linear function through `points` (x ascending), the
    /// end values beyond them.
    PiecewiseLinear {
        /// The column x.
        column: String,
        /// (x, y) points, two or more.
        points: Vec<(f64, f64)>,
    },
    /// A date or time of each step.
    DateTime {
        /// Which.
        variable: DateTimeVariable,
    },
    /// The dew point, °C.
    DewPoint {
        /// Temperature column (°C, K or °F as its units say).
        temperature: String,
        /// Relative humidity column, %.
        humidity: String,
    },
    /// The inflow angle, degrees: atan of vertical over horizontal speed.
    InflowAngle {
        /// Horizontal speed column.
        horizontal: String,
        /// Vertical speed column.
        vertical: String,
    },
    /// The rotor-equivalent wind speed: the speeds of horizontal slices of
    /// the rotor disk (one per sensor, bounded halfway between sensors)
    /// weighted by their area, as a mean or a cube root mean cube.
    Rews {
        /// Mean speeds at the heights across the rotor.
        columns: Vec<String>,
        /// Hub height, m.
        hub: f64,
        /// Rotor diameter, m.
        diameter: f64,
        /// The cube root of the mean cube, else the mean.
        cube: bool,
    },
    /// Two colocated anemometers averaged, a missing or excluded one
    /// reconstructed from the other by the total least squares line of its
    /// direction sector; a tower-shaded value serves as a predictor only.
    ReconstructAndAverage {
        /// The two mean speeds.
        columns: Vec<String>,
        /// The direction column that picks the sector.
        direction: String,
    },
    /// Two speed SDs or maxima averaged; where one is missing or excluded
    /// and the other only tower-shaded, the shaded value.
    ReconstructAndAverageSdMax {
        /// The two columns.
        columns: Vec<String>,
    },
    /// Of two columns, the one whose boom makes the angle with the wind
    /// nearest `offset` (45° by default; the control on a tie, unless the
    /// primary is at it), or the one with a value.
    CombineByBoom {
        /// Primary and control.
        columns: Vec<String>,
        /// Direction column.
        direction: String,
        /// Boom orientations of the primary and the control, degrees from north.
        booms: (f64, f64),
        /// The best angle between boom and wind, degrees.
        offset: f64,
    },
    /// A solar variable of each step, from the site's latitude, longitude,
    /// elevation and time zone.
    Solar {
        /// Which.
        variable: SolarVariable,
    },
    /// An algebraic expression assigning `X` from `A`–`D`
    /// ([`crate::expression`]).
    Expression {
        /// The statements, in C syntax.
        text: String,
        /// The columns `A`, `B`, `C` and `D` stand for, up to four.
        columns: Vec<String>,
    },
}

/// A column the user defines (Configurar dataset › Adicionar coluna). Its
/// JSON is the wire's `CalculatedColumnDto`, field for field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculatedColumn {
    /// Its id, unique in the dataset.
    pub id: String,
    /// What it computes.
    pub calculation: Calculation,
    /// Its type; `None` = the calculation's own.
    #[serde(default)]
    pub kind: Option<ColumnType>,
    /// Its units; `None` = the calculation's own.
    #[serde(default)]
    pub units: Option<String>,
    /// Its height, m.
    #[serde(default)]
    pub height_m: Option<f64>,
}

/// A calculated column resolved to the columns it reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Calculated {
    /// The definition.
    pub column: CalculatedColumn,
    /// The columns it names, in the order the calculation names them.
    pub sources: Vec<usize>,
    /// The dataset's air density column, for the density-based ones.
    pub density: Option<usize>,
}

impl Calculation {
    /// The ids it names, in order.
    fn names(&self) -> Vec<&str> {
        match self {
            Calculation::Combine { columns, .. }
            | Calculation::Shear { columns, .. }
            | Calculation::SpeedSdShear { columns }
            | Calculation::Veer { columns }
            | Calculation::TemperatureGradient { columns } => columns.iter().map(String::as_str).collect(),
            Calculation::DensityAdjusted { speed, .. } | Calculation::EnergyWeightedDensity { speed } => vec![speed],
            Calculation::MovingAverage { column, .. }
            | Calculation::FirstDifference { column }
            | Calculation::Accumulation { column, .. }
            | Calculation::Polynomial { column, .. }
            | Calculation::PiecewiseLinear { column, .. } => vec![column],
            Calculation::DateTime { .. } => Vec::new(),
            Calculation::DewPoint { temperature, humidity } => vec![temperature, humidity],
            Calculation::InflowAngle { horizontal, vertical } => vec![horizontal, vertical],
            Calculation::Expression { columns, .. }
            | Calculation::Rews { columns, .. }
            | Calculation::ReconstructAndAverageSdMax { columns } => columns.iter().map(String::as_str).collect(),
            Calculation::ReconstructAndAverage { columns, direction } | Calculation::CombineByBoom { columns, direction, .. } => {
                columns.iter().map(String::as_str).chain([direction.as_str()]).collect()
            }
            Calculation::Solar { .. } | Calculation::CombineType { .. } => Vec::new(),
        }
    }

    /// Whether it reads the air density.
    fn reads_density(&self) -> bool {
        matches!(self, Calculation::DensityAdjusted { .. } | Calculation::EnergyWeightedDensity { .. })
    }

    /// Whether its values are fitted across heights.
    fn fits_heights(&self) -> bool {
        matches!(self, Calculation::Shear { .. } | Calculation::SpeedSdShear { .. } | Calculation::Veer { .. } | Calculation::TemperatureGradient { .. })
    }
}

impl Calculated {
    /// The columns it reads.
    pub fn sources(&self) -> Vec<usize> {
        self.sources.iter().copied().chain(self.density).collect()
    }
}

/// The type, units and subtype a calculation gives by itself.
fn natural(dataset: &Dataset, calculation: &Calculation, sources: &[usize]) -> (ColumnType, String) {
    let of_first = || sources.first().map_or((ColumnType::Other, String::new()), |&first| (dataset.columns[first].kind, dataset.columns[first].units.clone()));
    match calculation {
        Calculation::Combine { method, .. } | Calculation::CombineType { method, .. } => match method {
            CombineMethod::Ratio => (ColumnType::Other, String::new()),
            _ => of_first(),
        },
        Calculation::Shear { roughness: true, .. } => (ColumnType::Other, "m".into()),
        Calculation::Shear { .. } | Calculation::SpeedSdShear { .. } | Calculation::DateTime { .. } => (ColumnType::Other, String::new()),
        Calculation::Veer { .. } => (ColumnType::Other, "°/100m".into()),
        Calculation::TemperatureGradient { .. } => {
            let units = sources.first().map_or(String::new(), |&first| format!("{}/100m", dataset.columns[first].units));
            (ColumnType::Other, units)
        }
        Calculation::DensityAdjusted { .. } => (ColumnType::WindSpeed, "m/s".into()),
        Calculation::EnergyWeightedDensity { .. } => (ColumnType::AirDensity, "kg/m³".into()),
        Calculation::DewPoint { .. } => (ColumnType::Temperature, derived::CELSIUS.into()),
        Calculation::InflowAngle { .. } => (ColumnType::Other, "°".into()),
        Calculation::Expression { .. } => (ColumnType::Other, String::new()),
        Calculation::Rews { .. } => (ColumnType::WindSpeed, "m/s".into()),
        Calculation::ReconstructAndAverage { .. } | Calculation::ReconstructAndAverageSdMax { .. } | Calculation::CombineByBoom { .. } => of_first(),
        Calculation::Solar { variable } => match variable {
            SolarVariable::Daylight => (ColumnType::Other, String::new()),
            SolarVariable::ClearSky | SolarVariable::Normal | SolarVariable::Horizontal => (ColumnType::SolarRadiation, "W/m²".into()),
            SolarVariable::Azimuth | SolarVariable::Altitude | SolarVariable::Zenith => (ColumnType::Other, "°".into()),
        },
        Calculation::MovingAverage { .. } | Calculation::FirstDifference { .. } | Calculation::Accumulation { .. } | Calculation::Polynomial { .. } | Calculation::PiecewiseLinear { .. } => {
            of_first()
        }
    }
}

/// The derived column of `spec`, or `None` when a column it names is
/// missing, when it fits across heights with fewer than two heights, or its
/// id is taken. Values are filled by [`refresh`].
pub fn column(dataset: &Dataset, spec: &CalculatedColumn) -> Option<DataColumn> {
    if spec.id.trim().is_empty() || dataset.columns.iter().any(|column| column.id == spec.id) {
        return None;
    }
    let sources: Vec<usize> = match &spec.calculation {
        Calculation::CombineType { kind, subtype, .. } => (0..dataset.columns.len())
            .filter(|&index| {
                let column = &dataset.columns[index];
                column.kind == *kind && column.subtype == *subtype && column.derived.is_none()
            })
            .collect(),
        calculation => calculation.names().into_iter().map(|id| dataset.columns.iter().position(|column| column.id == id)).collect::<Option<_>>()?,
    };
    if matches!(spec.calculation, Calculation::CombineType { .. }) && sources.is_empty() {
        return None;
    }
    let pair = |columns: &Vec<String>| columns.len() == 2;
    let valid = match &spec.calculation {
        Calculation::Expression { text, columns } => columns.len() <= VARIABLES.len() && Program::parse(text).is_ok(),
        Calculation::Rews { columns, hub, diameter, .. } => !columns.is_empty() && *hub > 0.0 && *diameter > 0.0,
        Calculation::ReconstructAndAverage { columns, .. } | Calculation::ReconstructAndAverageSdMax { columns } | Calculation::CombineByBoom { columns, .. } => pair(columns),
        _ => true,
    };
    if !valid {
        return None;
    }
    if spec.calculation.fits_heights() {
        let heights: Vec<f64> = sources.iter().filter_map(|&source| dataset.columns[source].height_m).collect();
        if heights.len() != sources.len() || heights.iter().all(|height| *height == heights[0]) {
            return None;
        }
    }
    let density = if spec.calculation.reads_density() { derived::air_density_column(dataset) } else { None };
    let (kind, units) = natural(dataset, &spec.calculation, &sources);
    Some(DataColumn {
        id: spec.id.clone(),
        kind: spec.kind.unwrap_or(kind),
        subtype: Subtype::Mean,
        units: spec.units.clone().unwrap_or(units),
        height_m: spec.height_m,
        parent: None,
        values: vec![f64::NAN; dataset.len].into(),
        derived: Some(Derived::Calculated(Box::new(Calculated { column: spec.clone(), sources, density }))),
    })
}

/// Computes every calculated column again under the dataset's current flags,
/// in column order (a calculated column may read an earlier one). A column
/// whose values did not change keeps its allocation. O(steps × sources) per
/// calculated column; nothing without one.
pub fn refresh(dataset: &mut Dataset) {
    let calculated: Vec<usize> = (0..dataset.columns.len()).filter(|&index| matches!(dataset.columns[index].derived, Some(Derived::Calculated(_)))).collect();
    if calculated.is_empty() {
        return;
    }
    let view = FilterView::new(dataset);
    for index in calculated {
        let Some(Derived::Calculated(spec)) = &dataset.columns[index].derived else { continue };
        let values = values(dataset, &view, spec);
        let unchanged = values.len() == dataset.columns[index].values.len()
            && values.iter().zip(dataset.columns[index].values.iter()).all(|(new, old)| new.to_bits() == old.to_bits());
        if !unchanged {
            dataset.columns[index].values = Arc::from(values);
        }
    }
}

/// Column `column`'s values with NaN where `view` excludes them.
fn filtered(dataset: &Dataset, view: &FilterView, column: usize) -> Vec<f64> {
    let mut values = dataset.columns[column].values.to_vec();
    for range in view.excluded(column).iter().chain(view.common()) {
        values[range.clone()].fill(f64::NAN);
    }
    values
}

/// `value` in °C from a temperature in `units`; NaN for unknown units.
fn celsius(value: f64, units: &str) -> f64 {
    derived::kelvin_offset(units).map_or(f64::NAN, |offset| value + offset - CELSIUS_ZERO_K)
}

/// The middle of step `step` as a date and time on the dataset's clock.
fn middle_of(dataset: &Dataset, step: usize) -> chrono::NaiveDateTime {
    DateTime::from_timestamp(dataset.time_at(step) + dataset.step_s / 2, 0).unwrap_or_default().naive_utc()
}

fn date_time(variable: DateTimeVariable, moment: chrono::NaiveDateTime) -> f64 {
    let weekday = moment.weekday().num_days_from_sunday();
    match variable {
        DateTimeVariable::HourOfDay => f64::from(moment.hour() + 1),
        DateTimeVariable::DayOfWeek => f64::from(weekday),
        DateTimeVariable::DayOfYear => {
            let ordinal = moment.ordinal();
            let leap = chrono::NaiveDate::from_yo_opt(moment.year(), 366).is_some();
            let shifted = if ordinal > LAST_DAY_BEFORE_LEAP && leap { ordinal - 1 } else { ordinal };
            f64::from(shifted - 1)
        }
        DateTimeVariable::WeekOfYear { from } => {
            let first_weekday = moment.with_ordinal(1).map_or(0, |january| january.weekday().num_days_from_sunday());
            let offset = (first_weekday + DAYS_PER_WEEK - from % DAYS_PER_WEEK) % DAYS_PER_WEEK;
            f64::from((moment.ordinal0() + offset) / DAYS_PER_WEEK + 1)
        }
        DateTimeVariable::TimeOfDay => f64::from(moment.num_seconds_from_midnight()) / SECONDS_PER_HOUR as f64,
        DateTimeVariable::Month => f64::from(moment.month()),
        DateTimeVariable::Quarter => f64::from(moment.month0() / MONTHS_PER_QUARTER + 1),
    }
}

/// Which accumulation period step `step` falls in, as a number that changes
/// exactly when a new period starts.
fn period_of(period: AccumulationPeriod, moment: chrono::NaiveDateTime) -> i64 {
    let days = moment.date().num_days_from_ce() as i64;
    match period {
        AccumulationPeriod::Daily => days,
        AccumulationPeriod::Weekly { from } => {
            let since_first = (i64::from(moment.weekday().num_days_from_sunday()) - i64::from(from % DAYS_PER_WEEK)).rem_euclid(i64::from(DAYS_PER_WEEK));
            days - since_first
        }
        AccumulationPeriod::Monthly => i64::from(moment.year()) * 12 + i64::from(moment.month0()),
        AccumulationPeriod::Yearly => i64::from(moment.year()),
    }
}

/// The slope of a least-squares line through `points` (x, y), or NaN.
fn slope_of(points: &[(f64, f64)]) -> (f64, f64) {
    let (x, y): (Vec<f64>, Vec<f64>) = points.iter().copied().unzip();
    linfit(&x, &y).unwrap_or((f64::NAN, f64::NAN))
}

/// The values of `spec` under `view`.
fn values(dataset: &Dataset, view: &FilterView, spec: &Calculated) -> Vec<f64> {
    let inputs: Vec<Vec<f64>> = spec.sources.iter().map(|&source| filtered(dataset, view, source)).collect();
    let density = || spec.density.map(|column| filtered(dataset, view, column));
    let len = dataset.len;
    let heights: Vec<f64> = spec.sources.iter().map(|&source| dataset.columns[source].height_m.unwrap_or(f64::NAN)).collect();
    let at = |step: usize| inputs.iter().map(move |input| input[step]);
    match &spec.column.calculation {
        Calculation::Combine { method, .. } | Calculation::CombineType { method, .. } => {
            let angles = spec.sources.first().is_some_and(|&first| dataset.columns[first].is_angle());
            (0..len).map(|step| combine(*method, angles, &at(step).collect::<Vec<f64>>())).collect()
        }
        Calculation::Shear { roughness, .. } => {
            let displaced = displacement::by_step(dataset);
            (0..len)
                .map(|step| {
                    let displacement_m = displaced.as_ref().map_or(0.0, |heights| heights[step]);
                    let points: Vec<(f64, f64)> =
                        heights.iter().zip(at(step)).filter(|(height, speed)| **height - displacement_m > 0.0 && *speed > 0.0).map(|(height, speed)| (height - displacement_m, speed)).collect();
                    if *roughness {
                        let logs: Vec<(f64, f64)> = points.iter().map(|(height, speed)| (libm::log(*height), *speed)).collect();
                        match slope_of(&logs) {
                            (slope, intercept) if slope > 0.0 => libm::exp(-intercept / slope),
                            _ => f64::NAN,
                        }
                    } else {
                        slope_of(&points.iter().map(|(height, speed)| (libm::log(*height), libm::log(*speed))).collect::<Vec<_>>()).0
                    }
                })
                .collect()
        }
        Calculation::SpeedSdShear { .. } => (0..len)
            .map(|step| {
                let points: Vec<(f64, f64)> = heights.iter().zip(at(step)).filter(|(_, sd)| *sd > 0.0).map(|(height, sd)| (libm::log(*height), libm::log(sd))).collect();
                slope_of(&points).0
            })
            .collect(),
        Calculation::Veer { .. } => (0..len)
            .map(|step| {
                let (present_heights, directions): (Vec<f64>, Vec<f64>) = heights.iter().copied().zip(at(step)).filter(|(_, direction)| direction.is_finite()).unzip();
                veer_rate(&present_heights, &directions)
            })
            .collect(),
        Calculation::TemperatureGradient { .. } => (0..len)
            .map(|step| {
                let points: Vec<(f64, f64)> = heights.iter().copied().zip(at(step)).filter(|(_, value)| value.is_finite()).collect();
                slope_of(&points).0 * PER_METRES
            })
            .collect(),
        Calculation::DensityAdjusted { target, .. } => {
            let density = density();
            (0..len)
                .map(|step| {
                    let rho = density.as_ref().map_or(RHO_STD, |values| values[step]);
                    if *target > 0.0 { inputs[0][step] * (rho / target).powf(CUBE_ROOT) } else { f64::NAN }
                })
                .collect()
        }
        Calculation::EnergyWeightedDensity { .. } => {
            let density = density();
            let rho_at = |step: usize| density.as_ref().map_or(RHO_STD, |values| values[step]);
            let power: Vec<f64> = (0..len).map(|step| HALF * rho_at(step) * inputs[0][step].powi(3)).collect();
            let present: Vec<f64> = power.iter().copied().filter(|value| value.is_finite()).collect();
            let mean = present.iter().sum::<f64>() / present.len() as f64;
            (0..len).map(|step| if mean > 0.0 { rho_at(step) * power[step] / mean } else { f64::NAN }).collect()
        }
        Calculation::MovingAverage { window, centered, .. } => moving_average(&inputs[0], *window, *centered),
        Calculation::FirstDifference { .. } => {
            (0..len).map(|step| if step == 0 { f64::NAN } else { inputs[0][step] - inputs[0][step - 1] }).collect()
        }
        Calculation::Accumulation { period, factor, .. } => {
            let (mut total, mut current) = (0.0, None);
            (0..len)
                .map(|step| {
                    let period = period_of(*period, middle_of(dataset, step));
                    if current != Some(period) {
                        (total, current) = (0.0, Some(period));
                    }
                    if inputs[0][step].is_finite() {
                        total += factor * inputs[0][step];
                    }
                    total
                })
                .collect()
        }
        Calculation::Polynomial { coefficients, .. } => {
            inputs[0].iter().map(|&x| if x.is_finite() { coefficients.iter().rev().fold(0.0, |sum, coefficient| sum * x + coefficient) } else { f64::NAN }).collect()
        }
        Calculation::PiecewiseLinear { points, .. } => {
            let mut sorted = points.clone();
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
            inputs[0].iter().map(|&x| piecewise(&sorted, x)).collect()
        }
        Calculation::DateTime { variable } => (0..len).map(|step| date_time(*variable, middle_of(dataset, step))).collect(),
        Calculation::DewPoint { .. } => {
            let units = dataset.columns[spec.sources[0]].units.clone();
            (0..len)
                .map(|step| {
                    let (temperature, humidity) = (celsius(inputs[0][step], &units), inputs[1][step]);
                    if !(temperature.is_finite() && humidity.is_finite()) {
                        return f64::NAN;
                    }
                    let share = (humidity / PERCENT).clamp(0.0, 1.0).max(MIN_HUMIDITY_SHARE);
                    let gamma = libm::log(share) + MAGNUS_A * temperature / (temperature + MAGNUS_B_C);
                    MAGNUS_B_C * gamma / (MAGNUS_A - gamma)
                })
                .collect()
        }
        Calculation::InflowAngle { .. } => (0..len).map(|step| libm::atan2(inputs[1][step], inputs[0][step]).to_degrees()).collect(),
        Calculation::Rews { hub, diameter, cube, .. } => rews(&inputs, &heights, *hub, *diameter, *cube),
        Calculation::ReconstructAndAverage { .. } => {
            let shaded = shaded_values(dataset, view, &spec.sources[..2]);
            reconstruct_and_average(&inputs, &shaded)
        }
        Calculation::ReconstructAndAverageSdMax { .. } => {
            let shaded = shaded_values(dataset, view, &spec.sources);
            (0..len)
                .map(|step| {
                    let present: Vec<f64> = inputs.iter().map(|input| input[step]).filter(|value| value.is_finite()).collect();
                    if present.is_empty() {
                        // Neither counts: a tower-shaded one stands in, as the original's.
                        shaded.iter().map(|values| values[step]).find(|value| value.is_finite()).unwrap_or(f64::NAN)
                    } else {
                        present.iter().sum::<f64>() / present.len() as f64
                    }
                })
                .collect()
        }
        Calculation::CombineByBoom { booms, offset, .. } => (0..len)
            .map(|step| {
                let (primary, control, direction) = (inputs[0][step], inputs[1][step], inputs[2][step]);
                match (primary.is_finite(), control.is_finite()) {
                    (true, true) if direction.is_finite() => {
                        let miss = |boom: f64| (((direction - boom + HALF_TURN).rem_euclid(DEGREES_PER_TURN) - HALF_TURN).abs() - offset).abs();
                        // As the original: the control when it is no worse, unless the primary is already at the best angle.
                        if miss(booms.1) <= miss(booms.0) && miss(booms.0) >= BOOM_TIE_DEG { control } else { primary }
                    }
                    (true, true) => f64::NAN,
                    (true, false) => primary,
                    (false, true) => control,
                    (false, false) => f64::NAN,
                }
            })
            .collect(),
        Calculation::Solar { variable } => {
            let Some(site) = Site::of(dataset) else { return vec![f64::NAN; len] };
            (0..len).map(|step| solar(*variable, &site, dataset, step)).collect()
        }
        Calculation::Expression { text, .. } => {
            let Ok(program) = Program::parse(text) else { return vec![f64::NAN; len] };
            let mut variables = [f64::NAN; VARIABLES.len()];
            (0..len)
                .map(|step| {
                    variables.iter_mut().zip(&inputs).for_each(|(variable, input)| *variable = input[step]);
                    program.evaluate(&variables)
                })
                .collect()
        }
    }
}

/// Area of a circle of radius `radius` below a height `above_bottom` over its lowest point.
fn segment_area(radius: f64, above_bottom: f64) -> f64 {
    let height = above_bottom.clamp(0.0, 2.0 * radius);
    let below_centre = radius - height;
    radius * radius * libm::acos(below_centre / radius) - below_centre * (2.0 * radius * height - height * height).max(0.0).sqrt()
}

/// [`Calculation::Rews`]: the slices run from the higher of the rotor top
/// and the top sensor down to the ground, split halfway between sensors;
/// each weighs the share of the disk inside it. A step whose sensors with
/// a value cover less than [`REWS_MIN_COVER`] of the disk gets none.
fn rews(inputs: &[Vec<f64>], heights: &[f64], hub: f64, diameter: f64, cube: bool) -> Vec<f64> {
    let radius = diameter / 2.0;
    let bottom = hub - radius;
    let mut order: Vec<usize> = (0..heights.len()).collect();
    order.sort_by(|a, b| heights[*b].total_cmp(&heights[*a]));
    let disk = std::f64::consts::PI * radius * radius;
    let mut weights = vec![0.0; heights.len()];
    let mut upper = (hub + radius).max(heights[order[0]]);
    for (rank, &sensor) in order.iter().enumerate() {
        let lower = order.get(rank + 1).map_or(0.0, |&next| (heights[sensor] + heights[next]) / 2.0);
        weights[sensor] = (segment_area(radius, upper - bottom) - segment_area(radius, lower - bottom)) / disk;
        upper = lower;
    }
    let len = inputs.first().map_or(0, Vec::len);
    (0..len)
        .map(|step| {
            let (mut total, mut covered) = (0.0, 0.0);
            for (input, weight) in inputs.iter().zip(&weights) {
                let speed = input[step];
                if speed.is_finite() {
                    total += weight * if cube { speed.powi(3) } else { speed };
                    covered += weight;
                }
            }
            if covered < REWS_MIN_COVER {
                return f64::NAN;
            }
            let mean = total / covered;
            if cube { mean.cbrt() } else { mean }
        })
        .collect()
}

/// For each of `columns`, its values where only the tower shading flag
/// keeps them out (NaN elsewhere): what the "reconstruct and average"
/// columns read as predictors.
fn shaded_values(dataset: &Dataset, view: &FilterView, columns: &[usize]) -> Vec<Vec<f64>> {
    let shadow = dataset.flags.iter().position(|flag| flag.name == crate::flags::TOWER_SHADOW_FLAG && !flag.include);
    let kept = shadow.map(|flag| FilterView::keeping(dataset, flag));
    columns
        .iter()
        .map(|&column| match &kept {
            Some(kept) => {
                let (default, with_shadow) = (filtered(dataset, view, column), filtered(dataset, kept, column));
                default.iter().zip(&with_shadow).map(|(default, shaded)| if default.is_nan() { *shaded } else { f64::NAN }).collect()
            }
            None => vec![f64::NAN; dataset.len],
        })
        .collect()
}

/// A total least squares line `b = slope·a + offset` and its R².
#[derive(Debug, Clone, Copy)]
struct Orthogonal {
    slope: f64,
    offset: f64,
    count: usize,
    r_squared: f64,
}

/// The total least squares line through `points`, or `None` without spread.
fn orthogonal(points: &[(f64, f64)]) -> Option<Orthogonal> {
    let count = points.len();
    if count < 2 {
        return None;
    }
    let n = count as f64;
    let (mean_a, mean_b) = points.iter().fold((0.0, 0.0), |(a, b), point| (a + point.0 / n, b + point.1 / n));
    let (mut s_aa, mut s_bb, mut s_ab) = (0.0, 0.0, 0.0);
    for (a, b) in points {
        s_aa += (a - mean_a).powi(2);
        s_bb += (b - mean_b).powi(2);
        s_ab += (a - mean_a) * (b - mean_b);
    }
    if s_ab == 0.0 || s_aa == 0.0 || s_bb == 0.0 {
        return None;
    }
    let slope = (s_bb - s_aa + ((s_bb - s_aa).powi(2) + 4.0 * s_ab * s_ab).sqrt()) / (2.0 * s_ab);
    Some(Orthogonal { slope, offset: mean_b - slope * mean_a, count, r_squared: s_ab * s_ab / (s_aa * s_bb) })
}

/// [`Calculation::ReconstructAndAverage`] over `inputs` (a, b, direction,
/// filtered) and `shaded` (a and b where only the shading flag excludes).
/// The lines by sector use every step where both read (counted or shaded);
/// a sector's line reconstructs only with [`RECONSTRUCT_MIN_POINTS`] points
/// and an R² of [`RECONSTRUCT_MIN_R_SQUARED`], as the original's settings.
fn reconstruct_and_average(inputs: &[Vec<f64>], shaded: &[Vec<f64>]) -> Vec<f64> {
    let (a, b, direction) = (&inputs[0], &inputs[1], &inputs[2]);
    let len = a.len();
    let read = |values: &[f64], shade: &[f64], step: usize| if values[step].is_finite() { values[step] } else { shade[step] };
    let mut by_sector: Vec<Vec<(f64, f64)>> = vec![Vec::new(); RECONSTRUCT_SECTORS];
    for step in 0..len {
        let (x, y) = (read(a, &shaded[0], step), read(b, &shaded[1], step));
        if x.is_finite() && y.is_finite() && direction[step].is_finite() {
            by_sector[crate::analysis::sector_of(direction[step], RECONSTRUCT_SECTORS)].push((x, y));
        }
    }
    let lines: Vec<Option<Orthogonal>> = by_sector
        .iter()
        .map(|points| orthogonal(points).filter(|line| line.count >= RECONSTRUCT_MIN_POINTS && line.r_squared >= RECONSTRUCT_MIN_R_SQUARED && line.slope != 0.0))
        .collect();
    (0..len)
        .map(|step| {
            let line = direction[step].is_finite().then(|| lines[crate::analysis::sector_of(direction[step], RECONSTRUCT_SECTORS)]).flatten();
            let a_value = if a[step].is_finite() {
                Some(a[step])
            } else if shaded[0][step].is_nan() {
                line.and_then(|line| Some(read(b, &shaded[1], step)).filter(|value| value.is_finite()).map(|value| (value - line.offset) / line.slope))
            } else {
                None
            };
            let b_value = if b[step].is_finite() {
                Some(b[step])
            } else if shaded[1][step].is_nan() {
                line.and_then(|line| Some(read(a, &shaded[0], step)).filter(|value| value.is_finite()).map(|value| line.slope * value + line.offset))
            } else {
                None
            };
            match (a_value, b_value) {
                (Some(a), Some(b)) => (a + b) / 2.0,
                (Some(value), None) | (None, Some(value)) => value,
                (None, None) => f64::NAN,
            }
        })
        .collect()
}

/// The site the solar variables need.
struct Site {
    latitude: f64,
    longitude: f64,
    elevation_m: f64,
    utc_offset_h: f64,
}

impl Site {
    /// The dataset's site, or `None` without a latitude and longitude.
    fn of(dataset: &Dataset) -> Option<Site> {
        Some(Site {
            latitude: dataset.latitude?.to_radians(),
            longitude: dataset.longitude?,
            elevation_m: dataset.elevation_m.unwrap_or(0.0),
            utc_offset_h: f64::from(dataset.utc_offset_min.unwrap_or(0)) / MINUTES_PER_HOUR,
        })
    }
}

/// The solar constant, W/m², and the eccentricity factor's amplitude.
const SOLAR_CONSTANT_W_M2: f64 = 1367.0;
const ECCENTRICITY: f64 = 0.033;
const DAYS_PER_YEAR: f64 = 365.0;
const MINUTES_PER_HOUR: f64 = 60.0;
const DEGREES_PER_HOUR: f64 = 15.0;
const HOURS_PER_DAY: f64 = 24.0;
/// Hottel's clear-sky model holds below this elevation, m.
const HOTTEL_MAX_ELEVATION_M: f64 = 2500.0;

/// Extraterrestrial normal radiation on day `day` (1 = 1 January), W/m².
fn normal_radiation(day: f64) -> f64 {
    SOLAR_CONSTANT_W_M2 * (1.0 + ECCENTRICITY * libm::cos(2.0 * std::f64::consts::PI * day / DAYS_PER_YEAR))
}

/// Cooper's declination on day `day`, radians.
fn declination(day: f64) -> f64 {
    23.45_f64.to_radians() * libm::sin(2.0 * std::f64::consts::PI * (284.0 + day) / DAYS_PER_YEAR)
}

/// The equation of time on day `day`, hours (Spencer, as Duffie and Beckman).
fn equation_of_time_h(day: f64) -> f64 {
    let angle = 2.0 * std::f64::consts::PI * (day - 1.0) / DAYS_PER_YEAR;
    229.2 * (0.000075 + 0.001868 * libm::cos(angle) - 0.032077 * libm::sin(angle) - 0.014615 * libm::cos(2.0 * angle) - 0.04089 * libm::sin(2.0 * angle))
        / MINUTES_PER_HOUR
}

/// cos of the zenith angle at `latitude`, `declination` and hour angle `hour_angle`.
fn cos_zenith(latitude: f64, declination: f64, hour_angle: f64) -> f64 {
    libm::sin(declination) * libm::sin(latitude) + libm::cos(declination) * libm::cos(latitude) * libm::cos(hour_angle)
}

/// Extraterrestrial horizontal radiation averaged over `hours` centred on
/// solar time `solar_h`, W/m²: the integral between the hour angles, held
/// inside sunrise and sunset.
fn horizontal_radiation(day: f64, solar_h: f64, hours: f64, latitude: f64) -> f64 {
    let declination = declination(day);
    let tangents = -libm::tan(latitude) * libm::tan(declination);
    let sunset = if tangents > 1.0 { 0.0 } else if tangents < -1.0 { std::f64::consts::PI } else { libm::acos(tangents) };
    let hour_angle = |hours_of: f64| ((hours_of * DEGREES_PER_HOUR - 180.0).to_radians()).clamp(-sunset, sunset);
    let (from, to) = (hour_angle(solar_h - hours / 2.0), hour_angle(solar_h + hours / 2.0));
    let radiation = (libm::cos(latitude) * libm::cos(declination) * (libm::sin(to) - libm::sin(from)) + (to - from) * libm::sin(latitude) * libm::sin(declination))
        * (12.0 / std::f64::consts::PI)
        * normal_radiation(day)
        / hours;
    radiation.max(0.0)
}

/// Solar variable `variable` at the middle of step `step`.
fn solar(variable: SolarVariable, site: &Site, dataset: &Dataset, step: usize) -> f64 {
    let moment = middle_of(dataset, step);
    let day = f64::from(moment.ordinal());
    let clock_h = f64::from(moment.num_seconds_from_midnight()) / SECONDS_PER_HOUR as f64;
    let solar_h = ((site.longitude - site.utc_offset_h * DEGREES_PER_HOUR) / DEGREES_PER_HOUR + clock_h + equation_of_time_h(day)).rem_euclid(HOURS_PER_DAY);
    let hour_angle = (solar_h * DEGREES_PER_HOUR - 180.0).to_radians();
    let declination = declination(day);
    let cos_zenith = cos_zenith(site.latitude, declination, hour_angle).clamp(-1.0, 1.0);
    let zenith = libm::acos(cos_zenith);
    let step_h = dataset.step_s as f64 / SECONDS_PER_HOUR as f64;
    match variable {
        SolarVariable::Zenith => zenith.to_degrees(),
        SolarVariable::Altitude => 90.0 - zenith.to_degrees(),
        SolarVariable::Azimuth => {
            let sin_zenith = libm::sin(zenith);
            let sine = libm::sin(hour_angle) * libm::cos(declination) / sin_zenith;
            if !(sine.abs() < 1.0) {
                return 90.0_f64.copysign(sine);
            }
            let azimuth = libm::asin(sine);
            let toward_pole = (libm::cos(hour_angle) * libm::cos(declination) * libm::sin(site.latitude) - libm::sin(declination) * libm::cos(site.latitude)) / sin_zenith;
            let azimuth = if toward_pole >= 0.0 { azimuth } else { std::f64::consts::PI.copysign(sine) - azimuth };
            azimuth.to_degrees()
        }
        SolarVariable::Daylight => {
            if horizontal_radiation(day, solar_h, step_h, site.latitude) > DAYLIGHT_MIN_W_M2 { 1.0 } else { 0.0 }
        }
        SolarVariable::Normal => normal_radiation(day),
        SolarVariable::Horizontal => horizontal_radiation(day, solar_h, step_h, site.latitude),
        SolarVariable::ClearSky => {
            if cos_zenith <= 0.0 {
                return 0.0;
            }
            let beam = if site.elevation_m <= HOTTEL_MAX_ELEVATION_M {
                let km = site.elevation_m / 1000.0;
                let a0 = 0.4237 - 0.00821 * (6.0 - km).powi(2);
                let a1 = 0.5055 + 0.00595 * (6.5 - km).powi(2);
                let k = 0.2711 + 0.01858 * (2.5 - km).powi(2);
                // Climate corrections r0, r1, rk: tropical, subarctic summer, midlatitude summer or winter.
                let (r0, r1, rk) = if site.latitude.abs() < 23.5_f64.to_radians() {
                    (0.95, 0.98, 1.02)
                } else if site.latitude.abs() >= 60.0_f64.to_radians() {
                    (0.99, 0.99, 1.01)
                } else if declination * site.latitude <= 0.0 {
                    (1.03, 1.01, 1.00)
                } else {
                    (0.97, 0.99, 1.02)
                };
                r0 * a0 + r1 * a1 * libm::exp(-rk * k / cos_zenith)
            } else {
                0.0
            };
            normal_radiation(day) * beam * cos_zenith + horizontal_radiation(day, solar_h, 1.0, site.latitude) * (0.271 - 0.294 * beam)
        }
    }
}

/// One step of [`Calculation::Combine`] over `values` (NaN = missing).
fn combine(method: CombineMethod, angles: bool, values: &[f64]) -> f64 {
    let present: Vec<f64> = values.iter().copied().filter(|value| value.is_finite()).collect();
    let pair = || (values.first().copied().unwrap_or(f64::NAN), values.get(1).copied().unwrap_or(f64::NAN));
    match method {
        CombineMethod::Average if angles => {
            if present.is_empty() {
                return f64::NAN;
            }
            let (east, north) = present.iter().fold((0.0, 0.0), |(east, north), angle| (east + libm::sin(angle.to_radians()), north + libm::cos(angle.to_radians())));
            libm::atan2(east, north).to_degrees().rem_euclid(360.0)
        }
        CombineMethod::Average => if present.is_empty() { f64::NAN } else { present.iter().sum::<f64>() / present.len() as f64 },
        CombineMethod::Median => {
            let mut sorted = present;
            sorted.sort_by(f64::total_cmp);
            match sorted.len() {
                0 => f64::NAN,
                count if count % 2 == 1 => sorted[count / 2],
                count => (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0,
            }
        }
        CombineMethod::PrimaryBackup => present.first().copied().unwrap_or(f64::NAN),
        CombineMethod::Maximum => present.iter().copied().reduce(f64::max).unwrap_or(f64::NAN),
        CombineMethod::Minimum => present.iter().copied().reduce(f64::min).unwrap_or(f64::NAN),
        CombineMethod::Sum => if present.is_empty() { f64::NAN } else { present.iter().sum() },
        CombineMethod::Difference => pair().0 - pair().1,
        CombineMethod::AbsoluteDifference => (pair().0 - pair().1).abs(),
        CombineMethod::Ratio => pair().0 / pair().1,
        CombineMethod::Product => pair().0 * pair().1,
    }
}

/// The moving mean of the present values in a window of `window` steps,
/// centred or ending at each step; NaN where the step itself has no value.
/// O(steps) by running sums.
fn moving_average(values: &[f64], window: usize, centered: bool) -> Vec<f64> {
    let window = window.max(1);
    let mut sums = vec![0.0; values.len() + 1];
    let mut counts = vec![0usize; values.len() + 1];
    for (step, value) in values.iter().enumerate() {
        let present = value.is_finite();
        sums[step + 1] = sums[step] + if present { *value } else { 0.0 };
        counts[step + 1] = counts[step] + usize::from(present);
    }
    let before = if centered { (window - 1) / 2 } else { window - 1 };
    let after = window - 1 - before;
    (0..values.len())
        .map(|step| {
            if !values[step].is_finite() {
                return f64::NAN;
            }
            let (from, to) = (step.saturating_sub(before), (step + after + 1).min(values.len()));
            (sums[to] - sums[from]) / (counts[to] - counts[from]) as f64
        })
        .collect()
}

/// The piecewise linear function through `points` (x ascending) at `x`.
fn piecewise(points: &[(f64, f64)], x: f64) -> f64 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return f64::NAN };
    if !x.is_finite() {
        return f64::NAN;
    }
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    let right = points.partition_point(|point| point.0 <= x);
    let ((x0, y0), (x1, y1)) = (points[right - 1], points[right]);
    if x1 == x0 { y1 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_text, ImportConfig};

    /// Two speeds (20 and 40 m, α = 0.2), a direction pair and a
    /// temperature with humidity, 10-min steps over two days from a Sunday.
    fn dataset(calculated: Vec<CalculatedColumn>) -> Dataset {
        let mut csv = String::from("t,ws20,ws40,wd20,wd40,temp,rh\n");
        for step in 0..288 {
            let speed = 5.0 + (step % 3) as f64;
            csv.push_str(&format!(
                "2023-01-{:02} {:02}:{:02},{speed},{},350,10,20,50\n",
                1 + step / 144,
                (step / 6) % 24,
                (step % 6) * 10,
                speed * 2f64.powf(0.2)
            ));
        }
        let mut data = import_text("t.csv", &csv, &ImportConfig::default()).unwrap().0;
        for (id, kind, height) in [("ws20", ColumnType::WindSpeed, 20.0), ("ws40", ColumnType::WindSpeed, 40.0), ("wd20", ColumnType::WindDirection, 20.0), ("wd40", ColumnType::WindDirection, 40.0)] {
            let column = data.columns.iter_mut().find(|column| column.id == id).unwrap();
            (column.kind, column.height_m) = (kind, Some(height));
        }
        data.columns.iter_mut().find(|column| column.id == "temp").unwrap().kind = ColumnType::Temperature;
        data.columns.iter_mut().find(|column| column.id == "temp").unwrap().units = "°C".into();
        data.columns.iter_mut().find(|column| column.id == "rh").unwrap().kind = ColumnType::RelativeHumidity;
        data.calculated = calculated;
        derived::append(&mut data);
        refresh(&mut data);
        data
    }

    fn calculated(id: &str, calculation: Calculation) -> CalculatedColumn {
        CalculatedColumn { id: id.into(), calculation, kind: None, units: None, height_m: None }
    }

    fn values_of<'a>(data: &'a Dataset, id: &str) -> &'a [f64] {
        &data.columns.iter().find(|column| column.id == id).unwrap().values
    }

    #[test]
    fn combine_follows_the_flags_and_directions_average_as_vectors() {
        let average = calculated("média", Calculation::Combine { method: CombineMethod::Average, columns: vec!["ws20".into(), "ws40".into()] });
        let maximum = calculated("máx", Calculation::CombineType { method: CombineMethod::Maximum, kind: ColumnType::WindSpeed, subtype: Subtype::Mean });
        let directions = calculated("dir", Calculation::Combine { method: CombineMethod::Average, columns: vec!["wd20".into(), "wd40".into()] });
        let mut data = dataset(vec![average, directions, maximum]);
        let (ws20, ws40) = (values_of(&data, "ws20")[0], values_of(&data, "ws40")[0]);
        assert_eq!(values_of(&data, "máx")[0], ws40, "every measured mean speed");
        assert!((values_of(&data, "média")[0] - (ws20 + ws40) / 2.0).abs() < 1e-12);
        assert!(values_of(&data, "dir")[0].abs() < 1e-9 || (values_of(&data, "dir")[0] - 360.0).abs() < 1e-9, "350° and 10° average to north");
        // Flag ws40 on the first step: the average takes ws20 alone, and keeps no flag of its own.
        let column = data.columns.iter().position(|column| column.id == "ws40").unwrap();
        crate::ops::Op::Flag { columns: vec![column], flag: 0, steps: 0..1 }.apply(&mut data).unwrap();
        assert_eq!(values_of(&data, "média")[0], ws20);
        let view = FilterView::new(&data);
        let mean = data.columns.iter().position(|column| column.id == "média").unwrap();
        assert!(view.excluded(mean).is_empty(), "a calculated column inherits no flag");
        assert!(crate::flags::flag(&mut data, &[mean], 0, 0..1).is_err(), "nor takes one");
    }

    #[test]
    fn per_step_fits_and_simple_functions() {
        let data = dataset(vec![
            calculated("alpha", Calculation::Shear { columns: vec!["ws20".into(), "ws40".into()], roughness: false }),
            calculated("veer", Calculation::Veer { columns: vec!["wd20".into(), "wd40".into()] }),
            calculated("dew", Calculation::DewPoint { temperature: "temp".into(), humidity: "rh".into() }),
            calculated("poly", Calculation::Polynomial { column: "ws20".into(), coefficients: vec![1.0, 0.0, 2.0] }),
            calculated("pw", Calculation::PiecewiseLinear { column: "ws20".into(), points: vec![(5.0, 0.0), (7.0, 10.0)] }),
            calculated("hour", Calculation::DateTime { variable: DateTimeVariable::HourOfDay }),
            calculated("weekday", Calculation::DateTime { variable: DateTimeVariable::DayOfWeek }),
            calculated("acc", Calculation::Accumulation { column: "ws20".into(), period: AccumulationPeriod::Daily, factor: 1.0 }),
            calculated("ma", Calculation::MovingAverage { column: "ws20".into(), window: 3, centered: true }),
            calculated("expr", Calculation::Expression { text: "X = A < 6 ? B - A : 0;".into(), columns: vec!["ws20".into(), "ws40".into()] }),
        ]);
        assert!((values_of(&data, "alpha")[0] - 0.2).abs() < 1e-9);
        assert!((values_of(&data, "veer")[0] - 100.0).abs() < 1e-9, "20° over 20 m");
        assert!((values_of(&data, "dew")[0] - 9.27).abs() < 0.01, "20 °C at 50 %: {}", values_of(&data, "dew")[0]);
        assert_eq!(values_of(&data, "poly")[1], 1.0 + 2.0 * 36.0);
        assert_eq!(values_of(&data, "pw")[1], 5.0, "6 m/s halfway between 5 and 7");
        assert_eq!(values_of(&data, "hour")[0], 1.0, "00:05, the middle of the first step, is hour 1");
        assert_eq!(values_of(&data, "weekday")[0], 0.0, "2023-01-01 was a Sunday");
        assert_eq!(values_of(&data, "acc")[2], 5.0 + 6.0 + 7.0);
        assert_eq!(values_of(&data, "ma")[1], 6.0);
        let (ws20, ws40) = (values_of(&data, "ws20")[0], values_of(&data, "ws40")[0]);
        assert_eq!(values_of(&data, "expr")[0], ws40 - ws20);
        assert_eq!(values_of(&data, "expr")[1], 0.0);
    }

    #[test]
    fn rotor_equivalent_speed_by_area() {
        let mut data = dataset(Vec::new());
        // Hub 30 m, 20 m across: 20 and 40 m sensors each take half the disk.
        let (ws20, ws40) = (values_of(&data, "ws20").to_vec(), values_of(&data, "ws40").to_vec());
        let rews = |cube| calculated(if cube { "cubo" } else { "media" }, Calculation::Rews { columns: vec!["ws20".into(), "ws40".into()], hub: 30.0, diameter: 20.0, cube });
        data.calculated = vec![rews(false), rews(true)];
        derived::append(&mut data);
        assert!((values_of(&data, "media")[0] - (ws20[0] + ws40[0]) / 2.0).abs() < 1e-9);
        let cube = ((ws20[0].powi(3) + ws40[0].powi(3)) / 2.0).cbrt();
        assert!((values_of(&data, "cubo")[0] - cube).abs() < 1e-9);
        assert!((segment_area(1.0, 1.0) - std::f64::consts::PI / 2.0).abs() < 1e-12, "half the disk below its centre");
    }

    #[test]
    fn reconstruct_and_average_and_the_boom_combination() {
        let mut data = dataset(vec![
            calculated("rec", Calculation::ReconstructAndAverage { columns: vec!["ws20".into(), "ws40".into()], direction: "wd20".into() }),
            calculated("boom", Calculation::CombineByBoom { columns: vec!["ws20".into(), "ws40".into()], direction: "wd20".into(), booms: (0.0, 180.0), offset: 45.0 }),
        ]);
        let ratio = 2f64.powf(0.2);
        let (ws20, ws40) = (data.columns.iter().position(|column| column.id == "ws20").unwrap(), data.columns.iter().position(|column| column.id == "ws40").unwrap());
        assert_eq!(values_of(&data, "boom")[0], values_of(&data, "ws20")[0], "350° wind: the 0° boom is 10° off, nearer 45° than the 180° one");
        // ws20 missing at step 5: rebuilt from ws40 by the sector's line, then averaged.
        let mut values = data.columns[ws20].values.to_vec();
        values[5] = f64::NAN;
        values[6] = f64::NAN;
        data.columns[ws20].values = values.into();
        // ws40 tower-shaded at step 6: it predicts ws20 but is not averaged.
        let shadow = data.flags.iter().position(|flag| flag.name == crate::flags::TOWER_SHADOW_FLAG).unwrap();
        crate::ops::Op::Flag { columns: vec![ws40], flag: shadow, steps: 6..7 }.apply(&mut data).unwrap();
        let b = data.columns[ws40].values.to_vec();
        assert!((values_of(&data, "rec")[5] - (b[5] / ratio + b[5]) / 2.0).abs() < 1e-9);
        assert!((values_of(&data, "rec")[6] - b[6] / ratio).abs() < 1e-9);
        assert_eq!(values_of(&data, "boom")[5], b[5], "only the control has a value");
    }

    #[test]
    fn the_sun_on_the_equator() {
        let mut data = dataset(Vec::new());
        (data.latitude, data.longitude, data.utc_offset_min) = (Some(0.0), Some(0.0), Some(0));
        let solar = |name: &str, variable| calculated(name, Calculation::Solar { variable });
        data.calculated = vec![solar("alt", SolarVariable::Altitude), solar("day", SolarVariable::Daylight), solar("normal", SolarVariable::Normal), solar("clear", SolarVariable::ClearSky)];
        derived::append(&mut data);
        let highest = values_of(&data, "alt")[..144].iter().copied().fold(f64::MIN, f64::max);
        assert!((highest - 67.0).abs() < 0.5, "1 January at noon, the sun 23° south of the zenith: {highest}");
        let (midnight, noon) = (0, 72);
        assert_eq!((values_of(&data, "day")[midnight], values_of(&data, "day")[noon]), (0.0, 1.0));
        assert!((values_of(&data, "normal")[0] - 1412.0).abs() < 1.0, "{}", values_of(&data, "normal")[0]);
        assert!(values_of(&data, "clear")[noon] > 700.0 && values_of(&data, "clear")[noon] < 1100.0, "{}", values_of(&data, "clear")[noon]);
        assert_eq!(values_of(&data, "clear")[midnight], 0.0);
    }

    #[test]
    fn bad_definitions_are_left_out() {
        let data = dataset(vec![
            calculated("ws20", Calculation::FirstDifference { column: "ws40".into() }),
            calculated("nada", Calculation::FirstDifference { column: "nope".into() }),
            calculated("one height", Calculation::Shear { columns: vec!["ws20".into(), "ws20".into()], roughness: false }),
        ]);
        assert!(!data.columns.iter().any(|column| column.id == "nada" || column.id == "one height"));
        assert_eq!(data.columns.iter().filter(|column| column.id == "ws20").count(), 1, "a taken id");
    }
}

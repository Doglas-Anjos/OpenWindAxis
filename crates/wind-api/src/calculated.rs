//! Calculated columns on the wire ([`wind_core::calculated`]): the
//! definitions the import configuration carries. Each type has the JSON of
//! its core twin, field for field (one-word fields, so the casing never
//! differs), and converts through it; the contract test below keeps the two
//! from drifting apart.
use crate::{ColumnKind, ColumnSubtype};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::calculated::CalculatedColumn;

/// How a combined column combines its columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CombineMethodDto {
    /// Mean of the values present (vector mean for directions).
    Average,
    /// Median of the values present.
    Median,
    /// The first column with a value.
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

/// When an accumulation restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccumulationPeriodDto {
    /// Every day.
    Daily,
    /// Every week, from day `from` (0 = Sunday).
    Weekly {
        /// 0 = Sunday … 6 = Saturday.
        from: u32,
    },
    /// Every calendar month.
    Monthly,
    /// Every calendar year.
    Yearly,
}

/// A date or time of each step (of its middle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DateTimeVariableDto {
    /// 1–24.
    HourOfDay,
    /// 0 = Sunday … 6 = Saturday.
    DayOfWeek,
    /// 0–364.
    DayOfYear,
    /// 1–53, weeks starting on day `from` (0 = Sunday).
    WeekOfYear {
        /// 0 = Sunday … 6 = Saturday.
        from: u32,
    },
    /// Hours since midnight.
    TimeOfDay,
    /// 1–12.
    Month,
    /// 1–4.
    Quarter,
}

/// A solar variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SolarVariableDto {
    /// Degrees from south, positive to the west.
    Azimuth,
    /// Degrees above the horizon.
    Altitude,
    /// Degrees from the vertical.
    Zenith,
    /// 1 while the sun is up, else 0.
    Daylight,
    /// Clear-sky global horizontal radiation, W/m².
    ClearSky,
    /// Extraterrestrial normal radiation, W/m².
    Normal,
    /// Extraterrestrial horizontal radiation, W/m².
    Horizontal,
}

/// What a calculated column computes, columns by id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CalculationDto {
    /// Average, median, primary/backup, max, min, sum of `columns`, or the
    /// difference, absolute difference, ratio or product of the first two.
    Combine {
        /// How.
        method: CombineMethodDto,
        /// The columns, in order.
        columns: Vec<String>,
    },
    /// Average, median, primary/backup, max, min or sum of every measured
    /// column of a type and subtype.
    CombineType {
        /// How.
        method: CombineMethodDto,
        /// The type.
        kind: ColumnKind,
        /// The subtype.
        subtype: ColumnSubtype,
    },
    /// Each step's power-law exponent, or log-law roughness length (m).
    Shear {
        /// Mean speeds at two or more heights.
        columns: Vec<String>,
        /// The roughness length rather than the exponent.
        roughness: bool,
    },
    /// Each step's power-law exponent of the speed SDs.
    SpeedSdShear {
        /// Speed SDs at two or more heights.
        columns: Vec<String>,
    },
    /// Each step's veer rate, °/100 m.
    Veer {
        /// Directions at two or more heights.
        columns: Vec<String>,
    },
    /// Each step's temperature gradient, per 100 m.
    TemperatureGradient {
        /// Temperatures at two or more heights.
        columns: Vec<String>,
    },
    /// The speed of the same power density at air density `target`.
    DensityAdjusted {
        /// Mean speed.
        speed: String,
        /// Target air density, kg/m³.
        target: f64,
    },
    /// ρ·WPD / mean WPD.
    EnergyWeightedDensity {
        /// Mean speed for the power density.
        speed: String,
    },
    /// Mean of a window of steps.
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
    /// Running sum of `factor` × the values, restarting every `period`.
    Accumulation {
        /// The column.
        column: String,
        /// When it restarts.
        period: AccumulationPeriodDto,
        /// Scale of each value.
        factor: f64,
    },
    /// c₀ + c₁x + c₂x² + …
    Polynomial {
        /// The column x.
        column: String,
        /// c₀, c₁, …
        coefficients: Vec<f64>,
    },
    /// Piecewise linear through `points`, the end values beyond them.
    PiecewiseLinear {
        /// The column x.
        column: String,
        /// (x, y) points, two or more.
        points: Vec<(f64, f64)>,
    },
    /// A date or time of each step.
    DateTime {
        /// Which.
        variable: DateTimeVariableDto,
    },
    /// The dew point, °C.
    DewPoint {
        /// Temperature.
        temperature: String,
        /// Relative humidity, %.
        humidity: String,
    },
    /// atan(vertical / horizontal), degrees.
    InflowAngle {
        /// Horizontal speed.
        horizontal: String,
        /// Vertical speed.
        vertical: String,
    },
    /// The rotor-equivalent wind speed of slices of the rotor disk.
    Rews {
        /// Mean speeds across the rotor.
        columns: Vec<String>,
        /// Hub height, m.
        hub: f64,
        /// Rotor diameter, m.
        diameter: f64,
        /// Cube root of the mean cube, else the mean.
        cube: bool,
    },
    /// Two colocated anemometers averaged, a missing one reconstructed.
    ReconstructAndAverage {
        /// The two mean speeds.
        columns: Vec<String>,
        /// Direction for the sector.
        direction: String,
    },
    /// Two speed SDs or maxima averaged, a tower-shaded one standing in.
    ReconstructAndAverageSdMax {
        /// The two columns.
        columns: Vec<String>,
    },
    /// The column whose boom makes the angle with the wind nearest `offset`.
    CombineByBoom {
        /// Primary and control.
        columns: Vec<String>,
        /// Direction.
        direction: String,
        /// Boom orientations of the primary and the control, degrees.
        booms: (f64, f64),
        /// Best angle between boom and wind, degrees.
        offset: f64,
    },
    /// A solar variable of each step.
    Solar {
        /// Which.
        variable: SolarVariableDto,
    },
    /// An algebraic expression assigning `X` from `A`–`D`.
    Expression {
        /// The statements, in C syntax.
        text: String,
        /// The columns `A`–`D` stand for, up to four.
        columns: Vec<String>,
    },
}

/// An algebraic expression to check as it is typed.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpressionCheckRequest {
    /// The statements.
    pub text: String,
}

/// Whether an expression parses: `null` message when it does.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExpressionCheckDto {
    /// What does not parse; `null` = it parses.
    pub message: Option<String>,
    /// Character offset of the problem.
    pub position: Option<usize>,
    /// Which of `A`–`D` it names.
    pub uses: Vec<bool>,
}

/// Parses `request`'s expression.
pub fn check_expression(request: ExpressionCheckRequest) -> ExpressionCheckDto {
    match wind_core::expression::Program::parse(&request.text) {
        Ok(program) => ExpressionCheckDto { message: None, position: None, uses: program.uses().to_vec() },
        Err(error) => ExpressionCheckDto { message: Some(error.message), position: Some(error.position), uses: Vec::new() },
    }
}

/// A column the user defines (Configurar dataset › Adicionar coluna).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalculatedColumnDto {
    /// Its id, unique in the dataset.
    pub id: String,
    /// What it computes.
    pub calculation: CalculationDto,
    /// Its type; omitted = the calculation's own.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ColumnKind>,
    /// Its units; omitted = the calculation's own.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Its height, m.
    #[serde(default)]
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_m: Option<f64>,
}

impl From<CalculatedColumnDto> for CalculatedColumn {
    fn from(dto: CalculatedColumnDto) -> Self {
        let json = serde_json::to_value(dto).expect("plain fields");
        serde_json::from_value(json).expect("the same JSON as the core's: see the contract test")
    }
}

#[cfg(test)]
mod contract {
    use super::*;

    /// Every variant once, through the core and back.
    #[test]
    fn every_calculation_crosses_to_the_core_and_back() {
        let column = |id: &str| id.to_string();
        let calculations = vec![
            CalculationDto::Combine { method: CombineMethodDto::AbsoluteDifference, columns: vec![column("a"), column("b")] },
            CalculationDto::Shear { columns: vec![column("a"), column("b")], roughness: true },
            CalculationDto::SpeedSdShear { columns: vec![column("a"), column("b")] },
            CalculationDto::Veer { columns: vec![column("a"), column("b")] },
            CalculationDto::TemperatureGradient { columns: vec![column("a"), column("b")] },
            CalculationDto::DensityAdjusted { speed: column("a"), target: 1.225 },
            CalculationDto::EnergyWeightedDensity { speed: column("a") },
            CalculationDto::MovingAverage { column: column("a"), window: 19, centered: true },
            CalculationDto::FirstDifference { column: column("a") },
            CalculationDto::Accumulation { column: column("a"), period: AccumulationPeriodDto::Weekly { from: 1 }, factor: 1.0 / 6.0 },
            CalculationDto::Polynomial { column: column("a"), coefficients: vec![1.0, 2.0] },
            CalculationDto::PiecewiseLinear { column: column("a"), points: vec![(0.0, 1.0), (2.0, 3.0)] },
            CalculationDto::DateTime { variable: DateTimeVariableDto::WeekOfYear { from: 0 } },
            CalculationDto::DewPoint { temperature: column("t"), humidity: column("rh") },
            CalculationDto::InflowAngle { horizontal: column("u"), vertical: column("w") },
            CalculationDto::Expression { text: "X = A + B;".into(), columns: vec![column("a"), column("b")] },
            CalculationDto::Rews { columns: vec![column("a"), column("b")], hub: 100.0, diameter: 120.0, cube: true },
            CalculationDto::ReconstructAndAverage { columns: vec![column("a"), column("b")], direction: column("d") },
            CalculationDto::ReconstructAndAverageSdMax { columns: vec![column("a"), column("b")] },
            CalculationDto::CombineByBoom { columns: vec![column("a"), column("b")], direction: column("d"), booms: (0.0, 180.0), offset: 45.0 },
            CalculationDto::Solar { variable: SolarVariableDto::ClearSky },
            CalculationDto::CombineType { method: CombineMethodDto::Sum, kind: ColumnKind::TurbineOutput, subtype: ColumnSubtype::Mean },
        ];
        for calculation in calculations {
            let dto = CalculatedColumnDto { id: "c".into(), calculation, kind: Some(ColumnKind::Other), units: Some("x".into()), height_m: Some(30.0) };
            let core = CalculatedColumn::from(dto.clone());
            let back: CalculatedColumnDto = serde_json::from_value(serde_json::to_value(core).unwrap()).unwrap();
            assert_eq!(back, dto);
        }
    }
}

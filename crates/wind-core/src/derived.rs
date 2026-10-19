//! Auto-generated derived columns (ARQUITETURA §2.1): air density,
//! turbulence intensity (TI) and wind power density (WPD), the columns the
//! reference product creates by itself from the measured ones
//! [manual: def-auto-generated-CC.htm].
//!
//! Rules for the whole module:
//! - **Computed at import, from the values.** [`append`] runs after every
//!   import and re-import and stores plain columns, so the time series,
//!   summaries, histogram and exports read them like any other column (P1).
//!   Values never depend on flags; only an edit of a source's values
//!   ([`crate::edits`]) computes a derived column again ([`recompute`]).
//! - **Flags are inherited, never set.** A derived value is excluded wherever
//!   any measured column it is computed from is excluded, directly or through
//!   another derived column (WPD through the air density). The
//!   [`FilterView`](crate::filter::FilterView) applies this, and
//!   [`affected`](crate::filter::affected) tells which summaries an edit
//!   touches. A derived column takes no flag of its own
//!   ([`CoreError::DerivedColumnNotFlaggable`](crate::error::CoreError::DerivedColumnNotFlaggable)).
//! - **Missing in, missing out:** a step without a needed input is NaN.
//! - **Order:** derived columns come after the file's columns, each one after
//!   its sources, so one pass in index order resolves every dependency.
//! - **Extrapolated columns** are the one kind the user asks for
//!   ([`Dataset::extrapolations`], from the import configuration;
//!   [`crate::extrapolation`]): the speed by the power law fitted in each
//!   step (from [`SHEAR_MIN_SPEED_M_S`]), else the median exponent of the
//!   step's month and hour (the reference product's lookup table; the
//!   median, so a few bad stretches not yet flagged do not move it), with its
//!   SD, TI and power density, and the direction and temperature when asked.
//! - **TI** is σ/U as the original computes it, 0 at or below 0.01 m/s;
//!   a dataset can choose the IEC cut instead (undefined below 4 m/s).
//!   Unlike the others they accept a flag of their own, as the original's
//!   synthesized columns do.
//!
//! The air density counts the humidity (IEC 61400-12-1, as the original
//! computes it, checked in its code): the primary relative humidity column
//! where it has a value, else 50 %, the original's default. The humidity is
//! read but not inherited: where it is flagged the density stays, as in the
//! original, which falls back to the 50 % there.
//! ponytail: a flagged humidity value still counts here; the original uses 50 % in its place.
use crate::analysis::RHO_STD;
use crate::calculated::{self, Calculated};
use crate::extrapolation::{self, Synthesized};
use crate::model::{ColumnType, DataColumn, Dataset, Subtype};
use crate::displacement;
use crate::primary;
use crate::time::{self, hour_of, HOURS_PER_DAY, MONTHS_PER_YEAR};

/// Specific gas constant of dry air, J/(kg·K) (ISO 2533:1975, table 1).
pub const DRY_AIR_GAS_CONSTANT_J_KG_K: f64 = 287.052_87;
/// Sea-level temperature of the ISO 2533 standard atmosphere, K.
pub const STANDARD_SEA_LEVEL_TEMPERATURE_K: f64 = 288.15;
/// Sea-level pressure of the ISO 2533 standard atmosphere, Pa.
pub const STANDARD_SEA_LEVEL_PRESSURE_PA: f64 = 101_325.0;
/// Temperature lapse rate of the ISO 2533 troposphere, K/m.
pub const STANDARD_LAPSE_RATE_K_M: f64 = 0.0065;
/// Standard acceleration of gravity, m/s² (ISO 2533).
pub const STANDARD_GRAVITY_M_S2: f64 = 9.806_65;
/// 0 °C in kelvin.
pub const CELSIUS_ZERO_K: f64 = 273.15;
/// Specific gas constant of water vapour, J/(kg·K) (IEC 61400-12-1).
pub const WATER_VAPOUR_GAS_CONSTANT_J_KG_K: f64 = 461.5;
/// Vapour pressure P_w = a·exp(b·T) of IEC 61400-12-1: a in Pa, b in 1/K.
const VAPOUR_PRESSURE_PA: f64 = 2.05e-5;
const VAPOUR_PRESSURE_PER_K: f64 = 0.063_184_6;
/// Relative humidity assumed without a measured one, %: the original's default.
pub const DEFAULT_RELATIVE_HUMIDITY_PERCENT: f64 = 50.0;
/// Percent in a share.
const PERCENT: f64 = 100.0;
/// The original's TI rule, read in its code: σ/U above this mean speed, m/s,
/// and 0 at or below it (where σ/U would blow up).
pub const TI_ORIGINAL_MIN_SPEED_M_S: f64 = 0.01;
/// The cut a dataset can choose instead ([`Dataset::ti_cutoff_m_s`]): below
/// it TI is undefined (NaN). 4 m/s is the bottom of the range over which IEC
/// 61400-12-1 (annex F) calibrates cup anemometers, so slower means and their
/// deviations are outside any calibrated range.
pub const TI_IEC_MIN_SPEED_M_S: f64 = 4.0;

/// Id of the air density column, unless a file column already has it.
pub const AIR_DENSITY_ID: &str = "Densidade do ar";
/// Appended to a speed column's id to name its TI column ("WS80 TI").
pub const TURBULENCE_INTENSITY_SUFFIX: &str = " TI";
/// Appended to a speed column's id to name its WPD column ("WS80 WPD").
pub const POWER_DENSITY_SUFFIX: &str = " WPD";
/// First suffix given to a derived id that a file column already uses.
const FIRST_DUPLICATE_SUFFIX: usize = 2;
/// Steps per call of a per-step kernel: the chunk function runs hundreds of
/// times per column, so the WASM engine soon swaps it for optimized code
/// (ARQUITETURA P13), and natively each chunk still vectorizes.
const KERNEL_CHUNK_STEPS: usize = 4_096;

/// Both speeds must read at least this for a step's own shear exponent,
/// m/s: below it the ratio of two small numbers is noise (and cups leave
/// their calibrated range, IEC 61400-12-1 annex F).
pub const SHEAR_MIN_SPEED_M_S: f64 = 4.0;

/// Units the import writes for kelvin.
pub const KELVIN: &str = "K";
/// Units the import writes for degrees Celsius (the type's default).
pub const CELSIUS: &str = "°C";
/// Other spellings of degrees Celsius accepted from a user (any letter case).
const CELSIUS_SPELLINGS: [&str; 5] = [CELSIUS, "ºC", "C", "degC", "deg C"];
/// Units the import writes for pascals.
pub const PASCAL: &str = "Pa";
/// Units the import writes for hectopascals (the type's default).
pub const HECTOPASCAL: &str = "hPa";
/// Units the import writes for kilopascals.
pub const KILOPASCAL: &str = "kPa";
/// Pressure spellings (any letter case) and their size in pascals.
const PRESSURE_UNITS_PA: [(&str, f64); 5] = [(HECTOPASCAL, 100.0), ("mbar", 100.0), ("mb", 100.0), (KILOPASCAL, 1000.0), (PASCAL, 1.0)];

/// How a derived column is computed; indices point into `Dataset::columns`
/// and always come before the derived column itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Derived {
    /// ρ of moist air ([`air_density_of`]) from a temperature column and a
    /// pressure column, or the standard pressure at the site when there is
    /// no pressure column, and the relative humidity.
    AirDensity {
        /// Temperature column.
        temperature: usize,
        /// Pressure column; `None` = standard pressure at the site elevation.
        pressure: Option<usize>,
        /// Relative humidity column, read but not inherited (module docs);
        /// `None` = [`DEFAULT_RELATIVE_HUMIDITY_PERCENT`].
        humidity: Option<usize>,
    },
    /// TI = σ/U of a mean speed column and its standard deviation child.
    TurbulenceIntensity {
        /// Mean speed column.
        speed: usize,
        /// Its standard deviation child.
        deviation: usize,
    },
    /// A speed, speed SD, direction or temperature synthesized at another
    /// height ([`Dataset::extrapolations`], [`crate::extrapolation`]).
    Extrapolated(Box<Synthesized>),
    /// A column the user defines ([`crate::calculated`]): computed under
    /// default flag filtering, so it inherits no flag.
    Calculated(Box<Calculated>),
    /// WPD = ½·ρ·U³ of a mean speed column, with the air density column when
    /// the dataset has one and [`RHO_STD`] otherwise.
    PowerDensity {
        /// Mean speed column.
        speed: usize,
        /// Air density column; `None` = `RHO_STD`.
        density: Option<usize>,
    },
    /// The wind speed √(u² + v²), or the direction it blows from, of a pair
    /// of wind components (reanalysis files give the wind as eastward `u`
    /// and northward `v`: ERA5's u100/v100, MERRA-2's U50M/V50M).
    Components {
        /// Eastward component column.
        u: usize,
        /// Northward component column.
        v: usize,
        /// The direction rather than the speed.
        direction: bool,
    },
}

impl Derived {
    /// The columns this one is computed from (measured or derived).
    pub fn sources(&self) -> Vec<usize> {
        let (first, second) = match self {
            Derived::AirDensity { temperature, pressure, .. } => (*temperature, *pressure),
            Derived::TurbulenceIntensity { speed, deviation } => (*speed, Some(*deviation)),
            Derived::PowerDensity { speed, density } => (*speed, *density),
            Derived::Extrapolated(synthesized) => return synthesized.sources(),
            Derived::Calculated(calculated) => return calculated.sources(),
            Derived::Components { u, v, .. } => (*u, Some(*v)),
        };
        std::iter::once(first).chain(second).collect()
    }

    /// The columns whose values this one reads: [`Derived::sources`] and the
    /// air density's humidity.
    fn inputs(&self) -> Vec<usize> {
        let mut inputs = self.sources();
        if let Derived::AirDensity { humidity: Some(humidity), .. } = self {
            inputs.push(*humidity);
        }
        inputs
    }

    /// Whether the column takes a flag of its own: a synthesized one does,
    /// as in the original; the others only inherit their sources'.
    pub fn is_flaggable(&self) -> bool {
        matches!(self, Derived::Extrapolated(_))
    }

    /// Whether the column is excluded wherever a source is: all but the
    /// calculated ones, which read their sources already filtered.
    pub fn inherits_flags(&self) -> bool {
        !matches!(self, Derived::Calculated(_))
    }

    /// Whether the column stands for a measured quantity, so it can be a
    /// primary column: the speed and direction of wind components are the
    /// dataset's wind, only written as components.
    pub fn is_measurement(&self) -> bool {
        matches!(self, Derived::Components { .. })
    }
}

/// The measured columns of `dataset` a column derived as `derived` is
/// computed from, through any chain of derived columns, in index order
/// without repeats. Their flags are the ones it inherits.
pub fn measured_sources(dataset: &Dataset, derived: &Derived) -> Vec<usize> {
    let mut found = Vec::new();
    let mut pending: Vec<usize> = derived.sources();
    while let Some(source) = pending.pop() {
        match &dataset.columns[source].derived {
            Some(derived) => pending.extend(derived.sources()),
            None => found.push(source),
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The dataset's air density column, if it has one.
pub fn air_density_column(dataset: &Dataset) -> Option<usize> {
    dataset.columns.iter().position(|column| matches!(column.derived, Some(Derived::AirDensity { .. })))
}

/// Adds the derived columns to a freshly imported `dataset`:
/// - the air density, when there is a temperature column and either a
///   pressure column or a known site elevation (otherwise energy figures use
///   [`RHO_STD`]);
/// - per pair of wind components at one height, the speed and direction
///   they make ([`Derived::Components`]), the speed with its WPD;
/// - per mean wind speed column, its TI (when it has a standard deviation
///   child) and its WPD.
///
/// Cost: one pass over the inputs per derived column, and one new column of
/// `len` values each.
pub fn append(dataset: &mut Dataset) {
    let density = air_density(dataset).map(|column| push(dataset, column));
    let mut speeds: Vec<usize> = (0..dataset.columns.len()).filter(|&index| is_measured_mean(&dataset.columns[index], ColumnType::WindSpeed)).collect();
    for (u, v, height_m) in component_pairs(dataset) {
        let [speed, direction] = [false, true].map(|direction| component_column(dataset, u, v, height_m, direction));
        speeds.push(push(dataset, speed));
        push(dataset, direction);
    }
    for speed in speeds {
        if let Some(deviation) = deviation_of(dataset, speed) {
            let derived = Derived::TurbulenceIntensity { speed, deviation };
            let column = derived_column(dataset, speed, TURBULENCE_INTENSITY_SUFFIX, ColumnType::TurbulenceIntensity, values_of(dataset, &derived), derived);
            push(dataset, column);
        }
        let derived = Derived::PowerDensity { speed, density };
        let column = derived_column(dataset, speed, POWER_DENSITY_SUFFIX, ColumnType::PowerDensity, values_of(dataset, &derived), derived);
        push(dataset, column);
    }
    for spec in dataset.extrapolations.clone() {
        let Some(plan) = extrapolation::plan(dataset, &spec) else { continue };
        let column = extrapolation::column(dataset, plan.speed, None);
        let speed = push(dataset, column);
        let derived = Derived::PowerDensity { speed, density };
        let column = derived_column(dataset, speed, POWER_DENSITY_SUFFIX, ColumnType::PowerDensity, values_of(dataset, &derived), derived);
        push(dataset, column);
        if let Some(synthesized) = plan.deviation {
            let column = extrapolation::column(dataset, synthesized, Some(speed));
            let deviation = push(dataset, column);
            let derived = Derived::TurbulenceIntensity { speed, deviation };
            let column = derived_column(dataset, speed, TURBULENCE_INTENSITY_SUFFIX, ColumnType::TurbulenceIntensity, values_of(dataset, &derived), derived);
            push(dataset, column);
        }
        for synthesized in [plan.direction, plan.temperature].into_iter().flatten() {
            let column = extrapolation::column(dataset, synthesized, None);
            push(dataset, column);
        }
    }
    for spec in dataset.calculated.clone() {
        if let Some(column) = calculated::column(dataset, &spec) {
            push(dataset, column);
        }
    }
    calculated::refresh(dataset);
}

/// The shear exponent of step `step` between speed columns `a` and `b`,
/// ln(U_b/U_a) / ln(z_b/z_a) on effective heights (less `displacement_m`),
/// when both read at least [`SHEAR_MIN_SPEED_M_S`] and both heights stay
/// above the displacement; NaN otherwise.
pub(crate) fn step_alpha(dataset: &Dataset, a: usize, b: usize, step: usize, displacement_m: f64) -> f64 {
    let (speed_a, speed_b) = (dataset.columns[a].values[step], dataset.columns[b].values[step]);
    if speed_a >= SHEAR_MIN_SPEED_M_S && speed_b >= SHEAR_MIN_SPEED_M_S {
        (speed_b / speed_a).ln() / height_ratio(dataset, b, a, displacement_m).ln()
    } else {
        f64::NAN
    }
}

/// Effective height of column `to` over that of column `from`, both less
/// `displacement_m`; NaN without a height or when one is not above it.
pub(crate) fn height_ratio(dataset: &Dataset, to: usize, from: usize, displacement_m: f64) -> f64 {
    let effective = |column: usize| dataset.columns[column].height_m.map_or(f64::NAN, |height| height - displacement_m);
    effective_ratio(effective(to), effective(from))
}

/// `to / from` when both effective heights are positive, NaN otherwise.
fn effective_ratio(to: f64, from: f64) -> f64 {
    if to > 0.0 && from > 0.0 {
        to / from
    } else {
        f64::NAN
    }
}

/// The median shear exponent between two speed columns per calendar month
/// and hour of day (the reference product's lookup table), for the steps
/// without one of their own; a cell without data takes the record's median.
pub(crate) struct ShearTable {
    month_of: Vec<usize>,
    table: Vec<f64>,
    /// The displacement height of each step ([`crate::displacement`]).
    displaced: Option<Vec<f64>>,
}

impl ShearTable {
    /// The table of columns `a` and `b` over the whole record. O(steps).
    pub(crate) fn new(dataset: &Dataset, a: usize, b: usize) -> Self {
        let days = time::days(dataset);
        let mut month_of = vec![0usize; dataset.len];
        for month in time::months(dataset, &days) {
            month_of[month.steps()].fill(month.index_in_year());
        }
        let displaced = displacement::by_step(dataset);
        let mut cells: Vec<Vec<f64>> = vec![Vec::new(); MONTHS_PER_YEAR * HOURS_PER_DAY];
        for step in 0..dataset.len {
            let alpha = step_alpha(dataset, a, b, step, displaced.as_ref().map_or(0.0, |heights| heights[step]));
            if alpha.is_finite() {
                cells[month_of[step] * HOURS_PER_DAY + hour_of(dataset, step)].push(alpha);
            }
        }
        let overall = median(cells.iter().flatten().copied().collect());
        let table = cells.into_iter().map(|alphas| if alphas.is_empty() { overall } else { median(alphas) }).collect();
        ShearTable { month_of, table, displaced }
    }

    /// The displacement height of step `step`, m; 0 without one.
    pub(crate) fn displacement(&self, step: usize) -> f64 {
        self.displaced.as_ref().map_or(0.0, |heights| heights[step])
    }

    /// The exponent of step `step`'s month and hour.
    pub(crate) fn alpha(&self, dataset: &Dataset, step: usize) -> f64 {
        self.table[self.month_of[step] * HOURS_PER_DAY + hour_of(dataset, step)]
    }
}

/// The median of `values`, NaN when empty.
fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_unstable_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// Computes again every derived column whose sources include one of
/// `edited`, directly or through another derived column, after an edit
/// changed their values. One pass in index order, since sources come first;
/// columns computed from nothing edited keep their allocation.
pub fn recompute(dataset: &mut Dataset, edited: &[usize]) {
    let mut changed = vec![false; dataset.columns.len()];
    for &column in edited {
        changed[column] = true;
    }
    for index in 0..dataset.columns.len() {
        let Some(derived) = dataset.columns[index].derived.clone() else { continue };
        // Calculated columns follow the flags too: [`calculated::refresh`] redoes them after every op.
        if matches!(derived, Derived::Calculated(_)) {
            continue;
        }
        if derived.inputs().into_iter().any(|source| changed[source]) {
            dataset.columns[index].values = values_of(dataset, &derived).into();
            changed[index] = true;
        }
    }
}

/// The values of a column derived as `derived`, from the current values of
/// its sources.
fn values_of(dataset: &Dataset, derived: &Derived) -> Vec<f64> {
    match *derived {
        Derived::AirDensity { temperature, pressure, humidity } => air_density_values(dataset, temperature, pressure, humidity),
        Derived::TurbulenceIntensity { speed, deviation } => {
            let cutoff = dataset.ti_cutoff_m_s;
            map_pairs(&dataset.columns[speed].values, &dataset.columns[deviation].values, |speed, deviation| turbulence_intensity(speed, deviation, cutoff))
        }
        Derived::Extrapolated(ref synthesized) => extrapolation::values(dataset, synthesized),
        Derived::Calculated(_) => vec![f64::NAN; dataset.len],
        Derived::PowerDensity { speed, density } => {
            let speeds = &dataset.columns[speed].values;
            match density {
                Some(density) => map_pairs(speeds, &dataset.columns[density].values, power_density),
                None => map_values(speeds, |speed| power_density(speed, RHO_STD)),
            }
        }
        Derived::Components { u, v, direction: false } => map_pairs(&dataset.columns[u].values, &dataset.columns[v].values, f64::hypot),
        Derived::Components { u, v, direction: true } => map_pairs(&dataset.columns[u].values, &dataset.columns[v].values, direction_from),
    }
}

/// Suffix of the speed and direction columns made from wind components.
pub const COMPONENTS_SUFFIX: &str = " (u,v)";

/// The direction the wind blows from, degrees clockwise from north, of the
/// eastward and northward components; NaN for a calm (no direction).
pub fn direction_from(u: f64, v: f64) -> f64 {
    if u == 0.0 && v == 0.0 {
        return f64::NAN;
    }
    // The wind blows toward atan2(u, v); it comes from the opposite side.
    // Adding 0 turns a −0 (a northerly) into 0.
    libm::atan2(-u, -v).to_degrees().rem_euclid(DEGREES_PER_TURN) + 0.0
}

/// Degrees in a turn.
const DEGREES_PER_TURN: f64 = 360.0;

/// Which wind component a column is and at what height, from names like
/// `u100`, `v100` (ERA5), `U50M`, `V50M` (MERRA-2) or `u_10m`: the letter,
/// an optional `_`, a positive height, an optional `m`. Only columns read as
/// "other" (the importer knows no such type) qualify.
fn component_of(column: &DataColumn) -> Option<(char, f64)> {
    if column.kind != ColumnType::Other || column.derived.is_some() {
        return None;
    }
    let mut chars = column.id.chars();
    let letter = chars.next()?.to_ascii_lowercase();
    if letter != 'u' && letter != 'v' {
        return None;
    }
    let rest = chars.as_str();
    let rest = rest.strip_prefix('_').unwrap_or(rest);
    let number = rest.strip_suffix(['m', 'M']).unwrap_or(rest);
    if number.is_empty() || !number.chars().all(|char| char.is_ascii_digit() || char == '.') {
        return None;
    }
    number.parse::<f64>().ok().filter(|height| *height > 0.0).map(|height| (letter, height))
}

/// Every `(u, v, height)` pair of wind component columns at one height, in
/// the order of the `u` columns.
fn component_pairs(dataset: &Dataset) -> Vec<(usize, usize, f64)> {
    let components: Vec<(usize, char, f64)> =
        dataset.columns.iter().enumerate().filter_map(|(index, column)| component_of(column).map(|(letter, height)| (index, letter, height))).collect();
    components
        .iter()
        .filter(|(_, letter, _)| *letter == 'u')
        .filter_map(|&(u, _, height)| {
            let v = components.iter().find(|&&(_, letter, other)| letter == 'v' && other == height)?.0;
            Some((u, v, height))
        })
        .collect()
}

/// The speed (or, with `direction`, the direction) column of components `u` and `v`.
fn component_column(dataset: &Dataset, u: usize, v: usize, height_m: f64, direction: bool) -> DataColumn {
    let derived = Derived::Components { u, v, direction };
    let (kind, prefix) = if direction { (ColumnType::WindDirection, "Dir") } else { (ColumnType::WindSpeed, "Vel") };
    DataColumn {
        id: format!("{prefix} {height_m}m{COMPONENTS_SUFFIX}"),
        kind,
        subtype: Subtype::Mean,
        units: kind.default_units().to_string(),
        height_m: Some(height_m),
        parent: None,
        values: values_of(dataset, &derived).into(),
        derived: Some(derived),
    }
}

/// Pressure of the ISO 2533 standard atmosphere at `elevation_m` above sea
/// level, Pa: p₀·(1 − L·z/T₀)^(g/(R·L)) (troposphere, valid below 11 km;
/// also Manwell, McGowan & Rogers 2002, ch. 2). NaN where the formula breaks
/// down (far above the troposphere).
pub fn standard_pressure_pa(elevation_m: f64) -> f64 {
    let exponent = STANDARD_GRAVITY_M_S2 / (DRY_AIR_GAS_CONSTANT_J_KG_K * STANDARD_LAPSE_RATE_K_M);
    let temperature_ratio = 1.0 - STANDARD_LAPSE_RATE_K_M * elevation_m / STANDARD_SEA_LEVEL_TEMPERATURE_K;
    STANDARD_SEA_LEVEL_PRESSURE_PA * libm::pow(temperature_ratio, exponent)
}

/// Density of moist air, kg/m³ (IEC 61400-12-1):
/// ρ = (1/T)·(p/R_d − φ·P_w·(1/R_d − 1/R_w)), with the vapour pressure
/// P_w = 2.05·10⁻⁵·exp(0.0631846·T) Pa and φ the relative humidity as a
/// share, held within [0, 1] as the original holds it. NaN unless both the
/// pressure (Pa) and the absolute temperature (K) are positive.
pub fn air_density_of(pressure_pa: f64, temperature_k: f64, humidity_percent: f64) -> f64 {
    if !(pressure_pa > 0.0 && temperature_k > 0.0) {
        return f64::NAN;
    }
    let humidity = (humidity_percent / PERCENT).clamp(0.0, 1.0);
    let vapour_pa = VAPOUR_PRESSURE_PA * libm::exp(VAPOUR_PRESSURE_PER_K * temperature_k);
    let lighter = 1.0 / DRY_AIR_GAS_CONSTANT_J_KG_K - 1.0 / WATER_VAPOUR_GAS_CONSTANT_J_KG_K;
    (pressure_pa / DRY_AIR_GAS_CONSTANT_J_KG_K - humidity * vapour_pa * lighter) / temperature_k
}

/// Kelvin to add to a temperature in `units`; `None` for units this module
/// does not know, so such a column is never read on a wrong scale.
pub fn kelvin_offset(units: &str) -> Option<f64> {
    let units = units.trim();
    if units.eq_ignore_ascii_case(KELVIN) {
        Some(0.0)
    } else if CELSIUS_SPELLINGS.iter().any(|spelling| units.eq_ignore_ascii_case(spelling)) {
        Some(CELSIUS_ZERO_K)
    } else {
        None
    }
}

/// Pascals in one unit of pressure `units`; `None` for units this module
/// does not know.
pub fn pascals_per_unit(units: &str) -> Option<f64> {
    let units = units.trim();
    PRESSURE_UNITS_PA.iter().find(|(spelling, _)| units.eq_ignore_ascii_case(spelling)).map(|&(_, pascals)| pascals)
}

/// The air density column, or `None` without a usable temperature column, or
/// without both a pressure column and a site elevation.
///
/// Sensors: the primary temperature and pressure columns among those whose
/// units are known ([`primary`]; by default the highest, as the reference
/// product picks the highest temperature sensor [manual:
/// dlg-cc-air-density.htm]). The density is computed at the
/// temperature sensor, so a pressure read at another height is moved there by
/// the standard atmosphere's pressure difference between the two heights
/// [manual: articles-calculating-air-density.htm]. A pressure gap is filled
/// with the standard pressure at the sensor when the site elevation is known,
/// and left NaN otherwise. For the height adjustment alone, an unknown site
/// is taken at sea level: the difference barely depends on it. A column
/// without height counts as 0 m above ground.
fn air_density(dataset: &Dataset) -> Option<DataColumn> {
    let temperature = primary::primary_among(dataset, ColumnType::Temperature, |column| kelvin_offset(&column.units).is_some())?;
    let pressure = primary::primary_among(dataset, ColumnType::Pressure, |column| pascals_per_unit(&column.units).is_some());
    if pressure.is_none() && dataset.elevation_m.is_none() {
        return None;
    }
    let humidity = primary::primary(dataset, ColumnType::RelativeHumidity);
    let source = &dataset.columns[temperature];
    Some(DataColumn {
        id: AIR_DENSITY_ID.to_string(),
        kind: ColumnType::AirDensity,
        subtype: Subtype::Mean,
        units: ColumnType::AirDensity.default_units().to_string(),
        height_m: source.height_m,
        parent: None,
        values: air_density_values(dataset, temperature, pressure, humidity).into(),
        derived: Some(Derived::AirDensity { temperature, pressure, humidity }),
    })
}

/// The air density from `temperature` and `pressure` (the standard pressure
/// at the site when `None`) and `humidity` (50 % when `None` or missing), as
/// [`air_density`] describes. Both columns were chosen for units
/// [`kelvin_offset`] and [`pascals_per_unit`] know, and units change only
/// with a new import.
fn air_density_values(dataset: &Dataset, temperature: usize, pressure: Option<usize>, humidity: Option<usize>) -> Vec<f64> {
    const KNOWN_UNITS: &str = "the air density's sensors are chosen for units this module knows";
    let offset_k = kelvin_offset(&dataset.columns[temperature].units).expect(KNOWN_UNITS);
    let height_of = |column: usize| dataset.columns[column].height_m.unwrap_or(0.0);
    let site = dataset.elevation_m;
    let standard_at_sensor = site.map(|site| standard_pressure_pa(site + height_of(temperature)));
    let temperatures = &dataset.columns[temperature].values;
    let humidities = humidity.map(|column| &dataset.columns[column].values[..]);
    let humidity_at = |step: usize| match humidities.map(|values| values[step]) {
        Some(percent) if !percent.is_nan() => percent,
        _ => DEFAULT_RELATIVE_HUMIDITY_PERCENT,
    };
    // Without a pressure column the column exists only with a known elevation.
    let standard_pa = standard_at_sensor.unwrap_or(f64::NAN);
    let pressure_at: Box<dyn Fn(usize) -> f64 + '_> = match pressure {
        Some(pressure) => {
            let pascals = pascals_per_unit(&dataset.columns[pressure].units).expect(KNOWN_UNITS);
            let ground = site.unwrap_or(0.0);
            let adjustment_pa = standard_pressure_pa(ground + height_of(temperature)) - standard_pressure_pa(ground + height_of(pressure));
            let readings = &dataset.columns[pressure].values;
            Box::new(move |step| if readings[step].is_nan() { standard_pa } else { readings[step] * pascals + adjustment_pa })
        }
        None => Box::new(move |_| standard_pa),
    };
    (0..temperatures.len()).map(|step| air_density_of(pressure_at(step), temperatures[step] + offset_k, humidity_at(step))).collect()
}

/// Turbulence intensity σ/U of one step (IEC 61400-1, §3): as the original,
/// 0 at a speed of [`TI_ORIGINAL_MIN_SPEED_M_S`] or less; with `cutoff`,
/// NaN below it instead. NaN without the speed or the deviation.
fn turbulence_intensity(speed: f64, deviation: f64, cutoff: Option<f64>) -> f64 {
    if speed.is_nan() || deviation.is_nan() {
        return f64::NAN;
    }
    match cutoff {
        Some(cutoff) if speed < cutoff => f64::NAN,
        Some(_) => deviation / speed,
        None if speed <= TI_ORIGINAL_MIN_SPEED_M_S => 0.0,
        None => deviation / speed,
    }
}

/// Wind power density ½·ρ·U³ of one step, W/m²: the kinetic energy flux
/// through a unit area (Manwell, McGowan & Rogers 2002, ch. 2).
fn power_density(speed: f64, density: f64) -> f64 {
    0.5 * density * speed * speed * speed
}

/// `kernel` over aligned pairs of `left` and `right`, a chunk of
/// [`KERNEL_CHUNK_STEPS`] per call of [`extend_with_pairs`].
fn map_pairs<K: Fn(f64, f64) -> f64>(left: &[f64], right: &[f64], kernel: K) -> Vec<f64> {
    let mut values = Vec::with_capacity(left.len());
    for (left, right) in left.chunks(KERNEL_CHUNK_STEPS).zip(right.chunks(KERNEL_CHUNK_STEPS)) {
        extend_with_pairs(&mut values, left, right, &kernel);
    }
    values
}

/// `kernel` over `inputs`, a chunk of [`KERNEL_CHUNK_STEPS`] per call of
/// [`extend_with`].
fn map_values<K: Fn(f64) -> f64>(inputs: &[f64], kernel: K) -> Vec<f64> {
    let mut values = Vec::with_capacity(inputs.len());
    for chunk in inputs.chunks(KERNEL_CHUNK_STEPS) {
        extend_with(&mut values, chunk, &kernel);
    }
    values
}

/// One chunk of [`map_pairs`]; never inlined, so it is its own WASM function.
#[inline(never)]
fn extend_with_pairs<K: Fn(f64, f64) -> f64>(values: &mut Vec<f64>, left: &[f64], right: &[f64], kernel: &K) {
    values.extend(left.iter().zip(right).map(|(&first, &second)| kernel(first, second)));
}

/// One chunk of [`map_values`]; never inlined, so it is its own WASM function.
#[inline(never)]
fn extend_with<K: Fn(f64) -> f64>(values: &mut Vec<f64>, inputs: &[f64], kernel: &K) {
    values.extend(inputs.iter().map(|&input| kernel(input)));
}

/// Whether `column` is a measured mean column of `kind`.
fn is_measured_mean(column: &DataColumn, kind: ColumnType) -> bool {
    column.kind == kind && column.subtype == Subtype::Mean && column.derived.is_none()
}

/// The standard deviation child of speed column `speed`, if any.
fn deviation_of(dataset: &Dataset, speed: usize) -> Option<usize> {
    dataset.columns.iter().position(|column| {
        column.parent == Some(speed) && column.subtype == Subtype::StdDev && column.kind == ColumnType::WindSpeed && column.derived.is_none()
    })
}

/// A derived column named after `source` (its id plus `suffix`), at its height.
fn derived_column(dataset: &Dataset, source: usize, suffix: &str, kind: ColumnType, values: Vec<f64>, derived: Derived) -> DataColumn {
    let source = &dataset.columns[source];
    DataColumn {
        id: format!("{}{suffix}", source.id),
        kind,
        subtype: Subtype::Mean,
        units: kind.default_units().to_string(),
        height_m: source.height_m,
        parent: None,
        values: values.into(),
        derived: Some(derived),
    }
}

/// Appends `column` with an id no other column has, and returns its index.
fn push(dataset: &mut Dataset, mut column: DataColumn) -> usize {
    let taken = |id: &str| dataset.columns.iter().any(|existing| existing.id == id);
    if taken(&column.id) {
        column.id = (FIRST_DUPLICATE_SUFFIX..)
            .map(|suffix| format!("{}_{suffix}", column.id))
            .find(|candidate| !taken(candidate))
            .expect("an unbounded range always has an unused name");
    }
    dataset.columns.push(column);
    dataset.columns.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard-atmosphere pressures published in ISO 2533 (and the US
    /// Standard Atmosphere 1976, identical below 11 km), Pa, by elevation, m.
    const PUBLISHED_STANDARD_PRESSURES: [(f64, f64); 3] = [(0.0, 101_325.0), (1_000.0, 89_874.6), (3_000.0, 70_108.5)];
    /// Relative tolerance against the published tables (five digits).
    const TABLE_TOLERANCE: f64 = 1e-5;

    #[test]
    fn standard_pressure_matches_the_published_table() {
        for (elevation_m, want_pa) in PUBLISHED_STANDARD_PRESSURES {
            let pressure_pa = standard_pressure_pa(elevation_m);
            assert!((pressure_pa - want_pa).abs() <= TABLE_TOLERANCE * want_pa, "{elevation_m} m: {pressure_pa} Pa");
        }
    }

    #[test]
    fn sea_level_standard_density_is_rho_std() {
        let density = air_density_of(STANDARD_SEA_LEVEL_PRESSURE_PA, STANDARD_SEA_LEVEL_TEMPERATURE_K, 0.0);
        assert!((density - RHO_STD).abs() < TABLE_TOLERANCE, "{density}");
        assert!(air_density_of(f64::NAN, STANDARD_SEA_LEVEL_TEMPERATURE_K, 0.0).is_nan());
        assert!(air_density_of(STANDARD_SEA_LEVEL_PRESSURE_PA, -CELSIUS_ZERO_K, 0.0).is_nan(), "below absolute zero");
    }

    #[test]
    fn turbulence_intensity_as_the_original_or_with_the_iec_cut() {
        assert_eq!(turbulence_intensity(3.0, 0.75, None), 0.25, "any speed above 0.01 m/s");
        assert_eq!(turbulence_intensity(0.01, 0.6, None), 0.0, "0 at or below it");
        assert!(turbulence_intensity(3.0, 0.6, Some(TI_IEC_MIN_SPEED_M_S)).is_nan());
        assert_eq!(turbulence_intensity(5.0, 0.5, Some(TI_IEC_MIN_SPEED_M_S)), 0.1);
        assert!(turbulence_intensity(5.0, f64::NAN, None).is_nan());
    }

    #[test]
    fn units_are_read_case_insensitively_and_unknown_ones_refused() {
        assert_eq!(kelvin_offset(" °C "), Some(CELSIUS_ZERO_K));
        assert_eq!(kelvin_offset("k"), Some(0.0));
        assert_eq!(kelvin_offset("°F"), None);
        assert_eq!(pascals_per_unit("KPA"), Some(1000.0));
        assert_eq!(pascals_per_unit("mbar"), Some(100.0));
        assert_eq!(pascals_per_unit("inHg"), None);
    }
}

//! Turbulence (ESPEC-INTERFACE §1, Analisar › Turbulência): the turbulence
//! intensity (TI) of one or more speed sensors by speed bin, direction
//! sector, calendar month, hour of day or sensor height, with the IEC
//! 61400-1 figures [manual: dlg-analyze-turbulence.htm,
//! articles-IEC-turbulence.htm].
//!
//! - **TI per step** is the sensor's derived TI column ([`crate::derived`]):
//!   σ/U as the original (0 at or below 0.01 m/s), or undefined below the
//!   dataset's cut when it chose one, and excluded wherever the speed or its
//!   SD is.
//! - **Per bin:** sample count, mean, sample standard deviation and peak TI,
//!   and the IEC figure, which assumes the bin's TI is normally distributed:
//!   the representative TI (the 90 % quantile, mean + 1.28·SD; editions 3
//!   and 4) or the characteristic TI (mean + SD; edition 2).
//! - **Category:** the sensor's IEC figure in the 15 m/s bin
//!   ([`BIN_15_M_S`]) against each category's normal turbulence model (NTM)
//!   at 15 m/s: the lowest category whose curve still covers the site; above
//!   all of them, "S" (special design).
use super::{check_sectors, sector_of, MAX_HISTOGRAM_BINS};
use crate::acc::DEGREES_PER_TURN;
use crate::derived::Derived;
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use crate::time::{hour_of, Month, HOURS_PER_DAY, MONTHS_PER_YEAR};
use std::ops::Range;

/// The speed bin of the IEC category, m/s: 15 m/s ± half a metre per second.
pub const BIN_15_M_S: Range<f64> = 14.5..15.5;
/// Speed of the IEC reference turbulence, m/s.
pub const IEC_REFERENCE_SPEED_M_S: f64 = 15.0;
/// Standard normal quantile of 90 %: representative TI = mean + this · SD.
pub const NORMAL_QUANTILE_90: f64 = 1.28;
/// The NTM's offset b of editions 3 and 4, m/s: σ₁ = I_ref·(0.75·U + b).
pub const NTM_OFFSET_M_S: f64 = 5.6;
/// The NTM's slope of editions 3 and 4.
pub const NTM_SLOPE: f64 = 0.75;
/// Category of a site above every category of its edition.
pub const SPECIAL_CATEGORY: &str = "S";
/// Default speed bin width, m/s: bins centred on whole metres per second.
pub const DEFAULT_SPEED_BIN_M_S: f64 = 1.0;

/// Edition of IEC 61400-1 the figures follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IecEdition {
    /// 1999: characteristic TI, categories A and B.
    Second,
    /// 2005: representative TI, categories A, B and C.
    Third,
    /// 2019: representative TI, categories A+, A, B and C.
    #[default]
    Fourth,
}

/// One turbulence category of an edition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IecCategory {
    /// "A+", "A", "B", "C".
    pub name: &'static str,
    /// I_ref (editions 3 and 4) or I₁₅ (edition 2).
    pub reference_ti: f64,
    /// The edition-2 slope parameter a; unused by later editions.
    pub slope_a: f64,
}

/// Categories by edition, lowest turbulence first (IEC 61400-1, table 1 of
/// each edition).
const SECOND_EDITION: [IecCategory; 2] = [
    IecCategory { name: "B", reference_ti: 0.16, slope_a: 3.0 },
    IecCategory { name: "A", reference_ti: 0.18, slope_a: 2.0 },
];
const THIRD_EDITION: [IecCategory; 3] = [
    IecCategory { name: "C", reference_ti: 0.12, slope_a: 0.0 },
    IecCategory { name: "B", reference_ti: 0.14, slope_a: 0.0 },
    IecCategory { name: "A", reference_ti: 0.16, slope_a: 0.0 },
];
const FOURTH_EDITION: [IecCategory; 4] = [
    IecCategory { name: "C", reference_ti: 0.12, slope_a: 0.0 },
    IecCategory { name: "B", reference_ti: 0.14, slope_a: 0.0 },
    IecCategory { name: "A", reference_ti: 0.16, slope_a: 0.0 },
    IecCategory { name: "A+", reference_ti: 0.18, slope_a: 0.0 },
];

impl IecEdition {
    /// The edition's categories, lowest turbulence first.
    pub fn categories(self) -> &'static [IecCategory] {
        match self {
            IecEdition::Second => &SECOND_EDITION,
            IecEdition::Third => &THIRD_EDITION,
            IecEdition::Fourth => &FOURTH_EDITION,
        }
    }

    /// The bin's IEC figure from its mean and SD: characteristic (edition 2)
    /// or representative TI.
    pub fn figure(self, mean: f64, deviation: f64) -> f64 {
        match self {
            IecEdition::Second => mean + deviation,
            IecEdition::Third | IecEdition::Fourth => mean + NORMAL_QUANTILE_90 * deviation,
        }
    }

    /// The category's NTM as TI at hub speed `speed_m_s`: edition 2,
    /// I₁₅·(15/U + a)/(a + 1); editions 3 and 4, I_ref·(0.75 + b/U).
    pub fn ntm(self, category: &IecCategory, speed_m_s: f64) -> f64 {
        match self {
            IecEdition::Second => {
                category.reference_ti * (IEC_REFERENCE_SPEED_M_S / speed_m_s + category.slope_a) / (category.slope_a + 1.0)
            }
            IecEdition::Third | IecEdition::Fourth => category.reference_ti * (NTM_SLOPE + NTM_OFFSET_M_S / speed_m_s),
        }
    }

    /// The lowest category whose NTM at 15 m/s covers `figure_at_15`, the
    /// site's IEC figure in the 15 m/s bin; [`SPECIAL_CATEGORY`] above all.
    pub fn category(self, figure_at_15: f64) -> &'static str {
        self.categories()
            .iter()
            .find(|category| figure_at_15 <= self.ntm(category, IEC_REFERENCE_SPEED_M_S))
            .map_or(SPECIAL_CATEGORY, |category| category.name)
    }
}

/// What the TI is binned by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurbulenceVersus {
    /// Speed bins of `bin_width` m/s, centred on its multiples.
    Speed {
        /// m/s.
        bin_width: f64,
    },
    /// Direction sectors of a direction column, sector 0 centred on north.
    Direction {
        /// The direction column.
        column: usize,
        /// Sectors.
        sectors: usize,
    },
    /// Calendar month over every year.
    Month,
    /// Hour of day.
    Hour,
    /// One bin per sensor, at its height.
    Height,
}

/// TI statistics of one bin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TiStats {
    /// Steps in the bin.
    pub count: usize,
    /// Mean TI; NaN for an empty bin.
    pub mean: f64,
    /// Sample standard deviation of the TI; NaN below two steps.
    pub deviation: f64,
    /// Highest TI; NaN for an empty bin.
    pub peak: f64,
    /// The edition's representative or characteristic TI.
    pub figure: f64,
}

/// One bin of one sensor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurbulenceBin {
    /// The speed column.
    pub sensor: usize,
    /// Bin centre: speed (m/s), sector centre (°), month (1–12), hour
    /// (0–23) or the sensor's height (m, NaN without one).
    pub center: f64,
    /// Its statistics.
    pub stats: TiStats,
}

/// A sensor's 15 m/s bin and IEC category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SensorAt15 {
    /// The speed column.
    pub sensor: usize,
    /// The 15 m/s bin.
    pub stats: TiStats,
    /// The category; `None` with fewer than two steps in the bin.
    pub category: Option<&'static str>,
}

/// The turbulence of some sensors.
#[derive(Debug, Clone, PartialEq)]
pub struct Turbulence {
    /// Every sensor's bins, sensor by sensor, in bin order.
    pub bins: Vec<TurbulenceBin>,
    /// Every sensor's 15 m/s bin and category, in sensor order.
    pub at_15: Vec<SensorAt15>,
}

/// Running mean, variance (Welford) and peak of TI values.
#[derive(Debug, Clone, Copy)]
struct TiAcc {
    count: usize,
    mean: f64,
    squares: f64,
    peak: f64,
}

impl TiAcc {
    const EMPTY: TiAcc = TiAcc { count: 0, mean: 0.0, squares: 0.0, peak: f64::NAN };

    fn add(&mut self, ti: f64) {
        self.count += 1;
        let delta = ti - self.mean;
        self.mean += delta / self.count as f64;
        self.squares += delta * (ti - self.mean);
        self.peak = self.peak.max(ti);
    }

    fn stats(&self, edition: IecEdition) -> TiStats {
        let mean = if self.count > 0 { self.mean } else { f64::NAN };
        let deviation = if self.count > 1 { (self.squares / (self.count - 1) as f64).sqrt() } else { f64::NAN };
        TiStats { count: self.count, mean, deviation, peak: self.peak, figure: edition.figure(mean, deviation) }
    }
}

/// The TI column derived from speed column `speed`.
///
/// # Errors
/// [`CoreError::NoTurbulenceIntensity`] when it has none (no SD child).
pub fn ti_column(dataset: &Dataset, speed: usize) -> CoreResult<usize> {
    dataset
        .columns
        .iter()
        .position(|column| matches!(column.derived, Some(Derived::TurbulenceIntensity { speed: source, .. }) if source == speed))
        .ok_or_else(|| CoreError::NoTurbulenceIntensity { column: dataset.columns.get(speed).map_or_else(|| speed.to_string(), |column| column.id.clone()) })
}

/// Turbulence of `sensors` (speed columns, each with a TI column) by
/// `versus`, under `view`. `months` is the record's calendar
/// (`DayStats::months`). One pass per sensor.
///
/// # Errors
/// [`CoreError::NoTurbulenceIntensity`], [`CoreError::BinWidthNotPositive`],
/// [`CoreError::TooManyBins`] or [`CoreError::SectorsOutOfRange`].
pub fn turbulence(dataset: &Dataset, view: &FilterView, months: &[Month], sensors: &[usize], versus: TurbulenceVersus, edition: IecEdition) -> CoreResult<Turbulence> {
    let columns = sensors.iter().map(|&sensor| ti_column(dataset, sensor).map(|ti| (sensor, ti))).collect::<CoreResult<Vec<_>>>()?;
    let bin_count = match versus {
        TurbulenceVersus::Speed { bin_width } => speed_bins(dataset, &columns, bin_width)?,
        TurbulenceVersus::Direction { sectors, .. } => {
            check_sectors(sectors)?;
            sectors
        }
        TurbulenceVersus::Month => MONTHS_PER_YEAR,
        TurbulenceVersus::Hour => HOURS_PER_DAY,
        TurbulenceVersus::Height => 1,
    };
    let mut result = Turbulence { bins: Vec::new(), at_15: Vec::with_capacity(columns.len()) };
    for (sensor, ti) in columns {
        let (tis, speeds) = (&dataset.columns[ti].values, &dataset.columns[sensor].values);
        let mut bins = vec![TiAcc::EMPTY; bin_count];
        let mut at_15 = TiAcc::EMPTY;
        let mut binned_columns = vec![ti];
        if let TurbulenceVersus::Direction { column, .. } = versus {
            binned_columns.push(column);
        }
        let runs = view.valid_runs(&binned_columns, 0..dataset.len);
        let mut month_of = month_lookup(months);
        for step in runs.into_iter().flatten() {
            let value = tis[step];
            if value.is_nan() {
                continue;
            }
            if BIN_15_M_S.contains(&speeds[step]) {
                at_15.add(value);
            }
            let bin = match versus {
                TurbulenceVersus::Speed { bin_width } => Some((speeds[step] / bin_width).round() as usize),
                TurbulenceVersus::Direction { column, sectors } => {
                    let direction = dataset.columns[column].values[step];
                    (!direction.is_nan()).then(|| sector_of(direction, sectors))
                }
                TurbulenceVersus::Month => month_of(step),
                TurbulenceVersus::Hour => Some(hour_of(dataset, step)),
                TurbulenceVersus::Height => Some(0),
            };
            if let Some(bin) = bin.filter(|&bin| bin < bin_count) {
                bins[bin].add(value);
            }
        }
        let center = |bin: usize| match versus {
            TurbulenceVersus::Speed { bin_width } => bin as f64 * bin_width,
            TurbulenceVersus::Direction { sectors, .. } => bin as f64 * DEGREES_PER_TURN / sectors as f64,
            TurbulenceVersus::Month => (bin + 1) as f64,
            TurbulenceVersus::Hour => bin as f64,
            TurbulenceVersus::Height => dataset.columns[sensor].height_m.unwrap_or(f64::NAN),
        };
        // Speed bins: from the first to the last one with data.
        let kept = match versus {
            TurbulenceVersus::Speed { .. } => {
                let first = bins.iter().position(|bin| bin.count > 0).unwrap_or(0);
                let end = bins.iter().rposition(|bin| bin.count > 0).map_or(0, |last| last + 1);
                first..end
            }
            _ => 0..bin_count,
        };
        result.bins.extend(kept.map(|bin| TurbulenceBin { sensor, center: center(bin), stats: bins[bin].stats(edition) }));
        let stats = at_15.stats(edition);
        // The figure needs a deviation: two steps or more.
        let category = stats.figure.is_finite().then(|| edition.category(stats.figure));
        result.at_15.push(SensorAt15 { sensor, stats, category });
    }
    Ok(result)
}

/// Speed bins of `bin_width` reaching the fastest speed of `columns`.
fn speed_bins(dataset: &Dataset, columns: &[(usize, usize)], bin_width: f64) -> CoreResult<usize> {
    if !(bin_width > 0.0 && bin_width.is_finite()) {
        return Err(CoreError::BinWidthNotPositive);
    }
    let fastest = columns
        .iter()
        .flat_map(|&(sensor, _)| dataset.columns[sensor].values.iter().copied())
        .filter(|speed| speed.is_finite())
        .fold(0.0_f64, f64::max);
    let bins = (fastest / bin_width).round() + 1.0;
    if bins > MAX_HISTOGRAM_BINS as f64 {
        return Err(CoreError::TooManyBins { bins, low: 0.0, high: fastest, max: MAX_HISTOGRAM_BINS });
    }
    Ok(bins as usize)
}

/// Month of the year (0 = January) of a step, for steps visited in
/// increasing order: walks `months` alongside.
fn month_lookup(months: &[Month]) -> impl FnMut(usize) -> Option<usize> + '_ {
    let mut current = 0;
    move |step| {
        while current < months.len() && months[current].step_end <= step {
            current += 1;
        }
        months.get(current).filter(|month| month.steps().contains(&step)).map(Month::index_in_year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NTM at 15 m/s of the fourth edition's categories: I_ref · (0.75 + 5.6/15).
    const FOURTH_EDITION_AT_15: [(&str, f64); 4] = [("C", 0.134_8), ("B", 0.157_267), ("A", 0.179_733), ("A+", 0.2022)];

    #[test]
    fn categories_follow_the_ntm_at_15_m_s() {
        let edition = IecEdition::Fourth;
        for (category, (name, at_15)) in edition.categories().iter().zip(FOURTH_EDITION_AT_15) {
            assert_eq!(category.name, name);
            assert!((edition.ntm(category, IEC_REFERENCE_SPEED_M_S) - at_15).abs() < 1e-6, "{name}");
        }
        assert_eq!(edition.category(0.13), "C");
        assert_eq!(edition.category(0.16), "A");
        assert_eq!(edition.category(0.21), SPECIAL_CATEGORY);
        assert_eq!(IecEdition::Third.category(0.19), SPECIAL_CATEGORY, "no A+ before the fourth edition");
        // Edition 2: the curve passes through I₁₅ at 15 m/s.
        assert_eq!(IecEdition::Second.category(0.17), "A");
        assert_eq!(IecEdition::Second.category(0.15), "B");
    }
}

//! WAsP observed wind climate (`.tab`) export: the joint frequency of
//! direction sector and speed class at one height, in the layout specified in
//! docs/formatos/wasp-tab.md (sources cited there: the WAsP help facility,
//! the European Wind Atlas and the reference product's manual).
//!
//! The steps counted are the wind rose's: speed and direction both valid and
//! not excluded by a flag, each put in its sector by the rose's own
//! `sector_of`, so the two never disagree. Frequencies are apportioned in
//! integers by largest remainder, so line 4 sums to exactly 100.00 % and each
//! sector's column to exactly 1000.000 ‰.
use crate::analysis::{sector_of, MAX_SECTORS};
use crate::error::{CoreError, CoreResult};
use crate::filter::FilterView;
use crate::model::Dataset;
use std::fmt::Write;

/// Most speed classes a table may have (P9): 0.1 m/s classes up to 100 m/s.
pub const MAX_TAB_SPEED_BINS: usize = 1_000;
/// Line 4 is in percent: the sectors share 100 %.
const PERCENT_TOTAL: u64 = 100;
/// Decimals of line 4 (6.00 %, as in the reference manual's example).
const SECTOR_DECIMALS: u32 = 2;
/// Lines 5+ are per mille within each sector: each sector's classes share 1000 ‰.
const PER_MILLE_TOTAL: u64 = 1_000;
/// Decimals of lines 5+ (94.699 ‰, as in the reference manual's example).
const CLASS_DECIMALS: u32 = 3;
/// Line 3 after the sector count: speeds are already in m/s, so the factor
/// is one, and sector 1 is centred on north, so the offset is zero.
const SPEED_FACTOR: &str = "1.00";
const DIRECTION_OFFSET: &str = "0.00";
/// Class edges are whole millionths of a m/s: edge `c` is
/// `(first + c·width) / EDGE_RESOLUTION` in integer units, so `0.1 + 0.2`
/// is written and counted as 0.3, never 0.30000000000000004.
const EDGE_RESOLUTION: f64 = 1e6;
/// Valid latitudes and longitudes, ± degrees.
const MAX_LATITUDE_DEG: f64 = 90.0;
const MAX_LONGITUDE_DEG: f64 = 180.0;
/// Joins the parts of the default title: dataset, speed column, direction column.
const TITLE_SEPARATOR: &str = " · ";
/// Ends every line: WAsP runs on Windows.
const LINE_END: &str = "\r\n";
/// Width of each number column, so the table also reads well as text.
const COLUMN_WIDTH: usize = 9;

/// What to export, and the header fields that override the dataset's.
#[derive(Debug, Clone)]
pub struct TabOptions {
    /// Speed column index; its values must be in m/s.
    pub speed: usize,
    /// Direction column index, degrees clockwise from north.
    pub direction: usize,
    /// Direction sectors, `1..=MAX_SECTORS`.
    pub sectors: usize,
    /// Width of the speed classes, m/s.
    pub bin_width_m_s: f64,
    /// First class half as wide, so classes centre on multiples of the width.
    pub first_bin_half: bool,
    /// Line 1; `None` = dataset name, speed and direction column ids.
    pub title: Option<String>,
    /// Latitude, degrees north; `None` = the dataset's.
    pub latitude_deg: Option<f64>,
    /// Longitude, degrees east; `None` = the dataset's.
    pub longitude_deg: Option<f64>,
    /// Height above ground, m; `None` = the speed column's.
    pub height_m: Option<f64>,
}

/// Valid steps counted by speed class and direction sector.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedSectorCounts {
    /// Direction sectors; sector 0 is centred on north, counting clockwise.
    pub sectors: usize,
    /// Upper edge of each speed class, m/s; class 0 starts at 0 (and also
    /// holds negative speeds), class `c` covers `[upper_edges[c-1], upper_edges[c])`.
    pub upper_edges: Vec<f64>,
    /// `[class * sectors + sector]`.
    pub counts: Vec<u64>,
}

/// The `.tab` file text for `options`, after default flag filtering.
///
/// Cost: one pass over the valid steps of the two columns; allocation
/// proportional to classes × sectors, plus the text.
///
/// # Errors
/// [`CoreError::SiteLocationUnknown`], [`CoreError::SiteLocationOutOfRange`],
/// [`CoreError::ColumnWithoutHeight`], [`CoreError::HeightNotPositive`],
/// and those of [`speed_sector_counts`].
pub fn to_tab(dataset: &Dataset, view: &FilterView, options: &TabOptions) -> CoreResult<String> {
    let site = Site::resolve(dataset, options)?;
    let table = speed_sector_counts(dataset, view, options)?;
    Ok(write_tab(&table, &title(dataset, options), &site))
}

/// Counts the steps where both columns are valid by speed class and
/// direction sector: the table behind [`to_tab`].
///
/// # Errors
/// [`CoreError::SectorsOutOfRange`], [`CoreError::BinWidthNotPositive`]
/// (also for a width below a millionth of a m/s), [`CoreError::TooManyBins`]
/// past [`MAX_TAB_SPEED_BINS`] (usually an outlier) or
/// [`CoreError::NoDataAfterFilter`].
pub fn speed_sector_counts(dataset: &Dataset, view: &FilterView, options: &TabOptions) -> CoreResult<SpeedSectorCounts> {
    let sectors = options.sectors;
    if !(1..=MAX_SECTORS).contains(&sectors) {
        return Err(CoreError::SectorsOutOfRange { sectors, max: MAX_SECTORS });
    }
    let classes = SpeedClasses::new(options.bin_width_m_s, options.first_bin_half)?;
    let speeds = &dataset.columns[options.speed].values;
    let directions = &dataset.columns[options.direction].values;
    let mut counts: Vec<u64> = Vec::new();
    for run in view.valid_runs(&[options.direction, options.speed], 0..dataset.len) {
        for step in run {
            let (speed, direction) = (speeds[step], directions[step]);
            if speed.is_nan() || direction.is_nan() {
                continue;
            }
            let class = classes.class_of(speed)?;
            let cell = class * sectors + sector_of(direction, sectors);
            if cell >= counts.len() {
                counts.resize((class + 1) * sectors, 0);
            }
            counts[cell] += 1;
        }
    }
    if counts.is_empty() {
        return Err(CoreError::NoDataAfterFilter);
    }
    let upper_edges = (0..counts.len() / sectors).map(|class| classes.upper_edge(class)).collect();
    Ok(SpeedSectorCounts { sectors, upper_edges, counts })
}

/// The speed classes, with edges in whole [`EDGE_RESOLUTION`] units.
struct SpeedClasses {
    /// Class width in units.
    width_units: f64,
    /// Upper edge of class 0 in units.
    first_units: f64,
}

impl SpeedClasses {
    fn new(width_m_s: f64, first_bin_half: bool) -> CoreResult<Self> {
        let width_units = (width_m_s * EDGE_RESOLUTION).round();
        if !(width_units >= 1.0 && width_units.is_finite()) {
            return Err(CoreError::BinWidthNotPositive);
        }
        let first_units = if first_bin_half { (width_units / 2.0).round().max(1.0) } else { width_units };
        Ok(SpeedClasses { width_units, first_units })
    }

    /// Upper edge of `class`, m/s: exact in units, then one division.
    fn upper_edge(&self, class: usize) -> f64 {
        (self.first_units + class as f64 * self.width_units) / EDGE_RESOLUTION
    }

    /// The class of `speed` (not NaN): estimated by arithmetic, then settled
    /// against the written edges, so a speed equal to an edge (common with
    /// loggers' two decimals) lands where the file says it does.
    fn class_of(&self, speed: f64) -> CoreResult<usize> {
        let (width, first) = (self.width_units / EDGE_RESOLUTION, self.first_units / EDGE_RESOLUTION);
        let estimate = ((speed - first) / width).floor() + 1.0;
        let too_many = |classes: f64| CoreError::TooManyBins { bins: classes, low: 0.0, high: speed, max: MAX_TAB_SPEED_BINS };
        if !(estimate < MAX_TAB_SPEED_BINS as f64) {
            return Err(too_many(estimate + 1.0));
        }
        let mut class = estimate.max(0.0) as usize;
        while class + 1 < MAX_TAB_SPEED_BINS && speed >= self.upper_edge(class) {
            class += 1;
        }
        while class > 0 && speed < self.upper_edge(class - 1) {
            class -= 1;
        }
        if speed >= self.upper_edge(class) {
            return Err(too_many((class + 2) as f64));
        }
        Ok(class)
    }
}

/// Line 2 of the file, checked.
struct Site {
    latitude_deg: f64,
    longitude_deg: f64,
    height_m: f64,
}

impl Site {
    /// The request's values where given, the dataset's and the speed
    /// column's otherwise.
    fn resolve(dataset: &Dataset, options: &TabOptions) -> CoreResult<Self> {
        let latitude = options.latitude_deg.or(dataset.latitude);
        let longitude = options.longitude_deg.or(dataset.longitude);
        let (Some(latitude_deg), Some(longitude_deg)) = (latitude, longitude) else {
            return Err(CoreError::SiteLocationUnknown);
        };
        if !(latitude_deg.abs() <= MAX_LATITUDE_DEG && longitude_deg.abs() <= MAX_LONGITUDE_DEG) {
            return Err(CoreError::SiteLocationOutOfRange { latitude: latitude_deg, longitude: longitude_deg });
        }
        let speed = &dataset.columns[options.speed];
        let height_m = match options.height_m.or(speed.height_m) {
            Some(height_m) => height_m,
            None => return Err(CoreError::ColumnWithoutHeight { column: speed.id.clone() }),
        };
        if !(height_m > 0.0 && height_m.is_finite()) {
            return Err(CoreError::HeightNotPositive { height_m });
        }
        Ok(Site { latitude_deg, longitude_deg, height_m })
    }
}

/// Line 1: the requested title or the default one, on a single line.
fn title(dataset: &Dataset, options: &TabOptions) -> String {
    let title = options.title.clone().unwrap_or_else(|| {
        let parts = [&dataset.name, &dataset.columns[options.speed].id, &dataset.columns[options.direction].id];
        parts.map(String::as_str).join(TITLE_SEPARATOR)
    });
    title.chars().map(|character| if character.is_control() { ' ' } else { character }).collect()
}

/// The file text: header lines, sector frequencies, then one line per class.
fn write_tab(table: &SpeedSectorCounts, title: &str, site: &Site) -> String {
    let sectors = table.sectors;
    let class_count = table.upper_edges.len();
    let count = |class: usize, sector: usize| table.counts[class * sectors + sector];
    let by_sector: Vec<Vec<u64>> = (0..sectors).map(|sector| (0..class_count).map(|class| count(class, sector)).collect()).collect();
    let sector_totals: Vec<u64> = by_sector.iter().map(|classes| classes.iter().sum()).collect();
    let sector_shares = apportion(&sector_totals, PERCENT_TOTAL * 10u64.pow(SECTOR_DECIMALS));
    let class_shares: Vec<Vec<u64>> =
        by_sector.iter().map(|classes| apportion(classes, PER_MILLE_TOTAL * 10u64.pow(CLASS_DECIMALS))).collect();

    let mut text = String::with_capacity((class_count + 1) * (sectors + 1) * (COLUMN_WIDTH + 1) + title.len());
    // Writing to a String cannot fail.
    let _ = write!(text, "{title}{LINE_END}");
    let _ = write!(text, "{} {} {}{LINE_END}", site.latitude_deg, site.longitude_deg, site.height_m);
    let _ = write!(text, "{sectors} {SPEED_FACTOR} {DIRECTION_OFFSET}{LINE_END}");
    let _ = write!(text, "{:>COLUMN_WIDTH$}", "");
    for &share in &sector_shares {
        let _ = write!(text, " {:>COLUMN_WIDTH$}", fixed(share, SECTOR_DECIMALS));
    }
    text.push_str(LINE_END);
    for (class, edge) in table.upper_edges.iter().enumerate() {
        let _ = write!(text, "{edge:>COLUMN_WIDTH$}");
        for shares in &class_shares {
            let _ = write!(text, " {:>COLUMN_WIDTH$}", fixed(shares[class], CLASS_DECIMALS));
        }
        text.push_str(LINE_END);
    }
    text
}

/// `counts` as shares of `total_units` that sum to exactly `total_units`
/// (all zero when the counts are), by the largest remainder method
/// (Hamilton): every share is the exact quota rounded down, and the units
/// still missing go to the largest remainders, ties to the earlier entry.
/// Each share is within one unit of its exact value. Integer arithmetic only.
fn apportion(counts: &[u64], total_units: u64) -> Vec<u64> {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return vec![0; counts.len()];
    }
    // In u128: a count of 1 Hz steps times a million units can pass 2^64.
    let total = u128::from(total);
    let scaled: Vec<u128> = counts.iter().map(|&count| u128::from(count) * u128::from(total_units)).collect();
    // Each quotient is at most `total_units`, so it fits back in a u64.
    let mut shares: Vec<u64> = scaled.iter().map(|&quota| (quota / total) as u64).collect();
    let missing = total_units - shares.iter().sum::<u64>();
    let mut by_remainder: Vec<usize> = (0..counts.len()).collect();
    // A stable sort: equal remainders keep index order.
    by_remainder.sort_by_key(|&entry| std::cmp::Reverse(scaled[entry] % total));
    by_remainder.iter().take(missing as usize).for_each(|&entry| shares[entry] += 1);
    shares
}

/// `units` written as a decimal with `decimals` places: 600 with 2 → "6.00".
fn fixed(units: u64, decimals: u32) -> String {
    let scale = 10u64.pow(decimals);
    format!("{}.{:0width$}", units / scale, units % scale, width = decimals as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::wind_rose;
    use crate::edits::Edit;
    use crate::flags;
    use crate::model::{ColumnType, DataColumn, Subtype, TimeStampMeaning};

    /// Ten-minute steps over 30 days.
    const STEP_S: i64 = 600;
    const STEPS: usize = 4_320;
    /// Pseudo-random but fixed speeds (two decimals, up to 25 m/s) and
    /// directions (one decimal): multiplying by a prime spreads them.
    const SPEED_PRIME: usize = 7_919;
    const SPEED_HUNDREDTHS: usize = 2_500;
    const DIRECTION_PRIME: usize = 104_729;
    const DIRECTION_TENTHS: usize = 3_600;
    const HUNDREDTHS: f64 = 100.0;
    const TENTHS: f64 = 10.0;
    /// Every this many steps one value is missing.
    const GAP_EVERY: usize = 97;
    const WASP_SECTORS: usize = 12;
    const ONE_M_S: f64 = 1.0;
    const SITE_LATITUDE: f64 = -5.1234;
    const SITE_LONGITUDE: f64 = -36.4567;
    const SPEED_HEIGHT_M: f64 = 100.0;
    const INVALID_FLAG: usize = 0;
    /// Columns of the test dataset.
    const SPEED: usize = 0;
    const DIRECTION: usize = 1;

    fn dataset() -> Dataset {
        let speed = (0..STEPS)
            .map(|step| if step % GAP_EVERY == 0 { f64::NAN } else { (step * SPEED_PRIME % SPEED_HUNDREDTHS) as f64 / HUNDREDTHS })
            .collect();
        let direction = (0..STEPS).map(|step| (step * DIRECTION_PRIME % DIRECTION_TENTHS) as f64 / TENTHS).collect();
        let column = |id: &str, kind, height_m, values: Vec<f64>| DataColumn {
            id: id.into(),
            kind,
            subtype: Subtype::Mean,
            units: String::new(),
            height_m,
            parent: None,
            values: values.into(),
            derived: None,
        };
        Dataset {
            name: "torre".into(),
            start: 0,
            step_s: STEP_S,
            len: STEPS,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![
                column("ws", ColumnType::WindSpeed, Some(SPEED_HEIGHT_M), speed),
                column("dir", ColumnType::WindDirection, None, direction),
            ],
            flags: flags::default_flags(),
            segments: Vec::new(),
            evidence: Default::default(),
            edits: Vec::new(),
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            displacement: None,
        }
    }

    fn options() -> TabOptions {
        TabOptions {
            speed: SPEED,
            direction: DIRECTION,
            sectors: WASP_SECTORS,
            bin_width_m_s: ONE_M_S,
            first_bin_half: true,
            title: None,
            latitude_deg: Some(SITE_LATITUDE),
            longitude_deg: Some(SITE_LONGITUDE),
            height_m: None,
        }
    }

    /// The numbers of a written line, as integer units of their last decimal.
    fn units_of(line: &str) -> Vec<u64> {
        line.split_whitespace().map(|number| number.replace('.', "").parse().unwrap()).collect()
    }

    #[test]
    fn counts_match_the_wind_rose() {
        let mut dataset = dataset();
        flags::flag(&mut dataset, &[SPEED], INVALID_FLAG, 100..900).unwrap();
        let view = FilterView::new(&dataset);
        let table = speed_sector_counts(&dataset, &view, &options()).unwrap();
        let edges = &table.upper_edges[..table.upper_edges.len() - 1];
        let no_calms = 0.0;
        let rose = wind_rose(&dataset, &view, DIRECTION, Some(SPEED), WASP_SECTORS, edges, no_calms).unwrap();
        assert_eq!(rose.samples as u64, table.counts.iter().sum::<u64>());
        for (class, row) in rose.frequency_by_speed.iter().enumerate() {
            for (sector, share) in row.iter().enumerate() {
                let rose_count = (share * rose.samples as f64).round() as u64;
                assert_eq!(rose_count, table.counts[class * WASP_SECTORS + sector], "class {class}, sector {sector}");
            }
        }
    }

    #[test]
    fn frequencies_sum_exactly_as_the_format_requires() {
        let dataset = dataset();
        let text = to_tab(&dataset, &FilterView::new(&dataset), &options()).unwrap();
        let lines: Vec<&str> = text.split(LINE_END).filter(|line| !line.is_empty()).collect();
        assert_eq!(lines[0], "torre · ws · dir");
        assert_eq!(lines[1], format!("{SITE_LATITUDE} {SITE_LONGITUDE} {SPEED_HEIGHT_M}"));
        assert_eq!(lines[2], format!("{WASP_SECTORS} {SPEED_FACTOR} {DIRECTION_OFFSET}"));
        let sector_units = units_of(lines[3]);
        assert_eq!(sector_units.len(), WASP_SECTORS);
        assert_eq!(sector_units.iter().sum::<u64>(), PERCENT_TOTAL * 10u64.pow(SECTOR_DECIMALS));
        let rows: Vec<Vec<f64>> =
            lines[4..].iter().map(|line| line.split_whitespace().map(|number| number.parse().unwrap()).collect()).collect();
        assert_eq!(&rows[0][..1], [0.5], "the first class is half wide");
        assert_eq!(&rows[1][..1], [1.5]);
        for sector in 0..WASP_SECTORS {
            let column: u64 = lines[4..].iter().map(|line| units_of(line)[1 + sector]).sum();
            assert_eq!(column, PER_MILLE_TOTAL * 10u64.pow(CLASS_DECIMALS), "sector {sector}");
        }
    }

    #[test]
    fn flagged_steps_are_left_out() {
        let mut flagged = dataset();
        let mut missing = dataset();
        let stretch = 0..STEPS / 2;
        flags::flag(&mut flagged, &[DIRECTION], INVALID_FLAG, stretch.clone()).unwrap();
        Edit::Delete { columns: vec![DIRECTION], steps: stretch }.apply(&mut missing).unwrap();
        let counts = |dataset: &Dataset| speed_sector_counts(dataset, &FilterView::new(dataset), &options()).unwrap();
        assert_eq!(counts(&flagged), counts(&missing));
        assert!(counts(&flagged).counts.iter().sum::<u64>() < counts(&dataset()).counts.iter().sum::<u64>());
    }

    #[test]
    fn a_speed_on_an_edge_lands_where_the_file_says() {
        let tenth = SpeedClasses::new(0.1, false).unwrap();
        assert_eq!(tenth.upper_edge(2), 0.3, "0.1 + 2 × 0.1 is written as 0.3");
        assert_eq!(tenth.class_of(0.3).unwrap(), 3, "0.3 starts the class [0.3, 0.4)");
        assert_eq!(tenth.class_of(0.29999).unwrap(), 2);
        assert_eq!(tenth.class_of(-1.0).unwrap(), 0, "negative speeds count in the first class, as in the rose");
        assert!(matches!(tenth.class_of(1e9), Err(CoreError::TooManyBins { .. })));
        assert!(matches!(SpeedClasses::new(0.0, false), Err(CoreError::BinWidthNotPositive)));
        assert!(matches!(SpeedClasses::new(f64::NAN, false), Err(CoreError::BinWidthNotPositive)));
    }

    #[test]
    fn the_header_needs_a_known_valid_site() {
        let mut dataset = dataset();
        let view = FilterView::new(&dataset);
        let error = |options: &TabOptions, dataset: &Dataset| to_tab(dataset, &view, options).unwrap_err();
        let unknown = TabOptions { latitude_deg: None, ..options() };
        assert_eq!(error(&unknown, &dataset), CoreError::SiteLocationUnknown);
        let far = TabOptions { latitude_deg: Some(MAX_LATITUDE_DEG + 1.0), ..options() };
        assert!(matches!(error(&far, &dataset), CoreError::SiteLocationOutOfRange { .. }));
        let underground = TabOptions { height_m: Some(-SPEED_HEIGHT_M), ..options() };
        assert!(matches!(error(&underground, &dataset), CoreError::HeightNotPositive { .. }));
        dataset.columns[SPEED].height_m = None;
        assert!(matches!(error(&options(), &dataset), CoreError::ColumnWithoutHeight { .. }));

        dataset.latitude = Some(SITE_LATITUDE);
        let from_dataset = TabOptions { latitude_deg: None, height_m: Some(SPEED_HEIGHT_M), title: Some("a\r\nb".into()), ..options() };
        let text = to_tab(&dataset, &view, &from_dataset).unwrap();
        assert!(text.starts_with(&format!("a  b{LINE_END}{SITE_LATITUDE} ")), "one title line, location from the dataset");
    }

    #[test]
    fn apportioned_shares_sum_exactly() {
        assert_eq!(apportion(&[1, 1, 1], 10_000), [3_334, 3_333, 3_333]);
        assert_eq!(apportion(&[0, 0], 10_000), [0, 0]);
        assert_eq!(apportion(&[2, 1, 0], 1_000), [667, 333, 0]);
        assert_eq!(fixed(600, SECTOR_DECIMALS), "6.00");
        assert_eq!(fixed(94_699, CLASS_DECIMALS), "94.699");
    }
}

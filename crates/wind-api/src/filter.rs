//! "Filtrar por" on the wire (ESPEC-INTERFACE §3.2): the optional `filter`
//! of every analysis request. Columns by id and dates as instants, resolved
//! against the dataset like every other request.
use crate::ops::column_index;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::criteria::{Criteria, FlagSelection, SectorCriterion, ValueCriterion};
use wind_core::error::CoreResult;
use wind_core::model::Dataset;

/// Milliseconds in a second: instants on the wire are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;

/// The criteria of one analysis; every field left out is inactive. A step is
/// read only when it meets all the active ones.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilterDto {
    /// Which flagged data to keep; omitted = default flag filtering.
    #[serde(default)]
    #[ts(optional)]
    pub flags: Option<FlagSelectionDto>,
    /// Only steps that start inside this period.
    #[serde(default)]
    #[ts(optional)]
    pub date_range: Option<DateRangeDto>,
    /// Only these calendar months.
    #[serde(default)]
    #[ts(optional)]
    pub months: Option<MonthRangeDto>,
    /// Only these hours of the day.
    #[serde(default)]
    #[ts(optional)]
    pub hours: Option<HourRangeDto>,
    /// Only one direction sector.
    #[serde(default)]
    #[ts(optional)]
    pub sector: Option<SectorFilterDto>,
    /// Only steps where a column lies in a range.
    #[serde(default)]
    #[ts(optional)]
    pub value: Option<ValueFilterDto>,
}

/// Which flagged data to keep. Exclusion wins: a step under any flag not
/// kept is out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlagSelectionDto {
    /// One entry per flag of the dataset, in order: keep its data.
    pub keep: Vec<bool>,
    /// Keep steps no flag covers ("Dados sem flag").
    pub keep_unflagged: bool,
}

/// Steps that start in `[fromMs, toMs)`. For whole days, `fromMs` is the
/// first day's midnight and `toMs` the midnight after the last day.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DateRangeDto {
    /// Start, epoch ms; a step starting here is kept.
    pub from_ms: f64,
    /// End, epoch ms; a step starting here is left out.
    pub to_ms: f64,
}

/// Months `first..=last` (1 = January); `first > last` wraps past December.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonthRangeDto {
    /// First month, 1 = January.
    pub first: u32,
    /// Last month, included.
    pub last: u32,
}

/// Hours `[start, end)` of the day: `start` in 0–23, `end` in 1–24;
/// `start ≥ end` wraps past midnight (20–5).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HourRangeDto {
    /// First hour, 0–23.
    pub start: u32,
    /// Hour the range stops at, 1–24, left out.
    pub end: u32,
}

/// One direction sector; steps without a valid direction are out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SectorFilterDto {
    /// Direction column id.
    pub direction: String,
    /// Sectors of the full circle, 1..=360; sector 0 is centred on north.
    pub sectors: usize,
    /// The sector kept, `0..sectors`, counting clockwise.
    pub sector: usize,
}

/// A column in `[min, max)`; steps without a value are out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueFilterDto {
    /// Column id.
    pub column: String,
    /// Lower bound, kept; `null` = none.
    pub min: Option<f64>,
    /// Upper bound, left out; `null` = none.
    pub max: Option<f64>,
}

impl FilterDto {
    /// The core's criteria, with column ids and instants resolved in
    /// `dataset`. The values are checked when the view is built.
    ///
    /// # Errors
    /// [`wind_core::error::CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn into_core(self, dataset: &Dataset) -> CoreResult<Criteria> {
        let first_step_from = |ms: f64| dataset.first_step_from((ms / MS_PER_SECOND).ceil() as i64);
        let sector = match self.sector {
            Some(sector) => Some(SectorCriterion {
                direction: column_index(dataset, &sector.direction)?,
                sectors: sector.sectors,
                sector: sector.sector,
            }),
            None => None,
        };
        let value = match self.value {
            Some(value) => Some(ValueCriterion {
                column: column_index(dataset, &value.column)?,
                min: value.min.unwrap_or(f64::NEG_INFINITY),
                max: value.max.unwrap_or(f64::INFINITY),
            }),
            None => None,
        };
        Ok(Criteria {
            flags: self.flags.map(|flags| FlagSelection { keep: flags.keep, keep_unflagged: flags.keep_unflagged }),
            steps: self.date_range.map(|range| first_step_from(range.from_ms)..first_step_from(range.to_ms)),
            months: self.months.map(|months| (months.first, months.last)),
            hours: self.hours.map(|hours| (hours.start, hours.end)),
            sector,
            value,
        })
    }
}

#[cfg(test)]
mod contract {
    use super::*;
    use wind_core::error::CoreError;
    use wind_core::flags;
    use wind_core::model::{ColumnType, DataColumn, Subtype, TimeStampMeaning};

    /// Ten-minute steps from 2014-01-01T00:00Z.
    const START_S: i64 = 1_388_534_400;
    const STEP_S: i64 = 600;
    const STEPS: usize = 144;
    const MS_PER_STEP: f64 = STEP_S as f64 * MS_PER_SECOND;

    fn dataset() -> Dataset {
        let column = |id: &str| DataColumn {
            id: id.into(),
            kind: ColumnType::WindSpeed,
            subtype: Subtype::Mean,
            units: String::new(),
            height_m: None,
            parent: None,
            values: vec![1.0; STEPS].into(),
            derived: None,
        };
        Dataset {
            name: "torre".into(),
            start: START_S,
            step_s: STEP_S,
            len: STEPS,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns: vec![column("ws"), column("dir")],
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

    #[test]
    fn a_filter_resolves_ids_instants_and_open_bounds() {
        let start_ms = START_S as f64 * MS_PER_SECOND;
        let json = format!(
            r#"{{"flags": {{"keep": [true, false, true], "keepUnflagged": false}},
                "dateRange": {{"fromMs": {}, "toMs": {}}},
                "months": {{"first": 12, "last": 2}}, "hours": {{"start": 20, "end": 5}},
                "sector": {{"direction": "dir", "sectors": 12, "sector": 3}},
                "value": {{"column": "ws", "min": 4, "max": null}}}}"#,
            start_ms + 6.0 * MS_PER_STEP,
            start_ms + 12.0 * MS_PER_STEP
        );
        let filter: FilterDto = serde_json::from_str(&json).unwrap();
        let criteria = filter.into_core(&dataset()).unwrap();
        assert_eq!(criteria.flags, Some(FlagSelection { keep: vec![true, false, true], keep_unflagged: false }));
        assert_eq!(criteria.steps, Some(6..12), "steps that start in [from, to)");
        assert_eq!((criteria.months, criteria.hours), (Some((12, 2)), Some((20, 5))));
        assert_eq!(criteria.sector, Some(SectorCriterion { direction: 1, sectors: 12, sector: 3 }));
        assert_eq!(criteria.value, Some(ValueCriterion { column: 0, min: 4.0, max: f64::INFINITY }), "null is no bound");
    }

    #[test]
    fn filters_refuse_typos_and_unknown_columns() {
        let typo = r#"{"months": {"first": 1, "lsat": 2}}"#;
        assert!(serde_json::from_str::<FilterDto>(typo).is_err());
        let unknown: FilterDto = serde_json::from_str(r#"{"value": {"column": "ws80", "min": 1, "max": 2}}"#).unwrap();
        assert_eq!(unknown.into_core(&dataset()).unwrap_err(), CoreError::UnknownColumn { id: "ws80".into() });
        let empty: FilterDto = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.into_core(&dataset()).unwrap(), Criteria::default(), "an empty filter changes nothing");
    }
}

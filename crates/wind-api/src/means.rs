//! Means over periods on the wire: the time series' daily, monthly and
//! yearly means, and the monthly means table with the MoMM and the
//! interannual variability ([`wind_core::analysis::monthly_table`]).
use crate::analyses::Prepared;
use crate::filter::FilterDto;
use crate::ops::column_index;
use crate::{finite, finite_all, DatasetId};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use ts_rs::TS;
use wind_core::analysis;
use wind_core::error::CoreResult;
use wind_core::series::{DayStats, MeanPeriod};

/// Milliseconds in a second: instants on the wire are epoch milliseconds.
const MS_PER_SECOND: f64 = 1000.0;

/// The period of a [`PeriodMeansRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MeanPeriodDto {
    /// Calendar day.
    Day,
    /// Calendar month.
    Month,
    /// Calendar year.
    Year,
}

/// Means of one column per day, month or year, under the default flag
/// filtering (the time series' "Exibir" choices).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeriodMeansRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// The period.
    pub period: MeanPeriodDto,
}

/// The answer to a [`PeriodMeansRequest`]: one entry per period, in order.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PeriodMeansDto {
    /// Start of each period's first step in the record, epoch ms.
    pub from_ms: Vec<f64>,
    /// End of each period's last step in the record, epoch ms.
    pub to_ms: Vec<f64>,
    /// Mean of each period; `null` without data.
    pub means: Vec<Option<f64>>,
    /// Valid steps of each period.
    pub valid: Vec<usize>,
}

/// The means of the request's column per period, from the kept per-day
/// summaries: O(days).
///
/// # Errors
/// An unknown column.
pub fn period_means(prepared: Prepared, request: PeriodMeansRequest) -> CoreResult<PeriodMeansDto> {
    let column = column_index(prepared.data, &request.column)?;
    let period = match request.period {
        MeanPeriodDto::Day => MeanPeriod::Day,
        MeanPeriodDto::Month => MeanPeriod::Month,
        MeanPeriodDto::Year => MeanPeriod::Year,
    };
    let means = prepared.stats.period_means(column, period);
    let at = |step: usize| prepared.data.time_at(step) as f64 * MS_PER_SECOND;
    Ok(PeriodMeansDto {
        from_ms: means.iter().map(|mean| at(mean.steps.start)).collect(),
        to_ms: means.iter().map(|mean| at(mean.steps.end)).collect(),
        means: means.iter().map(|mean| finite(mean.mean)).collect(),
        valid: means.iter().map(|mean| mean.valid).collect(),
    })
}

/// The monthly means table of one column.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonthlyTableRequest {
    /// The dataset.
    pub dataset: DatasetId,
    /// Column id.
    pub column: String,
    /// "Filtrar por" criteria; omitted = default flag filtering only.
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<FilterDto>,
}

/// The answer to a [`MonthlyTableRequest`]; `null` where a figure is undefined.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyTableDto {
    /// Calendar years, ascending.
    pub years: Vec<i32>,
    /// `[year][month]` mean, January first.
    pub means: Vec<Vec<Option<f64>>>,
    /// `[year][month]` share of the whole calendar month with data.
    pub recovery: Vec<Vec<f64>>,
    /// Per calendar month: the mean of the years' means.
    pub mean_of_years: Vec<Option<f64>>,
    /// Mean of monthly means, weighted by days (all twelve months needed).
    pub momm: Option<f64>,
    /// Per year: its day-weighted annual mean (all twelve months needed).
    pub annual: Vec<Option<f64>>,
    /// Standard deviation over mean of the complete years' annual means.
    pub interannual_variability: Option<f64>,
}

/// The monthly means table of the request's column. With a filter the
/// per-day summaries are rebuilt under it (one pass over the record).
///
/// # Errors
/// An unknown column, or the filter's.
pub fn monthly_table(prepared: Prepared, request: MonthlyTableRequest) -> CoreResult<MonthlyTableDto> {
    let column = column_index(prepared.data, &request.column)?;
    let stats = match request.filter {
        None => Cow::Borrowed(prepared.stats),
        Some(filter) => Cow::Owned(DayStats::new(prepared.data, &*prepared.view(Some(filter))?)),
    };
    let table = analysis::monthly_table(prepared.data, &stats, column);
    Ok(MonthlyTableDto {
        years: table.years,
        means: table.means.iter().map(|row| finite_all(row)).collect(),
        recovery: table.recovery.iter().map(|row| row.to_vec()).collect(),
        mean_of_years: finite_all(&table.mean_of_years),
        momm: finite(table.momm),
        annual: finite_all(&table.annual),
        interannual_variability: finite(table.interannual_variability),
    })
}

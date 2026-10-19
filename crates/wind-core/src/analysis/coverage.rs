//! Data recovery by calendar month, read from the per-day summaries.
use crate::series::DayStats;
use crate::time::Month;
use serde::Serialize;
use super::share;

/// Data recovery per column, by calendar month and over the record.
#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    /// Calendar months of the record, in order.
    pub months: Vec<Month>,
    /// `[column][month]`: share of the month's steps with data after
    /// filtering. Partial first/last months count only steps in the record.
    pub recovery: Vec<Vec<f64>>,
    /// Per column, over the whole record.
    pub total: Vec<f64>,
}

/// Coverage read from the per-day summaries: O(columns × days), no pass over
/// the data.
pub fn coverage(stats: &DayStats) -> Coverage {
    let months = stats.months().to_vec();
    let all_days = 0..months.last().map_or(0, |month| month.day_end);
    let columns = 0..stats.columns();
    let monthly_recovery = |column: usize| -> Vec<f64> {
        months
            .iter()
            .map(|month| share(stats.valid(column, month.days()) as f64, month.steps().len() as f64))
            .collect()
    };
    Coverage {
        recovery: columns.clone().map(monthly_recovery).collect(),
        total: columns.map(|column| share(stats.valid(column, all_days.clone()) as f64, stats.len() as f64)).collect(),
        months,
    }
}

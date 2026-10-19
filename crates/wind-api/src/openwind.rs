//! Exportar dados › Openwind on the wire ([`wind_core::export::openwind`]).
use crate::ops::column_index;
use crate::DatasetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::{CoreError, CoreResult};
use crate::finite;
use wind_core::export::openwind::{self, OpenwindMeans, OpenwindOptions};
use wind_core::filter::FilterView;
use wind_core::model::{ColumnType, Dataset};
use wind_core::primary;

/// The Openwind met mast file of one height of a dataset. Each column field
/// takes a column id; omitted = the primary speed, the vane nearest it in
/// height and the primary temperature; `temperature: ""` = none.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportOpenwindRequest {
    /// Dataset to export.
    pub dataset: DatasetId,
    /// Mean speed column.
    #[serde(default)]
    #[ts(optional)]
    pub speed: Option<String>,
    /// Mean direction column.
    #[serde(default)]
    #[ts(optional)]
    pub direction: Option<String>,
    /// Temperature column; `""` = the standard atmosphere's.
    #[serde(default)]
    #[ts(optional)]
    pub temperature: Option<String>,
    /// First instant exported, epoch ms; omitted = the record's start.
    #[serde(default)]
    #[ts(optional)]
    pub from_ms: Option<f64>,
    /// Last instant exported, epoch ms; omitted = the record's end.
    #[serde(default)]
    #[ts(optional)]
    pub to_ms: Option<f64>,
    /// The source file's name, written in the file; omitted = the dataset's.
    #[serde(default)]
    #[ts(optional)]
    pub source_name: Option<String>,
    /// Scale every speed so their simple mean is this, m/s; omitted = as measured.
    #[serde(default)]
    #[ts(optional)]
    pub target_mean: Option<f64>,
}

/// The original's three means of the speeds an Openwind file would export.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenwindMeansDto {
    /// Steps exported.
    pub steps: usize,
    /// Their simple mean, m/s.
    pub simple: Option<f64>,
    /// The mean of their 1 m/s distribution (Openwind's way), m/s.
    pub binned: Option<f64>,
    /// The mean of the seasonally unbiased distribution, m/s.
    pub unbiased: Option<f64>,
}

impl From<OpenwindMeans> for OpenwindMeansDto {
    fn from(means: OpenwindMeans) -> Self {
        OpenwindMeansDto { steps: means.steps, simple: finite(means.simple), binned: finite(means.binned), unbiased: finite(means.unbiased) }
    }
}

/// The Openwind file and a name for it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenwindFileDto {
    /// `<dataset>_openwind.csv`.
    pub name: String,
    /// The file's text.
    pub text: String,
}

impl ExportOpenwindRequest {
    /// The core's options, with ids resolved and the automatic choices made.
    ///
    /// # Errors
    /// [`CoreError::UnknownColumn`] for a column `dataset` lacks;
    /// [`CoreError::NoSuchColumn`] when it has no speed or no vane to pick.
    pub fn options(&self, dataset: &Dataset) -> CoreResult<OpenwindOptions> {
        let request = self.clone();
        let pick = |id: Option<String>, kind: ColumnType| -> CoreResult<Option<usize>> {
            match id.as_deref() {
                None => Ok(primary::primary(dataset, kind)),
                Some("") => Ok(None),
                Some(id) => column_index(dataset, id).map(Some),
            }
        };
        let missing = CoreError::NoSuchColumn { column: dataset.columns.len() };
        let speed = pick(request.speed, ColumnType::WindSpeed)?.ok_or(missing.clone())?;
        let direction = match request.direction {
            Some(id) => column_index(dataset, &id)?,
            None => primary::primary_among(dataset, ColumnType::WindDirection, |_| true).ok_or(missing)?,
        };
        let temperature = pick(request.temperature, ColumnType::Temperature)?;
        let steps_from = |ms: Option<f64>, fallback: usize| ms.map_or(fallback, |ms| dataset.index_of((ms / 1000.0).floor() as i64));
        let steps = steps_from(request.from_ms, 0)..request.to_ms.map_or(dataset.len, |ms| dataset.first_step_from((ms / 1000.0).floor() as i64 + 1));
        Ok(OpenwindOptions { speed, direction, temperature, steps, source_name: request.source_name.unwrap_or_else(|| dataset.name.clone()), scale: 1.0 })
    }

    /// The options with the scale that brings the simple mean to the
    /// request's target, when it has one.
    ///
    /// # Errors
    /// As [`ExportOpenwindRequest::options`] and [`openwind::openwind_means`].
    pub fn scaled_options(&self, dataset: &Dataset, view: &FilterView) -> CoreResult<OpenwindOptions> {
        let mut options = self.options(dataset)?;
        if let Some(target) = self.target_mean.filter(|target| *target > 0.0) {
            let simple = openwind::openwind_means(dataset, view, &options)?.simple;
            options.scale = target / simple;
        }
        Ok(options)
    }
}

/// `<dataset name without extension>_openwind.csv`.
pub fn file_name(dataset: &Dataset) -> String {
    let base = dataset.name.rsplit_once('.').map_or(dataset.name.as_str(), |(base, _)| base);
    format!("{base}_openwind.csv")
}

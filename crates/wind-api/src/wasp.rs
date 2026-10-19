//! The WAsP observed wind climate (`.tab`) export request. The file layout
//! is specified in docs/formatos/wasp-tab.md.
use crate::ops::column_index;
use crate::DatasetId;
use serde::Deserialize;
use ts_rs::TS;
use wind_core::error::CoreResult;
use wind_core::export::wasp_tab::TabOptions;
use wind_core::model::Dataset;

/// A WAsP `.tab` of one speed and one direction column. Latitude, longitude
/// and height left out come from the dataset and the speed column.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportWaspTabRequest {
    /// Dataset to export.
    pub dataset: DatasetId,
    /// Speed column id; its values must be in m/s.
    pub speed: String,
    /// Direction column id.
    pub direction: String,
    /// Direction sectors, 1..=360; WAsP usually takes 12.
    pub sectors: usize,
    /// Width of the speed classes, m/s.
    pub bin_width_m_s: f64,
    /// First class half as wide, so classes centre on multiples of the
    /// width; omitted = false.
    #[serde(default)]
    #[ts(optional)]
    pub first_bin_half: Option<bool>,
    /// Line 1 of the file; omitted = dataset name and the two column ids.
    #[serde(default)]
    #[ts(optional)]
    pub title: Option<String>,
    /// Degrees north, within ±90; omitted = the dataset's.
    #[serde(default)]
    #[ts(optional)]
    pub latitude_deg: Option<f64>,
    /// Degrees east, within ±180; omitted = the dataset's.
    #[serde(default)]
    #[ts(optional)]
    pub longitude_deg: Option<f64>,
    /// Height of the speed sensor above ground, m; omitted = the column's.
    #[serde(default)]
    #[ts(optional)]
    pub height_m: Option<f64>,
}

impl ExportWaspTabRequest {
    /// The core's options, with column ids resolved in `dataset`.
    ///
    /// # Errors
    /// [`wind_core::error::CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn options(self, dataset: &Dataset) -> CoreResult<TabOptions> {
        Ok(TabOptions {
            speed: column_index(dataset, &self.speed)?,
            direction: column_index(dataset, &self.direction)?,
            sectors: self.sectors,
            bin_width_m_s: self.bin_width_m_s,
            first_bin_half: self.first_bin_half.unwrap_or(false),
            title: self.title,
            latitude_deg: self.latitude_deg,
            longitude_deg: self.longitude_deg,
            height_m: self.height_m,
        })
    }
}

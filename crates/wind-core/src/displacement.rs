//! Displacement height (Configurar dataset › Altura de deslocamento [manual:
//! def-displacement-height.htm]): over vegetation the wind reaches zero above
//! the ground, at the displacement height d, so the shear laws use effective
//! heights z − d. It is constant, or varies by calendar month or by the
//! sector of a direction sensor (with a backup where the direction is
//! missing). The shear analysis, the vertical extrapolation and the
//! reconstruction from another height read it through [`by_step`].
use crate::analysis::{check_sectors, sector_of};
use crate::error::{CoreError, CoreResult};
use crate::model::Dataset;
use crate::time::{self, MONTHS_PER_YEAR};
use serde::{Deserialize, Serialize};

/// How a dataset's displacement height is given, m.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Displacement {
    /// The same in every step.
    Constant(f64),
    /// One per calendar month, January first.
    Monthly(Vec<f64>),
    /// One per sector of the direction column `direction` (sector 0 centred
    /// on north), and `backup` where that direction is missing.
    BySector {
        /// Id of the direction column.
        direction: String,
        /// Height per sector, clockwise from north.
        heights: Vec<f64>,
        /// Height where the direction is missing.
        backup: f64,
    },
}

impl Displacement {
    /// Fails unless every height is finite and not negative, a monthly one
    /// has twelve and a sector one a valid number of sectors.
    ///
    /// # Errors
    /// [`CoreError::DisplacementHeight`], [`CoreError::SectorsOutOfRange`].
    pub fn check(&self) -> CoreResult<()> {
        let heights: Vec<f64> = match self {
            Displacement::Constant(height) => vec![*height],
            Displacement::Monthly(heights) if heights.len() == MONTHS_PER_YEAR => heights.clone(),
            Displacement::Monthly(_) => return Err(CoreError::DisplacementHeight { height_m: f64::NAN }),
            Displacement::BySector { heights, backup, .. } => {
                check_sectors(heights.len())?;
                heights.iter().copied().chain([*backup]).collect()
            }
        };
        match heights.into_iter().find(|height| !(height.is_finite() && *height >= 0.0)) {
            Some(height_m) => Err(CoreError::DisplacementHeight { height_m }),
            None => Ok(()),
        }
    }
}

/// The displacement height of every step, m; `None` when the dataset has
/// none (all heights as measured). A sector displacement whose direction
/// column is gone takes its backup everywhere. O(steps).
pub fn by_step(dataset: &Dataset) -> Option<Vec<f64>> {
    Some(match dataset.displacement.as_ref()? {
        Displacement::Constant(height) => vec![*height; dataset.len],
        Displacement::Monthly(heights) => {
            let mut by_step = vec![0.0; dataset.len];
            for month in time::months(dataset, &time::days(dataset)) {
                by_step[month.steps()].fill(heights[month.index_in_year()]);
            }
            by_step
        }
        Displacement::BySector { direction, heights, backup } => {
            let directions = dataset.columns.iter().find(|column| column.id == *direction).map(|column| &column.values);
            (0..dataset.len)
                .map(|step| match directions.map(|values| values[step]).filter(|value| !value.is_nan()) {
                    Some(degrees) => heights[sector_of(degrees, heights.len())],
                    None => *backup,
                })
                .collect()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_text, ImportConfig};

    #[test]
    fn each_step_takes_its_month_or_sector() {
        // Two steps on 31 January and two on 1 February; directions north,
        // east, missing, south.
        let csv = "t,Dir 50m\n2023-01-31 23:40,350\n2023-01-31 23:50,90\n2023-02-01 00:00,\n2023-02-01 00:10,180\n";
        let (mut dataset, _) = import_text("torre", csv, &ImportConfig::default()).unwrap();
        assert_eq!(by_step(&dataset), None);
        let mut monthly = vec![0.0; MONTHS_PER_YEAR];
        (monthly[0], monthly[1]) = (2.0, 3.0);
        dataset.displacement = Some(Displacement::Monthly(monthly));
        assert_eq!(by_step(&dataset).unwrap(), [2.0, 2.0, 3.0, 3.0]);
        dataset.displacement = Some(Displacement::BySector { direction: "Dir 50m".into(), heights: vec![1.0, 2.0, 3.0, 4.0], backup: 9.0 });
        assert_eq!(by_step(&dataset).unwrap(), [1.0, 2.0, 9.0, 3.0], "350° is north; the missing direction takes the backup");

        assert!(Displacement::Constant(4.5).check().is_ok());
        assert_eq!(Displacement::Constant(-1.0).check().unwrap_err(), CoreError::DisplacementHeight { height_m: -1.0 });
        assert!(Displacement::Monthly(vec![1.0; 11]).check().is_err(), "twelve months");
        let no_sectors = Displacement::BySector { direction: "d".into(), heights: Vec::new(), backup: 0.0 };
        assert!(matches!(no_sectors.check().unwrap_err(), CoreError::SectorsOutOfRange { .. }));
    }
}

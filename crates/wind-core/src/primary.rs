//! Primary columns (ESPEC-INTERFACE §2.3, Colunas primárias): of the
//! measured mean columns of one type (wind components' speed and direction
//! count as measured), the one that represents the dataset:
//! the speed analyses start from, the temperature and pressure the air
//! density is computed from. The dataset says how to pick it
//! ([`Dataset::primary`]): a column by id, or a criterion on height.
//!
//! Height criteria follow the nearest-in-height rule [manual:
//! def-nearest-in-height-rule.htm]: columns within the colocation threshold
//! of the best count as tied, and the tie goes to the one with the most
//! data, then to the first. Columns without a height are picked only when no
//! column of the type has one.
use crate::model::{ColumnType, DataColumn, Dataset, PrimaryChoice, Subtype};

/// Height difference within which two sensors count as equally placed, m.
pub const COLOCATION_THRESHOLD_M: f64 = 1.0;

/// The primary column of `kind`, if the dataset measures that type.
pub fn primary(dataset: &Dataset, kind: ColumnType) -> Option<usize> {
    primary_among(dataset, kind, |_| true)
}

/// The primary column of `kind` among the columns `usable` accepts (the air
/// density takes only units it can convert). The dataset's choice for the
/// type, else the default: the highest, and for directions the nearest to
/// the primary speed. A chosen column that is not a usable measured mean of
/// the type falls back to the default.
pub fn primary_among(dataset: &Dataset, kind: ColumnType, usable: impl Fn(&DataColumn) -> bool) -> Option<usize> {
    let candidates: Vec<usize> = (0..dataset.columns.len())
        .filter(|&index| {
            let column = &dataset.columns[index];
            let measured = column.derived.as_ref().map_or(true, |derived| derived.is_measurement());
            column.kind == kind && column.subtype == Subtype::Mean && measured && usable(column)
        })
        .collect();
    let chosen = dataset.primary.iter().find(|(chosen_kind, _)| *chosen_kind == kind).map(|(_, choice)| choice);
    if let Some(PrimaryChoice::Column(id)) = chosen {
        if let Some(&index) = candidates.iter().find(|&&index| dataset.columns[index].id == *id) {
            return Some(index);
        }
    }
    let criterion = match chosen {
        Some(PrimaryChoice::Column(_)) | None => default_choice(dataset, kind),
        Some(criterion) => criterion.clone(),
    };
    best_placed(dataset, &candidates, &criterion)
}

/// What picks the primary column of `kind` when the dataset says nothing:
/// the highest; a direction is read where the primary speed is.
fn default_choice(dataset: &Dataset, kind: ColumnType) -> PrimaryChoice {
    let speed_height = || primary(dataset, ColumnType::WindSpeed).and_then(|speed| dataset.columns[speed].height_m);
    match (kind, kind == ColumnType::WindDirection) {
        (_, true) => speed_height().map_or(PrimaryChoice::Highest, PrimaryChoice::NearestTo),
        _ => PrimaryChoice::Highest,
    }
}

/// The candidate `criterion` places best, by the nearest-in-height rule.
fn best_placed(dataset: &Dataset, candidates: &[usize], criterion: &PrimaryChoice) -> Option<usize> {
    let distance = |height: f64| match *criterion {
        PrimaryChoice::Lowest => height,
        PrimaryChoice::NearestTo(target) => (height - target).abs(),
        PrimaryChoice::Highest | PrimaryChoice::Column(_) => -height,
    };
    let placed: Vec<(usize, f64)> =
        candidates.iter().filter_map(|&index| dataset.columns[index].height_m.map(|height| (index, distance(height)))).collect();
    let Some(best) = placed.iter().map(|&(_, distance)| distance).reduce(f64::min) else {
        return candidates.first().copied();
    };
    let valid = |index: usize| dataset.columns[index].values.iter().filter(|value| !value.is_nan()).count();
    placed
        .into_iter()
        .filter(|&(_, distance)| distance - best <= COLOCATION_THRESHOLD_M)
        .map(|(index, _)| (index, valid(index)))
        // Most data first, then the earlier column.
        .min_by_key(|&(index, valid)| (std::cmp::Reverse(valid), index))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DataColumn, TimeStampMeaning};

    fn column(id: &str, kind: ColumnType, height_m: Option<f64>, gaps: usize) -> DataColumn {
        let mut values = vec![1.0; 10];
        values[..gaps].fill(f64::NAN);
        DataColumn { id: id.into(), kind, subtype: Subtype::Mean, units: String::new(), height_m, parent: None, values: values.into(), derived: None }
    }

    fn dataset(columns: Vec<DataColumn>) -> Dataset {
        Dataset {
            name: "torre".into(),
            start: 0,
            step_s: 600,
            len: 10,
            timestamp_meaning: TimeStampMeaning::Start,
            utc_offset_min: None,
            latitude: None,
            longitude: None,
            elevation_m: None,
            columns,
            flags: Vec::new(),
            segments: Vec::new(),
            evidence: Default::default(),
            extrapolations: Vec::new(),
            calculated: Vec::new(),
            ti_cutoff_m_s: None,
            primary: Vec::new(),
            displacement: None,
            edits: Vec::new(),
        }
    }

    #[test]
    fn criteria_by_height_with_colocated_sensors_tied() {
        use ColumnType::{Temperature, WindDirection, WindSpeed};
        let mut tower = dataset(vec![
            column("ws100a", WindSpeed, Some(100.0), 3),
            column("ws100b", WindSpeed, Some(99.5), 0),
            column("ws60", WindSpeed, Some(60.0), 0),
            column("wd98", WindDirection, Some(98.0), 0),
            column("wd58", WindDirection, Some(58.0), 0),
            column("t", Temperature, None, 0),
        ]);
        assert_eq!(primary(&tower, WindSpeed), Some(1), "100 and 99.5 m are colocated: the one with more data");
        assert_eq!(primary(&tower, WindDirection), Some(3), "by default, where the primary speed is");
        assert_eq!(primary(&tower, Temperature), Some(5), "no heights: the first");
        assert_eq!(primary(&tower, ColumnType::Pressure), None);

        tower.primary = vec![(WindSpeed, PrimaryChoice::Lowest), (WindDirection, PrimaryChoice::Column("wd58".into()))];
        assert_eq!((primary(&tower, WindSpeed), primary(&tower, WindDirection)), (Some(2), Some(4)));
        tower.primary = vec![(WindSpeed, PrimaryChoice::NearestTo(75.0)), (WindDirection, PrimaryChoice::Column("ws60".into()))];
        assert_eq!(primary(&tower, WindSpeed), Some(2), "15 m from 60, 24.5 from 99.5");
        assert_eq!(primary(&tower, WindDirection), Some(4), "a speed is no direction: the default, now nearest to 60 m");
    }
}

//! The EPE export request ([`wind_core::export::epe`]): which columns go in
//! each group of the layout, chosen automatically when left out.
use crate::ops::{column_index, steps_between};
use crate::DatasetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::CoreResult;
use wind_core::export::epe::{EpeOptions, Group};
use wind_core::model::{ColumnType, Dataset, Subtype};
use wind_core::primary;

/// An EPE file of one dataset. Each column field takes a column id;
/// omitted = chosen automatically (the speeds and vanes by height: the
/// highest, the lowest and, with three speeds or more, one between; the
/// primary temperature, pressure and humidity); `""` = the station has no
/// such sensor (its fields are `-` and its bit is set, as EPE asks).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportEpeRequest {
    /// Dataset to export.
    pub dataset: DatasetId,
    /// The station's numeric code at EPE; a JSON number is read as its
    /// digits (`wind export … station=1234567`), text keeps leading zeros.
    #[serde(deserialize_with = "text_or_number")]
    pub station: String,
    /// First instant exported, epoch ms; omitted = the record's start.
    #[serde(default)]
    #[ts(optional)]
    pub from_ms: Option<f64>,
    /// Last instant exported, epoch ms; omitted = the record's end.
    #[serde(default)]
    #[ts(optional)]
    pub to_ms: Option<f64>,
    /// Upper anemometer (mean speed column).
    #[serde(default)]
    #[ts(optional)]
    pub upper_speed: Option<String>,
    /// Middle anemometer.
    #[serde(default)]
    #[ts(optional)]
    pub middle_speed: Option<String>,
    /// Lower anemometer.
    #[serde(default)]
    #[ts(optional)]
    pub lower_speed: Option<String>,
    /// Upper vane (mean direction column).
    #[serde(default)]
    #[ts(optional)]
    pub upper_vane: Option<String>,
    /// Lower vane.
    #[serde(default)]
    #[ts(optional)]
    pub lower_vane: Option<String>,
    /// Air pressure.
    #[serde(default)]
    #[ts(optional)]
    pub pressure: Option<String>,
    /// Air temperature.
    #[serde(default)]
    #[ts(optional)]
    pub temperature: Option<String>,
    /// Relative humidity.
    #[serde(default)]
    #[ts(optional)]
    pub humidity: Option<String>,
}

/// A string, or a JSON number as the text of its digits.
fn text_or_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Station {
        Text(String),
        Number(serde_json::Number),
    }
    Ok(match Station::deserialize(deserializer)? {
        Station::Text(text) => text,
        Station::Number(number) => number.to_string(),
    })
}

/// The EPE file and the name EPE asks for it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EpeFileDto {
    /// `<station>_<first day>_<last day>.txt`.
    pub name: String,
    /// The file's text.
    pub text: String,
}

impl ExportEpeRequest {
    /// The core's options, with ids resolved in `dataset` and the automatic
    /// choices made.
    ///
    /// # Errors
    /// [`wind_core::error::CoreError::UnknownColumn`] for a column `dataset` lacks.
    pub fn options(self, dataset: &Dataset) -> CoreResult<EpeOptions> {
        let by_height = |kind: ColumnType| {
            let mut columns: Vec<usize> = (0..dataset.columns.len())
                .filter(|&index| {
                    let column = &dataset.columns[index];
                    let measured = column.derived.as_ref().map_or(true, |derived| derived.is_measurement());
                    column.kind == kind && column.subtype == Subtype::Mean && measured && column.height_m.is_some()
                })
                .collect();
            // Highest first.
            columns.sort_by(|&a, &b| dataset.columns[b].height_m.partial_cmp(&dataset.columns[a].height_m).expect("heights are numbers"));
            columns
        };
        let speeds = by_height(ColumnType::WindSpeed);
        let vanes = by_height(ColumnType::WindDirection);
        let automatic = |group: Group| -> Option<usize> {
            match group {
                Group::UpperSpeed => speeds.first().copied(),
                Group::LowerSpeed => (speeds.len() >= 2).then(|| speeds[speeds.len() - 1]),
                Group::MiddleSpeed => (speeds.len() >= 3).then(|| speeds[speeds.len() / 2]),
                Group::UpperVane => vanes.first().copied(),
                Group::LowerVane => (vanes.len() >= 2).then(|| vanes[vanes.len() - 1]),
                Group::Pressure => primary::primary(dataset, ColumnType::Pressure),
                Group::Temperature => primary::primary(dataset, ColumnType::Temperature),
                Group::Humidity => primary::primary(dataset, ColumnType::RelativeHumidity),
            }
        };
        let chosen = [
            (Group::Pressure, self.pressure),
            (Group::Temperature, self.temperature),
            (Group::Humidity, self.humidity),
            (Group::UpperSpeed, self.upper_speed),
            (Group::UpperVane, self.upper_vane),
            (Group::LowerSpeed, self.lower_speed),
            (Group::LowerVane, self.lower_vane),
            (Group::MiddleSpeed, self.middle_speed),
        ];
        let mut groups = Vec::new();
        for (group, id) in chosen {
            let column = match id.as_deref() {
                None => automatic(group),
                Some("") => None,
                Some(id) => Some(column_index(dataset, id)?),
            };
            groups.extend(column.map(|column| (group, column)));
        }
        let steps = match (self.from_ms, self.to_ms) {
            (None, None) => 0..dataset.len,
            (from, to) => steps_between(dataset, from.unwrap_or(f64::NEG_INFINITY), to.unwrap_or(f64::INFINITY)),
        };
        Ok(EpeOptions { station: self.station, steps, groups })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wind_core::export::epe;
    use wind_core::filter::FilterView;
    use wind_core::import::{import_text, ImportConfig};

    /// The first two intervals of the example in NT EPE-DEE-RE-057/2016-R4
    /// (5.15), as a logger file: pressure, temperature (in kelvin, to see
    /// it converted), humidity, three anemometers with max, min and SD, two
    /// vanes with SD. The second interval has a 55 °C temperature and no
    /// upper mean.
    fn tower() -> Dataset {
        let csv = "Data;Pres 2m;Temp 2m;UR 2m;Vel 80m Média;Vel 80m Máx;Vel 80m Mín;Vel 80m DP;Dir 78m;Dir 78m DP;\
                   Vel 50m Média;Vel 50m Máx;Vel 50m Mín;Vel 50m DP;Dir 50m;Dir 50m DP;Vel 78m Média;Vel 78m Máx;Vel 78m Mín;Vel 78m DP\n\
                   2012-02-16 00:00;906;293,25;97;11,36;14,04;8,50;1,07;106;6;10,17;13,31;7,53;1,09;105;5;11,29;14,09;8,51;1,05\n\
                   2012-02-16 00:10;906;328,15;97;;15,10;8,18;1,35;106;6;9,89;13,95;6,57;1,49;105;6;10,98;15,33;8,23;1,37\n";
        let (mut dataset, _) = import_text("000001.csv", csv, &ImportConfig::default()).unwrap();
        crate::requests::finish_import(&mut dataset, None, None).unwrap();
        dataset
    }

    fn request(json: &str) -> ExportEpeRequest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_numeric_station_reads_as_its_digits() {
        assert_eq!(request(r#"{"dataset": 0, "station": 1234567}"#).station, "1234567");
        assert_eq!(request(r#"{"dataset": 0, "station": "000001"}"#).station, "000001", "text keeps its leading zeros");
    }

    #[test]
    fn the_note_s_example_reads_back_record_for_record() {
        let mut dataset = tower();
        // Every sensor chosen automatically, "UR 2m" read as the humidity.
        let options = request(r#"{"dataset": 0, "station": "000001"}"#).options(&dataset).unwrap();
        let text = epe::to_epe(&dataset, &FilterView::new(&dataset), &options).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(&lines[..3], ["Estação 000001", "Início 20120216000000", "Fim 20120216001000"]);
        assert_eq!(lines[3], "CH01 Dia do início do intervalo (de 10 minutos) de medição [AAAAMMDD]");
        assert_eq!(lines[9], "CH07 Anemômetro superior (80m), velocidade do vento [m/s]: média do intervalo");
        assert_eq!(lines[14], "CH12 Wind Vane superior (78m), direção de vento [°]: desvio padrão do intervalo");
        assert_eq!(lines[24], "CH22 Anemômetro 3 (78m), velocidade do vento [m/s]: desvio padrão do intervalo");
        assert_eq!(lines[25], "CH01|CH02|CH03|CH04|CH05|CH06|CH07|CH08|CH09|CH10|CH11|CH12|CH13|CH14|CH15|CH16|CH17|CH18|CH19|CH20|CH21|CH22|");
        assert_eq!(lines[26], "dados");
        // The note's first record, with the decimals its formats ask for (the example writes 0906|20,1|97,0).
        assert_eq!(lines[27], "20120216|000000|000|0906,00|20,10|097,00|11,36|14,04|08,50|01,07|106|006|10,17|13,31|07,53|01,09|105|005|11,29|14,09|08,51|01,05|");
        // 55 °C is implausible (5.4) and the upper mean is missing: "-" and bits 64 + 2.
        let second: Vec<&str> = lines[28].split('|').collect();
        assert_eq!((second[2], second[4], second[6]), ("066", "-", "-"), "{}", lines[28]);
        assert_eq!(lines[29], "fimdados");
        assert_eq!(epe::file_name(&dataset, &options), "000001_20120216_20120216.txt");

        // The note's flag example: a lower vane flagged for icing sets 16.
        let lower_vane = dataset.columns.iter().position(|column| column.id == "Dir 50m").unwrap();
        let icing = dataset.flags.iter().position(|flag| flag.name == "Gelo").unwrap();
        wind_core::flags::flag(&mut dataset, &[lower_vane], icing, 0..1).unwrap();
        let text = epe::to_epe(&dataset, &FilterView::new(&dataset), &options).unwrap();
        assert_eq!(text.lines().nth(27).unwrap().split('|').nth(2), Some("016"));
    }

    #[test]
    fn a_missing_sensor_is_dashes_with_its_bit_and_other_steps_are_refused() {
        let dataset = tower();
        let options = request(r#"{"dataset": 0, "station": "000001", "humidity": "", "middleSpeed": ""}"#).options(&dataset).unwrap();
        let text = epe::to_epe(&dataset, &FilterView::new(&dataset), &options).unwrap();
        let first: Vec<&str> = text.lines().nth(27).unwrap().split('|').collect();
        assert_eq!(first.len(), 23, "always the 22 channels, and the closing pipe");
        assert_eq!((first[2], first[5], &first[18..22]), ("160", "-", &["-", "-", "-", "-"][..]), "no hygrometer (128) nor intermediate anemometer (32)");
        assert!(epe::to_epe(&dataset, &FilterView::new(&dataset), &EpeOptions { station: "torre-1".into(), ..options.clone() }).is_err(), "the code is numeric");
        let mut hourly = tower();
        hourly.step_s = 3_600;
        let error = epe::to_epe(&hourly, &FilterView::new(&hourly), &options).unwrap_err();
        assert_eq!(error, wind_core::error::CoreError::EpeNeedsTenMinutes { step_s: 3_600 });
    }
}

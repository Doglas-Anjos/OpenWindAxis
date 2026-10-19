//! NRG SymphoniePRO text exports (SymphoniePRO Desktop, "Measurements"),
//! as NRG's own reader of them, the open-source nrgpy, parses them: tab
//! separated; a description header of key/value lines (Export Parameters,
//! Site Properties with Latitude, Longitude and Time Zone, Logger History,
//! and a Sensor History block per channel: `Channel:`, `Description:`,
//! `Height:`, `Units:`, …); a line `Data`; then the table, whose first
//! column is `Timestamp` and whose channel columns start `Ch<n>_`
//! ("Ch1_Anem_80.00m_N_Avg_m/s").
//!
//! [`adapt`] keeps the site lines as the preamble (the header can run past
//! the lines the importer searches for data), and names each channel the
//! importer would not recognise by its sensor's description from the Sensor
//! History ("Ch20_Analog_2.00m_N_Avg_C (NRG T60 Temperature)"). The table
//! is passed as it is.
use super::columns::infer_column;
use super::Adapted;
use crate::model::ColumnType;
use std::collections::HashMap;

/// The line before the table.
const DATA_MARK: &str = "Data";
/// The table's first header.
const TIMESTAMP_HEADER: &str = "Timestamp";
/// Field separator.
const SEPARATOR: char = '\t';
/// Header keys read, without their colons.
const CHANNEL_KEY: &str = "Channel";
const DESCRIPTION_KEY: &str = "Description";
/// Site keys kept as the preamble.
const SITE_KEYS: [&str; 4] = ["Latitude", "Longitude", "Elevation", TIME_ZONE_KEY];
/// The site's clock, written like "UTC-03:00".
const TIME_ZONE_KEY: &str = "Time Zone";
/// Prefix of a channel column, before its number.
const CHANNEL_PREFIX: &str = "Ch";

/// `text` rewritten with a short preamble and recognisable channel names
/// when it is a SymphoniePRO export; `None` otherwise. O(text).
pub(super) fn adapt(text: &str) -> Option<Adapted> {
    let mut lines = text.lines();
    let header_lines: Vec<&str> = lines.by_ref().take_while(|line| line.trim() != DATA_MARK).collect();
    let table_header = lines.next()?;
    if table_header.split(SEPARATOR).next()?.trim() != TIMESTAMP_HEADER || !header_lines.iter().any(|line| line.starts_with(CHANNEL_KEY)) {
        return None;
    }
    // The description of each channel, the last one its history gives.
    let mut descriptions: HashMap<String, String> = HashMap::new();
    let mut channel: Option<String> = None;
    let mut preamble = String::new();
    let mut utc_offset_min = None;
    for line in &header_lines {
        let Some((key, value)) = line.split_once(SEPARATOR) else { continue };
        let (key, value) = (key.trim().trim_end_matches(':'), value.trim());
        if key == CHANNEL_KEY {
            channel = Some(value.to_string());
        } else if key == DESCRIPTION_KEY {
            if let Some(channel) = &channel {
                descriptions.insert(channel.clone(), value.to_string());
            }
        } else if SITE_KEYS.contains(&key) && channel.is_none() {
            preamble.push_str(&format!("{key}: {value}\n"));
            if key == TIME_ZONE_KEY {
                utc_offset_min = super::ammonit::utc_offset_min(value);
            }
        }
    }
    let renamed: Vec<String> = table_header.split(SEPARATOR).map(|name| named(name.trim(), &descriptions)).collect();
    let mut adapted = String::with_capacity(text.len());
    adapted.push_str(&preamble);
    adapted.push_str(&renamed.join(&SEPARATOR.to_string()));
    adapted.push('\n');
    for line in lines {
        adapted.push_str(line);
        adapted.push('\n');
    }
    Some(Adapted { text: adapted, utc_offset_min, ..Default::default() })
}

/// A channel column's name, with its sensor's description after it when
/// the name alone reads as "other".
fn named(name: &str, descriptions: &HashMap<String, String>) -> String {
    let number = name.strip_prefix(CHANNEL_PREFIX).and_then(|rest| rest.split('_').next());
    match number.and_then(|number| descriptions.get(number)) {
        Some(description) if infer_column(name).0 == ColumnType::Other => format!("{name} ({description})"),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An export in nrgpy's layout: header blocks, `Data`, the table.
    const FILE: &str = "SymphoniePRO Logger data file ----- Measurements\n\
                        \n\
                        Export Parameters\n\
                        Export version:\t3.5\n\
                        Site Properties\n\
                        Site Number:\t001234\n\
                        Latitude:\t-5.1234\n\
                        Longitude:\t-36.4567\n\
                        Elevation:\t12\n\
                        Time Zone:\tUTC-03:00\n\
                        Sensor History\n\
                        Channel:\t1\n\
                        Type:\tAnemometer\n\
                        Description:\tNRG Class1 Anem\n\
                        Height:\t80.00\n\
                        Channel:\t20\n\
                        Type:\tAnalog\n\
                        Description:\tNRG T60 Temperature\n\
                        Height:\t2.00\n\
                        Data\n\
                        Timestamp\tCh1_Anem_80.00m_N_Avg_m/s\tCh1_Anem_80.00m_N_SD_m/s\tCh20_Analog_2.00m_N_Avg_C\n\
                        2023-01-01 00:00:00\t6.21\t0.62\t27.1\n\
                        2023-01-01 00:10:00\t6.50\t0.65\t27.0\n";

    #[test]
    fn analog_channels_take_their_descriptions_and_the_site_is_kept() {
        let adapted = adapt(FILE).expect("a SymphoniePRO export");
        let lines: Vec<&str> = adapted.text.lines().collect();
        assert_eq!(&lines[..4], ["Latitude: -5.1234", "Longitude: -36.4567", "Elevation: 12", "Time Zone: UTC-03:00"]);
        assert_eq!(lines[4], "Timestamp\tCh1_Anem_80.00m_N_Avg_m/s\tCh1_Anem_80.00m_N_SD_m/s\tCh20_Analog_2.00m_N_Avg_C (NRG T60 Temperature)");
        assert_eq!(lines[5], "2023-01-01 00:00:00\t6.21\t0.62\t27.1");
        assert_eq!(adapted.utc_offset_min, Some(-180));
        assert!(adapt("t,ws\n2023-01-01 00:00,5\n").is_none(), "an ordinary CSV is left alone");
    }
}

//! Campbell Scientific TOA5 files (the "Table Oriented ASCII" of
//! LoggerNet and CR-series loggers): four header lines, then data.
//! 1. `"TOA5"`, station name, logger model, serial number, OS, program,
//!    signature, table name;
//! 2. field names (`TIMESTAMP`, `RECORD`, `WS_80m_Avg`, …);
//! 3. units (`TS`, `RN`, `m/s`, `Deg`, …);
//! 4. processing (`Avg`, `Std`, `Max`, `Min`, `Smp`, …).
//!
//! [`adapt`] rewrites one as the CSV the importer reads: the identification
//! line as the preamble, the field names as the header (with the processing
//! after a name that does not already say it, so "WS80" + "Std" reads as a
//! standard deviation), and the data as it is, without the RECORD counter.
//! Campbell time stamps mark the end of each interval: the import report
//! names the format so the user can set "Carimbo indica" accordingly.
use super::layout::reader;
use super::Adapted;

/// First field of a TOA5 file.
const TOA5_MARK: &str = "TOA5";
/// The record counter column, dropped.
const RECORD_FIELD: &str = "RECORD";
/// Header lines before the data.
const HEADER_LINES: usize = 4;
/// Processing labels that name a statistic, and the word the importer reads
/// for it.
const STATISTICS: [(&str, &str); 3] = [("Std", "Std"), ("Max", "Max"), ("Min", "Min")];
/// Delimiter of the rewritten file.
const SEPARATOR: char = ',';

/// `text` rewritten as a plain CSV when it is a TOA5 file; `None` otherwise.
/// O(text).
pub(super) fn adapt(text: &str) -> Option<Adapted> {
    let mut records = reader(text, SEPARATOR).into_records().take(HEADER_LINES).filter_map(Result::ok);
    let identification = records.next()?;
    if identification.get(0)?.trim() != TOA5_MARK {
        return None;
    }
    let (names, _units, processing) = (records.next()?, records.next()?, records.next()?);
    let kept: Vec<usize> = (0..names.len()).filter(|&index| names.get(index).map(str::trim) != Some(RECORD_FIELD)).collect();
    let header: Vec<String> = kept.iter().map(|&index| named(names.get(index).unwrap_or_default(), processing.get(index).unwrap_or_default())).collect();

    let mut adapted = String::with_capacity(text.len());
    // The identification line, unquoted, as the preamble.
    adapted.push_str(&identification.iter().collect::<Vec<_>>().join(" "));
    adapted.push('\n');
    adapted.push_str(&header.join(&SEPARATOR.to_string()));
    adapted.push('\n');
    for record in reader(text, SEPARATOR).into_records().skip(HEADER_LINES).filter_map(Result::ok) {
        let fields: Vec<&str> = kept.iter().map(|&index| record.get(index).unwrap_or_default()).collect();
        adapted.push_str(&fields.join(&SEPARATOR.to_string()));
        adapted.push('\n');
    }
    Some(Adapted { text: adapted, utc_offset_min: None, ..Default::default() })
}

/// A field's name with its processing after it when that names a statistic
/// the name does not already carry; separators the importer splits on are
/// kept, a delimiter in the name is not.
fn named(name: &str, processing: &str) -> String {
    let name = name.trim().replace(SEPARATOR, " ");
    let statistic = STATISTICS.iter().find(|(label, _)| processing.trim().eq_ignore_ascii_case(label));
    match statistic {
        Some((_, word)) if !name.to_ascii_lowercase().contains(&word.to_ascii_lowercase()) => format!("{name} {word}"),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_processing_names_a_statistic_once() {
        assert_eq!(named("WS80", "Std"), "WS80 Std");
        assert_eq!(named("WS_80m_Std", "Std"), "WS_80m_Std", "already said");
        assert_eq!(named("WS_80m_Avg", "Avg"), "WS_80m_Avg");
        assert_eq!(named("WD_78m", "Smp"), "WD_78m");
        assert!(adapt("t,ws\n2023-01-01 00:00,5\n").is_none(), "an ordinary CSV is left alone");
    }
}

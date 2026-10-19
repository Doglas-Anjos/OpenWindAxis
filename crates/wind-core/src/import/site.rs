//! The site's coordinates from a logger preamble, such as
//! "Lat: -5,1234; Lon: -36,4567" or "Latitude = 12.5 N". Loggers write them
//! in the lines above the header; the map of the workbook places the tower
//! with them (ESPEC-INTERFACE §1.1). A configured location wins.

/// Labels of a latitude, longest first so "latitude" is not read as "lat".
const LATITUDE_LABELS: [&str; 2] = ["latitude", "lat"];
/// Labels of a longitude, longest first.
const LONGITUDE_LABELS: [&str; 3] = ["longitude", "long", "lon"];
/// Characters allowed between a label and its number.
const SEPARATORS: [char; 4] = [':', '=', ' ', '\t'];
/// Hemisphere letters that make a coordinate negative: south and west, in
/// English and Portuguese ("O" for oeste).
const NEGATIVE_HEMISPHERES: [char; 3] = ['s', 'w', 'o'];
/// Valid coordinates, ± degrees.
const MAX_LATITUDE_DEG: f64 = 90.0;
const MAX_LONGITUDE_DEG: f64 = 180.0;

/// The latitude and longitude written in `preamble` (the text above the
/// header), each `None` when absent or out of range.
pub(super) fn site_from_preamble(preamble: &str) -> (Option<f64>, Option<f64>) {
    let lower = preamble.to_lowercase();
    let latitude = find_coordinate(&lower, &LATITUDE_LABELS).filter(|value| value.abs() <= MAX_LATITUDE_DEG);
    let longitude = find_coordinate(&lower, &LONGITUDE_LABELS).filter(|value| value.abs() <= MAX_LONGITUDE_DEG);
    (latitude, longitude)
}

/// The first number after one of `labels` standing as a word in `text`.
fn find_coordinate(text: &str, labels: &[&str]) -> Option<f64> {
    for label in labels {
        let mut from = 0;
        while let Some(found) = text[from..].find(label) {
            let start = from + found;
            let end = start + label.len();
            from = end;
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            let standalone = !before.is_some_and(char::is_alphabetic) && !after.is_some_and(char::is_alphabetic);
            if let Some(value) = standalone.then(|| number_after(&text[end..])).flatten() {
                return Some(value);
            }
        }
    }
    None
}

/// A signed decimal (point or comma) at the start of `text`, after the
/// separators, negated by a south or west hemisphere letter that follows it.
fn number_after(text: &str) -> Option<f64> {
    let text = text.trim_start_matches(SEPARATORS);
    let length = text
        .char_indices()
        .take_while(|&(index, character)| character.is_ascii_digit() || ((character == '-' || character == '+') && index == 0) || character == '.' || character == ',')
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    // A trailing separator ("-5,12;" or "-5.12,") belongs to the line, not the number.
    let digits = text[..length].trim_end_matches(['.', ',']);
    let value: f64 = digits.replace(',', ".").parse().ok()?;
    let hemisphere = text[length..].trim_start().chars().next();
    let negative = hemisphere.is_some_and(|letter| NEGATIVE_HEMISPHERES.contains(&letter) && value > 0.0);
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_in_the_usual_logger_spellings() {
        assert_eq!(site_from_preamble("Site: Torre\nLat: -5,1234; Lon: -36,4567\n"), (Some(-5.1234), Some(-36.4567)));
        assert_eq!(site_from_preamble("Latitude = 12.5 N\nLongitude = 38.25 W\n"), (Some(12.5), Some(-38.25)));
        assert_eq!(site_from_preamble("LAT 10.5, LONG 20.75 O"), (Some(10.5), Some(-20.75)));
    }

    #[test]
    fn words_that_only_contain_a_label_are_not_coordinates() {
        assert_eq!(site_from_preamble("Plataforma 7; Lonas 3"), (None, None));
        assert_eq!(site_from_preamble("Lat: 123; Lon: 45"), (None, Some(45.0)), "a latitude past 90° is no latitude");
        assert_eq!(site_from_preamble("Logger SN 0001"), (None, None));
    }
}

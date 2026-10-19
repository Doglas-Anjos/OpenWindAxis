//! Fast parsing of the common ISO-like timestamps, with chrono as fallback.
use crate::time::{SECONDS_PER_DAY, SECONDS_PER_HOUR, SECONDS_PER_MINUTE};

// ---- Fast timestamps ----

/// Digits of a `%Y` field.
pub(super) const YEAR_DIGITS: usize = 4;
/// Digits of every other numeric field (`%m %d %H %M %S`).
pub(super) const FIELD_DIGITS: usize = 2;
/// Upper bounds accepted by the fast path; leap seconds (60) go to chrono.
pub(super) const LAST_MONTH: i64 = 12;
pub(super) const LAST_HOUR: i64 = 23;
pub(super) const LAST_MINUTE: i64 = 59;
pub(super) const LAST_SECOND: i64 = 59;
/// Years in one Gregorian cycle ("era"), after which the calendar repeats.
pub(super) const YEARS_PER_ERA: i64 = 400;
/// Days in one era.
pub(super) const DAYS_PER_ERA: i64 = 146_097;
/// Days from 0000-03-01 (the algorithm's origin) to 1970-01-01.
pub(super) const DAYS_FROM_ORIGIN_TO_EPOCH: i64 = 719_468;

/// A calendar field a [`FastTime`] piece fills.
#[derive(Clone, Copy)]
pub(super) enum Field {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

/// One piece of a timestamp layout.
#[derive(Clone, Copy)]
pub(super) enum Piece {
    /// A byte that must be there as is.
    Literal(u8),
    /// A zero-padded number of exactly `width` digits.
    Number { field: Field, width: usize },
}

/// Fast timestamp parser for formats made only of `%Y %m %d %H %M %S` and
/// single-byte literals, about 10× quicker than chrono's interpreter.
///
/// It accepts only zero-padded fixed-width fields with in-range values and
/// returns `None` for anything else, so the caller falls back to chrono and
/// every accepted input gets exactly chrono's answer.
pub(super) struct FastTime {
    pub(super) pieces: Vec<Piece>,
    /// Exact byte length of a matching stamp.
    pub(super) len: usize,
}

impl FastTime {
    /// `None` when the format uses anything beyond the supported pieces.
    pub(super) fn new(format: &str) -> Option<Self> {
        let (mut pieces, mut len) = (Vec::new(), 0);
        let mut bytes = format.bytes();
        while let Some(byte) = bytes.next() {
            let piece = if byte == b'%' {
                let (field, width) = match bytes.next()? {
                    b'Y' => (Field::Year, YEAR_DIGITS),
                    b'm' => (Field::Month, FIELD_DIGITS),
                    b'd' => (Field::Day, FIELD_DIGITS),
                    b'H' => (Field::Hour, FIELD_DIGITS),
                    b'M' => (Field::Minute, FIELD_DIGITS),
                    b'S' => (Field::Second, FIELD_DIGITS),
                    _ => return None,
                };
                Piece::Number { field, width }
            } else if byte == b' ' || (byte.is_ascii() && !byte.is_ascii_whitespace()) {
                Piece::Literal(byte)
            } else {
                return None;
            };
            len += match piece {
                Piece::Literal(_) => 1,
                Piece::Number { width, .. } => width,
            };
            pieces.push(piece);
        }
        Some(FastTime { pieces, len })
    }

    /// Seconds since 1970 for an exact match of the layout, else `None`.
    pub(super) fn parse(&self, stamp: &str) -> Option<i64> {
        let stamp = stamp.as_bytes();
        if stamp.len() != self.len {
            return None;
        }
        let (mut year, mut month, mut day) = (1970, 1, 1);
        let (mut hour, mut minute, mut second) = (0, 0, 0);
        let mut position = 0;
        for &piece in &self.pieces {
            match piece {
                Piece::Literal(expected) => {
                    if stamp[position] != expected {
                        return None;
                    }
                    position += 1;
                }
                Piece::Number { field, width } => {
                    let mut value = 0i64;
                    for &digit in &stamp[position..position + width] {
                        if !digit.is_ascii_digit() {
                            return None;
                        }
                        value = value * 10 + (digit - b'0') as i64;
                    }
                    match field {
                        Field::Year => year = value,
                        Field::Month => month = value,
                        Field::Day => day = value,
                        Field::Hour => hour = value,
                        Field::Minute => minute = value,
                        Field::Second => second = value,
                    }
                    position += width;
                }
            }
        }
        let valid_date = (1..=LAST_MONTH).contains(&month) && (1..=days_in_month(year, month)).contains(&day);
        if !valid_date || hour > LAST_HOUR || minute > LAST_MINUTE || second > LAST_SECOND {
            return None;
        }
        let seconds_of_day = hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + second;
        Some(days_from_civil(year, month, day) * SECONDS_PER_DAY + seconds_of_day)
    }
}

/// Days in `month` (1–12) of `year`, Gregorian leap rule.
pub(super) fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a proleptic Gregorian date (H. Hinnant's
/// algorithm: years counted from March, so the leap day ends the year).
pub(super) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year_from_march = if month <= 2 { year - 1 } else { year };
    let era = year_from_march.div_euclid(YEARS_PER_ERA);
    let year_of_era = year_from_march - era * YEARS_PER_ERA;
    let month_from_march = (month + 9) % 12;
    // Days before the month in a March-based year: the closed form of 31, 30, 31, 30, 31, 31, 30, ...
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    // 365 days a year plus the Gregorian leap days (every 4th year, not every 100th).
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_ERA + day_of_era - DAYS_FROM_ORIGIN_TO_EPOCH
}

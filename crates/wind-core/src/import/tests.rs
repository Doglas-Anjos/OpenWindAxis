//! Unit tests of the import stages.
use super::*;

/// Random cases per format in the timestamp property test.
const CASES_PER_FORMAT: usize = 20_000;
/// Random decimals in the number property test.
const DECIMAL_CASES: usize = 200_000;
/// 1900-01-01 and the span to 2100-01-01, seconds.
const YEAR_1900: i64 = -2_208_988_800;
const SPAN_1900_TO_2100: u64 = 6_311_433_600;

/// xorshift64*, for reproducible random cases.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn formatted(seconds: i64, format: &str) -> String {
    chrono::DateTime::from_timestamp(seconds, 0).unwrap().naive_utc().format(format).to_string()
}

#[test]
fn fast_time_agrees_with_chrono_or_defers() {
    let mut rng = Rng(99);
    let formats: Vec<String> = datetime_formats().into_iter().filter(|format| FastTime::new(format).is_some()).collect();
    assert!(formats.len() >= 24, "the common formats take the fast path: {formats:?}");
    let replacement_bytes = b"0123456789 /-:.T3";
    for format in &formats {
        let fast = FastTime::new(format).unwrap();
        // 1900..2100, any second: every well-formed stamp must match chrono exactly.
        for _ in 0..CASES_PER_FORMAT {
            let stamp = formatted(YEAR_1900 + (rng.next() % SPAN_1900_TO_2100) as i64, format);
            assert_eq!(fast.parse(&stamp), parse_time(&stamp, format), "{format}: {stamp}");
        }
        // Broken stamps: the fast path may defer (None) but never disagree.
        for _ in 0..CASES_PER_FORMAT {
            let mut bytes = formatted((rng.next() % SPAN_1900_TO_2100) as i64, format).into_bytes();
            let position = (rng.next() as usize) % bytes.len();
            bytes[position] = replacement_bytes[(rng.next() as usize) % replacement_bytes.len()];
            let stamp = String::from_utf8(bytes).unwrap();
            if let Some(seconds) = fast.parse(&stamp) {
                assert_eq!(Some(seconds), parse_time(&stamp, format), "{format}: {stamp}");
            }
        }
        let impossible = ["2015-02-29 00:00", "2015-04-31 00:00", "2015-13-01 00:00", "2015-01-01 24:00", "2016-02-29 23:59:58"];
        for stamp in impossible {
            if let Some(seconds) = fast.parse(stamp) {
                assert_eq!(Some(seconds), parse_time(stamp, format), "{format}: {stamp}");
            }
        }
    }
}

#[test]
fn plain_decimal_fast_path_gives_the_same_bits_as_str_parse() {
    let mut rng = Rng(5);
    for _ in 0..DECIMAL_CASES {
        let random = rng.next();
        let whole = (random >> 20) % 10u64.pow((random % 9) as u32 + 1);
        let decimals = (random >> 8) as usize % 8;
        let fraction = (random >> 40) % 10u64.pow(decimals as u32);
        let sign = if random & 1 == 1 { "-" } else { "" };
        let text = if decimals == 0 { format!("{sign}{whole}") } else { format!("{sign}{whole}.{fraction:0decimals$}") };
        let want: f64 = text.parse().unwrap();
        let got = parse_plain_decimal(text.as_bytes(), b'.');
        assert!(got.is_none() || got.unwrap().to_bits() == want.to_bits(), "{text}: {got:?} vs {want}");
        let with_comma = text.replace('.', ",");
        let got = parse_plain_decimal(with_comma.as_bytes(), b',');
        assert!(got.is_none() || got.unwrap().to_bits() == want.to_bits(), "{with_comma}: {got:?} vs {want}");
    }
    for (text, want) in [("-0", -0.0_f64), (".5", 0.5), ("5.", 5.0), ("007.50", 7.5)] {
        assert_eq!(parse_plain_decimal(text.as_bytes(), b'.').map(f64::to_bits), Some(want.to_bits()), "{text}");
    }
    for text in [".", "-", "+5", "1e3", "1.2.3", "inf", "1234567890123456"] {
        assert_eq!(parse_plain_decimal(text.as_bytes(), b'.'), None, "{text} goes to the general parser");
    }
}

#[test]
fn values_parse_numbers_first_with_the_same_outcomes() {
    let tokens = ImportConfig::default().missing;
    let numbers = [-999.0, -9999.0, -99999.0];
    let parse = |text: &str, decimal_comma: bool| parse_value(text, decimal_comma, &tokens, &numbers);
    assert_eq!(parse("5.25", false), (5.25, false));
    assert_eq!(parse(" -5,25 ", true), (-5.25, false));
    let longer_than_buffer = format!("1,{}", "0".repeat(DECIMAL_COMMA_BUFFER + 16));
    assert_eq!(parse(&longer_than_buffer, true).0, 1.0);
    for gap in ["", "NaN", "-", "N/A", "-999", "-999.0", "-999,0", "inf"] {
        let (value, unparsed) = parse(gap, true);
        assert!(value.is_nan() && !unparsed, "{gap:?} reads as missing, not as garbage");
    }
    let (value, unparsed) = parse("abc", false);
    assert!(value.is_nan() && unparsed, "garbage is reported");
}

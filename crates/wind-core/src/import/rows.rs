//! Stage 3: one streaming pass over the rows, with fast number parsing.
use crate::error::{CoreError, CoreResult};
use csv::StringRecord;
use super::*;

// ---- Numbers ----

/// Digits the exact decimal fast path handles: 10^15 < 2^53.
pub(super) const FAST_DECIMAL_MAX_DIGITS: usize = 15;
/// Exact powers of ten up to the fast path's digit limit.
pub(super) const POWERS_OF_TEN: [f64; FAST_DECIMAL_MAX_DIGITS + 1] =
    [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
/// Decimal-comma fields up to this length are rewritten on the stack.
pub(super) const DECIMAL_COMMA_BUFFER: usize = 64;

// ---- Stage 3: rows ----

/// The data rows in file order, before the grid.
pub(super) struct Rows {
    /// Timestamp of each kept row, seconds.
    pub(super) times: Vec<i64>,
    /// `[column][row]`.
    pub(super) values: Vec<Vec<f64>>,
    /// Per column: fields that were not numbers.
    pub(super) unparsed: Vec<usize>,
    /// Non-blank rows seen.
    pub(super) read: usize,
    /// Rows dropped for an unparsable timestamp.
    pub(super) bad_time: usize,
}

/// One streaming pass over the data rows.
pub(super) fn read_rows(text: &str, layout: &Layout, source_fields: &[usize], config: &ImportConfig) -> CoreResult<Rows> {
    let mut parser = RowParser::new(layout, source_fields, config, estimate_rows(text.len(), layout));
    let mut csv = reader(&text[layout.data_offset..], layout.delimiter);
    let mut record = StringRecord::new();
    while csv.read_record(&mut record).map_err(|error| CoreError::MalformedCsv { detail: error.to_string() })? {
        parser.push(&record);
    }
    let rows = parser.rows;
    if rows.times.len() < 2 {
        return Err(CoreError::TooFewTimestamps);
    }
    Ok(rows)
}

/// Rows the file probably holds, from the mean length of the sampled records,
/// so 30 columns × 1 M rows are allocated once instead of regrowing.
pub(super) fn estimate_rows(text_len: usize, layout: &Layout) -> usize {
    let sampled = layout.data_records().len();
    match (sampled > 1, layout.head.last()) {
        (true, Some((_, last_offset))) => {
            let mean_record_bytes = ((last_offset - layout.data_offset) / (sampled - 1)).max(1);
            ((text_len - layout.data_offset) / mean_record_bytes).saturating_add(sampled)
        }
        _ => 0,
    }
}

/// Parses one row per call into [`Rows`].
pub(super) struct RowParser<'a> {
    pub(super) fast_time: Option<FastTime>,
    pub(super) time_format: &'a str,
    pub(super) time_columns: &'a [usize],
    pub(super) source_fields: &'a [usize],
    pub(super) decimal_comma: bool,
    pub(super) missing_tokens: &'a [String],
    /// Numeric missing tokens, matched by value ("-999" also matches "-999.0").
    pub(super) missing_numbers: Vec<f64>,
    /// Reused to join separate date and time fields without allocating per row.
    pub(super) stamp_buffer: String,
    pub(super) rows: Rows,
}

impl<'a> RowParser<'a> {
    pub(super) fn new(layout: &'a Layout, source_fields: &'a [usize], config: &'a ImportConfig, capacity: usize) -> Self {
        let missing_numbers = config
            .missing
            .iter()
            .filter_map(|token| token.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .collect();
        RowParser {
            fast_time: FastTime::new(&layout.time_format),
            time_format: &layout.time_format,
            time_columns: &layout.time_columns,
            source_fields,
            decimal_comma: layout.decimal_comma,
            missing_tokens: &config.missing,
            missing_numbers,
            stamp_buffer: String::new(),
            rows: Rows {
                times: Vec::with_capacity(capacity),
                values: (0..source_fields.len()).map(|_| Vec::with_capacity(capacity)).collect(),
                unparsed: vec![0; source_fields.len()],
                read: 0,
                bad_time: 0,
            },
        }
    }

    /// Reads one record. Deliberately not inlined into the read loop: V8
    /// moves a WASM function to optimized code only between calls, so a
    /// per-row function is optimized after a few rows, while one big loop
    /// would run the whole file in baseline code.
    #[inline(never)]
    pub(super) fn push(&mut self, record: &StringRecord) {
        let stamp = stamp_text(record, self.time_columns, &mut self.stamp_buffer).trim();
        let parsed = self
            .fast_time
            .as_ref()
            .and_then(|fast| fast.parse(stamp))
            .or_else(|| parse_time(stamp, self.time_format));
        let Some(time) = parsed else {
            // Blank lines are skipped silently; anything else is a bad row.
            if !record.iter().all(|field| field.trim().is_empty()) {
                self.rows.read += 1;
                self.rows.bad_time += 1;
            }
            return;
        };
        self.rows.read += 1;
        self.rows.times.push(time);
        for (column, &field) in self.source_fields.iter().enumerate() {
            let text = record.get(field).unwrap_or("");
            let (value, unparsed) = parse_value(text, self.decimal_comma, self.missing_tokens, &self.missing_numbers);
            self.rows.unparsed[column] += unparsed as usize;
            self.rows.values[column].push(value);
        }
    }
}

/// Returns the value (NaN when missing) and whether the field was not a number.
///
/// Numbers are tried first and the missing-token list only on failure: every
/// numeric token of `missing_tokens` is also in `missing_numbers`, so the
/// result is the same as checking the list first, at a fraction of the cost.
pub(super) fn parse_value(field: &str, decimal_comma: bool, missing_tokens: &[String], missing_numbers: &[f64]) -> (f64, bool) {
    let text = field.trim();
    let separator = if decimal_comma { b',' } else { b'.' };
    let parsed = parse_plain_decimal(text.as_bytes(), separator).or_else(|| {
        if decimal_comma && text.contains(',') {
            parse_decimal_comma(text)
        } else {
            text.parse::<f64>().ok()
        }
    });
    match parsed {
        Some(value) if value.is_finite() && !missing_numbers.contains(&value) => (value, false),
        Some(_) => (f64::NAN, false),
        None => (f64::NAN, !missing_tokens.iter().any(|token| token == text)),
    }
}

/// Exact fast path for plain decimals such as "-12.345" (or "-12,345" with
/// `separator = b','`): with at most 15 digits both the integer mantissa and
/// 10^k are exact doubles, so one division gives the correctly rounded value,
/// the same bits as `str::parse` (Clinger 1990). Signs other than '-',
/// exponents, longer numbers and words return `None` for the general parser.
pub(super) fn parse_plain_decimal(text: &[u8], separator: u8) -> Option<f64> {
    let (negative, unsigned) = match text.first()? {
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    let (mut mantissa, mut digits, mut fraction_digits, mut after_separator) = (0u64, 0usize, 0usize, false);
    for &byte in unsigned {
        if byte.is_ascii_digit() {
            mantissa = mantissa * 10 + (byte - b'0') as u64;
            digits += 1;
            fraction_digits += after_separator as usize;
            if digits > FAST_DECIMAL_MAX_DIGITS {
                return None;
            }
        } else if byte == separator && !after_separator {
            after_separator = true;
        } else {
            return None;
        }
    }
    if digits == 0 {
        return None;
    }
    let magnitude = mantissa as f64 / POWERS_OF_TEN[fraction_digits];
    Some(if negative { -magnitude } else { magnitude })
}

/// Parses "5,25" as 5.25 through a stack buffer, without allocating.
pub(super) fn parse_decimal_comma(text: &str) -> Option<f64> {
    let mut buffer = [0u8; DECIMAL_COMMA_BUFFER];
    let Some(rewritten) = buffer.get_mut(..text.len()) else {
        return text.replace(',', ".").parse().ok();
    };
    for (out, &byte) in rewritten.iter_mut().zip(text.as_bytes()) {
        *out = if byte == b',' { b'.' } else { byte };
    }
    // Only ASCII ',' became ASCII '.', so the bytes are still valid UTF-8.
    std::str::from_utf8(rewritten).ok()?.parse().ok()
}

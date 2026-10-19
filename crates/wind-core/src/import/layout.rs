//! Stage 1: delimiter, header, timestamp columns and format.
use crate::error::{CoreError, CoreResult};
use chrono::NaiveDateTime;
use csv::{ReaderBuilder, StringRecord};
use std::borrow::Cow;
use std::collections::HashMap;
use super::*;

// ---- Detection ----

/// Records read from the top of the file to find the header, the first data
/// row and the layout.
pub(super) const HEAD_RECORDS: usize = 200;
/// Lines sampled across the whole file to choose between date formats.
pub(super) const FORMAT_SAMPLE_LINES: usize = 400;
/// Files up to this size are sampled whole.
pub(super) const SMALL_FILE_BYTES: usize = 64 * 1024;
/// Past this many head lines, only the second half votes on the delimiter,
/// so a logger preamble cannot win.
pub(super) const PREAMBLE_SKIP_MIN_LINES: usize = 20;
/// A format must parse at least this share of the sampled timestamps.
pub(super) const MIN_PARSED_SHARE: f64 = 0.5;
/// Delimiters tried, in tie order: `;` beats `,` because decimal commas also count.
pub(super) const DELIMITERS: [char; 4] = [';', '\t', '|', ','];
/// Date layouts tried when no format is configured.
pub(super) const DATE_FORMATS: [&str; 6] = ["%Y-%m-%d", "%Y/%m/%d", "%d/%m/%Y", "%m/%d/%Y", "%d.%m.%Y", "%d-%m-%Y"];
/// Time layouts tried after each date.
pub(super) const TIME_FORMATS: [&str; 3] = ["%H:%M:%S", "%H:%M:%S%.f", "%H:%M"];
/// Separators tried between date and time.
pub(super) const DATE_TIME_SEPARATORS: [&str; 2] = [" ", "T"];
/// Where the timestamp may be: one column, or date and time side by side.
pub(super) const TIMESTAMP_LAYOUTS: [&[usize]; 2] = [&[0], &[0, 1]];
/// Joins a separate date and time field before parsing.
pub(super) const DATE_TIME_JOIN: &str = " ";
/// UTF-8 byte order mark some editors put at the start of a file.
pub(super) const BYTE_ORDER_MARK: char = '\u{feff}';

// ---- Stage 1: layout ----

/// Where the data is and how to read it, worked out from the head of the file.
pub(super) struct Layout {
    pub(super) delimiter: char,
    /// The first records, each with its byte offset in the text.
    pub(super) head: Vec<(StringRecord, usize)>,
    /// Index in `head` of the first data row; the row before it is the header.
    pub(super) data_row: usize,
    /// Byte offset of the first data row.
    pub(super) data_offset: usize,
    /// File columns holding the timestamp.
    pub(super) time_columns: Vec<usize>,
    /// chrono format of the timestamps.
    pub(super) time_format: String,
    pub(super) decimal_comma: bool,
}

impl Layout {
    /// The header record, when the file has one.
    pub(super) fn header(&self) -> Option<&StringRecord> {
        self.data_row.checked_sub(1).map(|row| &self.head[row].0)
    }

    /// The sampled data records.
    pub(super) fn data_records(&self) -> &[(StringRecord, usize)] {
        &self.head[self.data_row..]
    }
}

/// Detects the delimiter, the header, the timestamp columns and format, and
/// the decimal separator, honouring every setting given in `config`.
pub(super) fn detect_layout(text: &str, config: &ImportConfig) -> CoreResult<Layout> {
    let head_lines: Vec<&str> = text.lines().take(HEAD_RECORDS).collect();
    if head_lines.iter().all(|line| line.trim().is_empty()) {
        return Err(CoreError::EmptyFile);
    }
    let delimiter = config.delimiter.unwrap_or_else(|| detect_delimiter(&head_lines));
    if !delimiter.is_ascii() {
        return Err(CoreError::NonAsciiDelimiter { delimiter });
    }
    let candidate_formats: Vec<String> = match &config.time_format {
        Some(format) => vec![format.clone()],
        None => datetime_formats(),
    };
    let head: Vec<(StringRecord, usize)> = reader(text, delimiter)
        .into_records()
        .take(HEAD_RECORDS)
        .filter_map(Result::ok)
        .map(|record| {
            let offset = record.position().map_or(0, |position| position.byte() as usize);
            (record, offset)
        })
        .collect();
    let time_columns_of = |record: &StringRecord| -> Option<Vec<usize>> {
        match &config.time_columns {
            Some(columns) => {
                let stamp = time_string(record, columns);
                candidate_formats.iter().any(|format| parse_time(&stamp, format).is_some()).then(|| columns.clone())
            }
            None => detect_time_columns(record, &candidate_formats),
        }
    };
    let data_row = match config.skip_rows {
        Some(skipped) => skipped + 1,
        None => (0..head.len())
            .find(|&row| {
                let next_also_has_time = row + 1 >= head.len() || time_columns_of(&head[row + 1].0).is_some();
                time_columns_of(&head[row].0).is_some() && next_also_has_time
            })
            .ok_or(CoreError::NoTimestampInHead { lines: HEAD_RECORDS })?,
    };
    let (first_row, data_offset) = head.get(data_row).ok_or(CoreError::SkipRowsPastEnd)?;
    let data_offset = *data_offset;
    let time_columns = time_columns_of(first_row).ok_or(CoreError::FirstRowWithoutTimestamp)?;
    let time_format = if candidate_formats.len() == 1 {
        candidate_formats[0].clone()
    } else {
        choose_format(&text[data_offset..], delimiter, &time_columns, &candidate_formats)?
    };
    let decimal_comma = config
        .decimal_comma
        .unwrap_or_else(|| guess_decimal_comma(&head[data_row..], delimiter, &time_columns));
    Ok(Layout { delimiter, head, data_row, data_offset, time_columns, time_format, decimal_comma })
}

pub(super) fn reader(text: &str, delimiter: char) -> csv::Reader<&[u8]> {
    ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter as u8)
        .from_reader(text.as_bytes())
}

/// Every date × separator × time combination, in priority order.
pub(super) fn datetime_formats() -> Vec<String> {
    let mut formats = Vec::new();
    for date in DATE_FORMATS {
        for separator in DATE_TIME_SEPARATORS {
            for time in TIME_FORMATS {
                formats.push(format!("{date}{separator}{time}"));
            }
        }
    }
    formats
}

/// Seconds since 1970 of `stamp` read with chrono `format`.
pub(super) fn parse_time(stamp: &str, format: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(stamp.trim(), format)
        .ok()
        .map(|time| time.and_utc().timestamp())
}

/// The timestamp text of a record: one field, or date and time joined.
pub(super) fn time_string<'a>(record: &'a StringRecord, columns: &[usize]) -> Cow<'a, str> {
    match columns {
        [column] => Cow::Borrowed(record.get(*column).unwrap_or("")),
        _ => Cow::Owned(
            columns
                .iter()
                .map(|&column| record.get(column).unwrap_or("").trim())
                .collect::<Vec<_>>()
                .join(DATE_TIME_JOIN),
        ),
    }
}

/// Like [`time_string`], for the per-row path: separate date and time fields
/// are joined into `buffer`, which stops allocating once it has grown.
pub(super) fn stamp_text<'a>(record: &'a StringRecord, columns: &[usize], buffer: &'a mut String) -> &'a str {
    if let [column] = columns {
        return record.get(*column).unwrap_or("");
    }
    buffer.clear();
    for (position, &column) in columns.iter().enumerate() {
        if position > 0 {
            buffer.push_str(DATE_TIME_JOIN);
        }
        buffer.push_str(record.get(column).unwrap_or("").trim());
    }
    buffer
}

/// The first [`TIMESTAMP_LAYOUTS`] entry whose fields parse as a timestamp.
pub(super) fn detect_time_columns(record: &StringRecord, formats: &[String]) -> Option<Vec<usize>> {
    TIMESTAMP_LAYOUTS
        .iter()
        .find(|columns| {
            let last = columns[columns.len() - 1];
            record.len() > last && {
                let stamp = time_string(record, columns);
                formats.iter().any(|format| parse_time(&stamp, format).is_some())
            }
        })
        .map(|columns| columns.to_vec())
}

/// Pick the format that parses most rows of a sample spread over the whole
/// file, so day-first vs month-first is settled by any day above 12.
/// ponytail: a file shorter than 12 days with every day <= 12 stays ambiguous; day-first wins the tie.
pub(super) fn choose_format(body: &str, delimiter: char, columns: &[usize], formats: &[String]) -> CoreResult<String> {
    let sample = spread_lines(body, FORMAT_SAMPLE_LINES).join("\n");
    let stamps: Vec<String> = reader(&sample, delimiter)
        .into_records()
        .filter_map(Result::ok)
        .map(|record| time_string(&record, columns).into_owned())
        .collect();
    let mut best: Option<&String> = None;
    let mut best_parsed = 0;
    for format in formats {
        let parsed = stamps.iter().filter(|stamp| parse_time(stamp, format).is_some()).count();
        if parsed > best_parsed {
            best = Some(format);
            best_parsed = parsed;
        }
    }
    match best {
        Some(format) if best_parsed as f64 >= MIN_PARSED_SHARE * stamps.len() as f64 => Ok(format.clone()),
        _ => Err(CoreError::TimeFormatNotRecognized { parsed: best_parsed, sampled: stamps.len() }),
    }
}

/// About `count` lines spread evenly over `body` (all lines if it is small).
pub(super) fn spread_lines(body: &str, count: usize) -> Vec<&str> {
    if body.len() <= SMALL_FILE_BYTES {
        return body.lines().filter(|line| !line.trim().is_empty()).collect();
    }
    let bytes = body.as_bytes();
    let mut lines = Vec::with_capacity(count);
    for sample in 0..count {
        // In u64: length × sample overflows a 32-bit usize (WASM) past ~10 MB.
        let position = (body.len() as u64 * sample as u64 / count as u64) as usize;
        let start = if sample == 0 {
            0
        } else {
            match bytes[position..].iter().position(|&byte| byte == b'\n') {
                Some(offset) => position + offset + 1,
                None => break,
            }
        };
        let end = bytes[start..].iter().position(|&byte| byte == b'\n').map_or(body.len(), |offset| start + offset);
        let line = body[start..end].trim_end_matches('\r');
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// The delimiter whose per-line count is most consistent (and non-zero),
/// looking past a possible logger preamble.
pub(super) fn detect_delimiter(lines: &[&str]) -> char {
    let body: Vec<&str> = lines.iter().copied().filter(|line| !line.trim().is_empty()).collect();
    let body = if body.len() > PREAMBLE_SKIP_MIN_LINES { &body[body.len() / 2..] } else { &body[..] };
    let (mut best, mut best_lines, mut best_count) = (',', 0usize, 0usize);
    for delimiter in DELIMITERS {
        let mut lines_by_count: HashMap<usize, usize> = HashMap::new();
        for line in body {
            *lines_by_count.entry(line.matches(delimiter).count()).or_default() += 1;
        }
        let most_common = lines_by_count
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .max_by_key(|&(count, lines)| (lines, count));
        if let Some((count, lines)) = most_common {
            if (lines, count) > (best_lines, best_count) {
                (best, best_lines, best_count) = (delimiter, lines, count);
            }
        }
    }
    best
}

/// Whether the sampled data fields use decimal commas: more fields look like
/// "5,25" than "5.25". Never with a comma delimiter.
pub(super) fn guess_decimal_comma(records: &[(StringRecord, usize)], delimiter: char, time_columns: &[usize]) -> bool {
    if delimiter == ',' {
        return false;
    }
    let (mut with_comma, mut with_dot) = (0usize, 0usize);
    for (record, _) in records {
        let data_fields = record.iter().enumerate().filter(|(field, _)| !time_columns.contains(field));
        for (_, field) in data_fields {
            if looks_decimal(field, ',') {
                with_comma += 1;
            } else if looks_decimal(field, '.') {
                with_dot += 1;
            }
        }
    }
    with_comma > with_dot
}

/// Whether `field` is digits, `separator`, digits (with an optional sign).
pub(super) fn looks_decimal(field: &str, separator: char) -> bool {
    let unsigned = field.trim().trim_start_matches(['-', '+']);
    match unsigned.split_once(separator) {
        Some((whole, fraction)) => {
            !whole.is_empty()
                && !fraction.is_empty()
                && whole.bytes().all(|byte| byte.is_ascii_digit())
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
        }
        None => false,
    }
}

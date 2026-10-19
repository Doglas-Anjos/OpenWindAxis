//! Where text import spends its time: 1 M rows × 30 columns of 10-min data
//! (~170 MB), split into the CSV scan, timestamp parsing and number parsing.
//! Run: `cargo bench -p wind-core --bench import` (native, one thread).
use chrono::NaiveDateTime;
use csv::{ByteRecord, ReaderBuilder, StringRecord};
use std::hint::black_box;
use std::time::Instant;
use wind_core::import::{import_text, ImportConfig};

/// Rows in the benchmark file.
const ROWS: usize = 1_000_000;
/// Rows used for the per-field micro-measures (results scale ×10).
const SAMPLE_ROWS: usize = 100_000;
/// Timestamp of the first row.
const FIRST_STAMP: &str = "2005-01-01 00:00";
/// Minutes between rows.
const STEP_MINUTES: i64 = 10;
/// Speed heights (m), each written as a mean and an SD column.
const SPEED_HEIGHTS: [usize; 10] = [40, 50, 60, 70, 80, 90, 100, 110, 120, 130];
const DIRECTION_SENSORS: usize = 5;
const TEMPERATURE_SENSORS: usize = 5;
/// Repetitions per measurement; the best one is reported.
const REPETITIONS: usize = 3;
const ISO_FORMAT: &str = "%Y-%m-%d %H:%M";
const BRAZILIAN_FORMAT: &str = "%d/%m/%Y %H:%M";

/// The same file layout as bench/wasm_bench.mjs; `brazilian` writes the
/// Brazilian variant (semicolons, decimal commas, day-first dates).
fn csv_text(brazilian: bool) -> String {
    let mut state: u32 = 2_463_534_242;
    let mut uniform = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f64 / 4_294_967_296.0
    };
    let time_format = if brazilian { BRAZILIAN_FORMAT } else { ISO_FORMAT };
    let mut text = String::from("time");
    for height in SPEED_HEIGHTS {
        text += &format!(",WS{height}_Avg,WS{height}_SD");
    }
    for sensor in 0..DIRECTION_SENSORS {
        text += &format!(",Dir{}", 100 + sensor);
    }
    for sensor in 0..TEMPERATURE_SENSORS {
        text += &format!(",Temp{}m", 2 + sensor);
    }
    let first = NaiveDateTime::parse_from_str(FIRST_STAMP, ISO_FORMAT).unwrap();
    for row in 0..ROWS {
        let stamp = first + chrono::Duration::minutes(STEP_MINUTES * row as i64);
        text += &format!("\n{}", stamp.format(time_format));
        for _ in SPEED_HEIGHTS {
            text += &format!(",{:.2},{:.2}", 8.0 * (-(1.0 - uniform()).ln()).sqrt(), uniform());
        }
        for _ in 0..DIRECTION_SENSORS {
            text += &format!(",{:.1}", 360.0 * uniform());
        }
        for _ in 0..TEMPERATURE_SENSORS {
            text += &format!(",{:.1}", 25.0 + 5.0 * uniform());
        }
    }
    if brazilian {
        // No field holds a comma yet: swap the separators, then the decimals.
        text = text.replace(',', ";").replace('.', ",");
    }
    text
}

fn time<T>(label: &str, mut run: impl FnMut() -> T) {
    let mut best = f64::INFINITY;
    for _ in 0..REPETITIONS {
        let started = Instant::now();
        black_box(run());
        best = best.min(started.elapsed().as_secs_f64() * 1e3);
    }
    println!("{label:<44} {best:>9.1} ms");
}

fn reader(text: &str) -> csv::Reader<&[u8]> {
    ReaderBuilder::new().has_headers(false).flexible(true).from_reader(text.as_bytes())
}

fn main() {
    let brazilian = csv_text(true);
    time("import_text, BR (; , dd/mm/yyyy)", || import_text("b", &brazilian, &ImportConfig::default()).unwrap());
    drop(brazilian);
    let text = csv_text(false);
    println!("file: {:.0} MB, {ROWS} rows", text.len() as f64 / 1e6);
    time("import_text (everything)", || import_text("b", &text, &ImportConfig::default()).unwrap());
    time("csv scan, StringRecord", || {
        let (mut csv, mut record, mut fields) = (reader(&text), StringRecord::new(), 0usize);
        while csv.read_record(&mut record).unwrap() {
            fields += record.len();
        }
        fields
    });
    time("csv scan, ByteRecord (no UTF-8 check)", || {
        let (mut csv, mut record, mut fields) = (reader(&text), ByteRecord::new(), 0usize);
        while csv.read_byte_record(&mut record).unwrap() {
            fields += record.len();
        }
        fields
    });
    let stamps: Vec<String> = text.lines().skip(1).map(|line| line[..FIRST_STAMP.len()].to_string()).collect();
    time("chrono parse of 1 M timestamps", || {
        stamps.iter().filter_map(|stamp| NaiveDateTime::parse_from_str(stamp.trim(), ISO_FORMAT).ok()).count()
    });
    let fields: Vec<&str> = text.lines().skip(1).take(SAMPLE_ROWS).flat_map(|line| line.split(',').skip(1)).collect();
    let missing_tokens: Vec<String> = ImportConfig::default().missing;
    time("30 M numbers: missing-list check (x10 est.)", || {
        fields.iter().filter(|field| missing_tokens.iter().any(|token| token == field.trim())).count()
    });
    time("30 M numbers: str::parse::<f64> (x10 est.)", || {
        fields.iter().filter_map(|field| field.trim().parse::<f64>().ok()).map(|value| value as u64).sum::<u64>()
    });
}

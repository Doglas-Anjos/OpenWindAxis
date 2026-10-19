//! Every way the core can fail, in one place (ARQUITETURA §2.4). Each variant
//! carries the values a message needs and has a stable snake_case [`code`]
//! for the UI to translate; [`Display`] gives the English text.
//!
//! [`code`]: CoreError::code
//! [`Display`]: std::fmt::Display
use crate::weibull::Method;
use std::fmt;

/// A failure of an import, export, edit or analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreError {
    // ---- Import ----
    /// The file has no non-blank line.
    EmptyFile,
    /// Delimiters must be one ASCII byte.
    NonAsciiDelimiter {
        /// The delimiter asked for.
        delimiter: char,
    },
    /// No row in the first `lines` parses as a timestamp.
    NoTimestampInHead {
        /// Lines searched.
        lines: usize,
    },
    /// `skip_rows` points past the last record.
    SkipRowsPastEnd,
    /// The row chosen as first data row has no timestamp.
    FirstRowWithoutTimestamp,
    /// Only timestamp columns were found.
    NoDataColumns,
    /// Fewer than two rows have a valid timestamp.
    TooFewTimestamps,
    /// A configured time step that is zero or negative.
    StepNotPositive {
        /// The configured step, s.
        step_s: i64,
    },
    /// Every timestamp is the same, so no step can be inferred.
    IdenticalTimestamps,
    /// The timestamps cover far more steps than there are rows (a wrong
    /// format, step or an outlier timestamp).
    TimestampsSpanTooMany {
        /// Steps the timestamps would span.
        steps: i64,
        /// The time step, s.
        step_s: i64,
        /// Rows read.
        rows: usize,
    },
    /// A selection shape with fewer than three corners.
    ShapeTooFewVertices {
        /// Its corners.
        vertices: usize,
    },
    /// An appended file has another time step than the dataset.
    AppendStepMismatch {
        /// The dataset's step, s.
        master_s: i64,
        /// The file's step, s.
        appended_s: i64,
    },
    /// The appended files' sizes add up to more than the import buffer.
    AppendedBytesExceedBuffer {
        /// The buffer's length, bytes.
        buffer: usize,
    },
    /// An appended file's steps fall between the dataset's.
    AppendOutOfPhase {
        /// How far into a step of the dataset the file's steps start, s.
        offset_s: i64,
    },
    /// No candidate format parses at least half of the sampled rows.
    TimeFormatNotRecognized {
        /// Rows the best format parsed.
        parsed: usize,
        /// Rows sampled.
        sampled: usize,
    },
    /// The CSV reader failed (I/O-level problem in the text).
    MalformedCsv {
        /// The CSV reader's message.
        detail: String,
    },
    /// A site elevation that is not a number inside the standard
    /// atmosphere's range.
    ElevationOutOfRange {
        /// The elevation given, m.
        elevation_m: f64,
        /// Lowest accepted, m.
        min_m: f64,
        /// Highest accepted, m.
        max_m: f64,
    },

    // ---- Export ----
    /// A decimal comma would be ambiguous with a comma delimiter.
    DecimalCommaWithCommaDelimiter,
    /// The strftime pattern does not parse.
    InvalidTimeFormat {
        /// The pattern given.
        format: String,
    },
    /// A step's time cannot be written as a calendar date.
    TimestampOutOfRange {
        /// The time, epoch seconds.
        seconds: i64,
    },
    /// Latitude or longitude is needed but neither the request nor the
    /// dataset has it.
    SiteLocationUnknown,
    /// Latitude outside ±90° or longitude outside ±180°.
    SiteLocationOutOfRange {
        /// Degrees north.
        latitude: f64,
        /// Degrees east.
        longitude: f64,
    },
    /// A displacement height that is negative or not a number, or (in an
    /// analysis) not below every sensor it applies to.
    DisplacementHeight {
        /// The height, m; NaN for a monthly list without twelve.
        height_m: f64,
    },
    /// An EPE station code that is not a number (it is numeric, and it
    /// names the file).
    EpeStationInvalid {
        /// The station given.
        station: String,
    },
    /// An EPE export of a dataset whose step is not the 10 minutes EPE fixes.
    EpeNeedsTenMinutes {
        /// The dataset's step, s.
        step_s: i64,
    },
    /// A clock offset from UTC no time zone has.
    UtcOffsetOutOfRange {
        /// The offset given, minutes.
        minutes: i32,
    },
    /// A height that is zero, negative or not a number.
    HeightNotPositive {
        /// The height given, m.
        height_m: f64,
    },

    // ---- Project file ----
    /// Wrong signature, framing or manifest: not a project, or a damaged one.
    NotAProject,
    /// The project was written by a newer program, in layout `found`.
    NewerProjectFormat {
        /// The file's layout version.
        found: f64,
        /// The newest this program reads.
        supported: u32,
    },

    // ---- Edits and lookups ----
    /// A flag index past the dataset's flags.
    NoSuchFlag {
        /// The index given.
        flag: usize,
    },
    /// A flag name that is empty or already taken (names compare without
    /// case or surrounding spaces).
    FlagNameInvalid {
        /// The name given.
        name: String,
    },
    /// A column index past the dataset's columns.
    NoSuchColumn {
        /// The index given.
        column: usize,
    },
    /// A saved segment that is empty or runs past the record.
    SegmentOutOfRange {
        /// First step.
        start: usize,
        /// One past the last step.
        end: usize,
        /// Steps in the record.
        len: usize,
    },
    /// No column has this id.
    UnknownColumn {
        /// The id given.
        id: String,
    },
    /// A dataset index past the session's datasets.
    NoSuchDataset {
        /// The id given.
        dataset: usize,
    },
    /// Derived columns inherit their sources' flags and take none of their own.
    DerivedColumnNotFlaggable {
        /// Id of the derived column.
        column: String,
    },
    /// Derived columns are computed from their sources and take no edit of
    /// their own.
    DerivedColumnNotEditable {
        /// Id of the derived column.
        column: String,
    },
    /// A scale or offset that is not a finite number.
    ScaleOffsetNotFinite {
        /// The scale given.
        scale: f64,
        /// The offset given.
        offset: f64,
    },

    // ---- Analyses ----
    /// An averaging period shorter than a dataset's time step.
    AveragingShorterThanStep {
        /// The averaging period, s.
        period_s: i64,
        /// The dataset's step, s.
        step_s: i64,
    },
    /// Too few periods where the site and the reference both have data.
    TooFewConcurrentPeriods {
        /// Periods found.
        found: usize,
        /// Fewest a line is drawn from.
        needed: usize,
    },
    /// A Markov reconstruction of a direction: its bins would cut the circle.
    MarkovNeedsLinearColumn {
        /// Id of the column.
        column: String,
    },
    /// The MCP method asked for could not be fitted to these data (a
    /// Weibull without a solution, too few vertical slices), or none could
    /// (a reference without spread).
    McpMethodUnavailable,
    /// Too few peaks (periods with enough data, or storms) for a Gumbel fit.
    TooFewPeaks {
        /// Peaks found.
        found: usize,
        /// Fewest the fit takes.
        needed: usize,
    },
    /// The Gumbel likelihood has no solution (the peaks have no spread).
    GumbelNoSolution,
    /// A speed column without a turbulence intensity column (it needs an SD child).
    NoTurbulenceIntensity {
        /// Id of the speed column.
        column: String,
    },
    /// Histogram bins must have a positive width.
    BinWidthNotPositive,
    /// Every sample was excluded or missing.
    NoDataAfterFilter,
    /// The data range needs more bins than allowed (usually an outlier).
    TooManyBins {
        /// Bins the range needs.
        bins: f64,
        /// Lowest value.
        low: f64,
        /// Highest value.
        high: f64,
        /// Most bins allowed.
        max: usize,
    },
    /// Sector counts must be in `1..=max`.
    SectorsOutOfRange {
        /// Sectors asked for.
        sectors: usize,
        /// Most sectors allowed.
        max: usize,
    },
    /// A calm threshold must be a speed ≥ 0.
    CalmThresholdInvalid {
        /// The threshold given, m/s.
        calm_threshold: f64,
    },
    /// A "Filtrar por" criterion with a value that makes no sense (a month
    /// past 12, an empty value range, a flag list of the wrong length...).
    FilterCriterionInvalid {
        /// Which criterion: "flags", "months", "hours", "sector" or "value".
        criterion: &'static str,
        /// The value refused.
        value: f64,
    },
    /// Shear needs a positive height on every speed column.
    ColumnWithoutHeight {
        /// Id of the column.
        column: String,
    },
    /// Shear needs speed columns at two or more heights.
    ShearNeedsTwoHeights,
    /// Yearly divisions of an MCP must be 1, 2, 3, 4, 6 or 12.
    YearlyDivisionsInvalid {
        /// Divisions asked for.
        divisions: usize,
    },
    /// A shear display subdivides by at most `max` divisions at once.
    TooManyDivisions {
        /// Divisions asked for.
        divisions: usize,
        /// Most allowed.
        max: usize,
    },
    /// No step where every selected column has data.
    NoConcurrentStep,

    // ---- Weibull ----
    /// Fewer than two non-negative speeds.
    TooFewSpeeds,
    /// Fewer than two distinct positive speeds.
    TooFewDistinctSpeeds,
    /// The moment-matching equation has no root for these speeds.
    WeibullNoSolution {
        /// The method that found no root.
        method: Method,
    },
    /// The least-squares line came out flat or falling.
    NonPositiveShape,

    // ---- Flag rules ----
    /// A range rule needs `min < max` (either bound may be infinite).
    RuleRangeInvalid {
        /// Lower bound given.
        min: f64,
        /// Upper bound given.
        max: f64,
    },
    /// A change limit or stuck tolerance must be finite and not negative.
    RuleThresholdInvalid {
        /// The limit or tolerance given.
        threshold: f64,
    },
    /// A rule's minimum duration is shorter than its test can see.
    RuleMinStepsTooSmall {
        /// Duration given, steps.
        min_steps: usize,
        /// Shortest the test can see, steps.
        least: usize,
    },
    /// More rules in one run than allowed.
    TooManyRules {
        /// Rules given.
        rules: usize,
        /// Most rules allowed.
        max: usize,
    },
    /// The rules found more stretches than one run may flag.
    TooManyRuleHits {
        /// Most stretches allowed.
        max: usize,
    },
    /// Saved evidence for a stretch that no segment of its column and flag covers.
    EvidenceNotFlagged {
        /// Column index.
        column: usize,
        /// First step.
        start: usize,
        /// One past the last step.
        end: usize,
    },
}

impl CoreError {
    /// Stable identifier, safe to match on in the UI and in logs.
    pub fn code(&self) -> &'static str {
        use CoreError::*;
        match self {
            EmptyFile => "empty_file",
            NonAsciiDelimiter { .. } => "non_ascii_delimiter",
            NoTimestampInHead { .. } => "no_timestamp_in_head",
            SkipRowsPastEnd => "skip_rows_past_end",
            FirstRowWithoutTimestamp => "first_row_without_timestamp",
            NoDataColumns => "no_data_columns",
            TooFewTimestamps => "too_few_timestamps",
            StepNotPositive { .. } => "step_not_positive",
            IdenticalTimestamps => "identical_timestamps",
            TimestampsSpanTooMany { .. } => "timestamps_span_too_many",
            AppendStepMismatch { .. } => "append_step_mismatch",
            ShapeTooFewVertices { .. } => "shape_too_few_vertices",
            AppendOutOfPhase { .. } => "append_out_of_phase",
            AppendedBytesExceedBuffer { .. } => "appended_bytes_exceed_buffer",
            TimeFormatNotRecognized { .. } => "time_format_not_recognized",
            MalformedCsv { .. } => "malformed_csv",
            ElevationOutOfRange { .. } => "elevation_out_of_range",
            DecimalCommaWithCommaDelimiter => "decimal_comma_with_comma_delimiter",
            InvalidTimeFormat { .. } => "invalid_time_format",
            TimestampOutOfRange { .. } => "timestamp_out_of_range",
            SiteLocationUnknown => "site_location_unknown",
            SiteLocationOutOfRange { .. } => "site_location_out_of_range",
            DisplacementHeight { .. } => "displacement_height",
            EpeStationInvalid { .. } => "epe_station_invalid",
            EpeNeedsTenMinutes { .. } => "epe_needs_ten_minutes",
            UtcOffsetOutOfRange { .. } => "utc_offset_out_of_range",
            HeightNotPositive { .. } => "height_not_positive",
            NotAProject => "not_a_project",
            NewerProjectFormat { .. } => "newer_project_format",
            NoSuchFlag { .. } => "no_such_flag",
            FlagNameInvalid { .. } => "flag_name_invalid",
            NoSuchColumn { .. } => "no_such_column",
            SegmentOutOfRange { .. } => "segment_out_of_range",
            UnknownColumn { .. } => "unknown_column",
            NoSuchDataset { .. } => "no_such_dataset",
            DerivedColumnNotFlaggable { .. } => "derived_column_not_flaggable",
            DerivedColumnNotEditable { .. } => "derived_column_not_editable",
            ScaleOffsetNotFinite { .. } => "scale_offset_not_finite",
            AveragingShorterThanStep { .. } => "averaging_shorter_than_step",
            TooFewConcurrentPeriods { .. } => "too_few_concurrent_periods",
            McpMethodUnavailable => "mcp_method_unavailable",
            MarkovNeedsLinearColumn { .. } => "markov_needs_linear_column",
            TooFewPeaks { .. } => "too_few_peaks",
            GumbelNoSolution => "gumbel_no_solution",
            NoTurbulenceIntensity { .. } => "no_turbulence_intensity",
            BinWidthNotPositive => "bin_width_not_positive",
            NoDataAfterFilter => "no_data_after_filter",
            TooManyBins { .. } => "too_many_bins",
            SectorsOutOfRange { .. } => "sectors_out_of_range",
            CalmThresholdInvalid { .. } => "calm_threshold_invalid",
            FilterCriterionInvalid { .. } => "filter_criterion_invalid",
            ColumnWithoutHeight { .. } => "column_without_height",
            ShearNeedsTwoHeights => "shear_needs_two_heights",
            TooManyDivisions { .. } => "too_many_divisions",
            YearlyDivisionsInvalid { .. } => "yearly_divisions_invalid",
            NoConcurrentStep => "no_concurrent_step",
            TooFewSpeeds => "too_few_speeds",
            TooFewDistinctSpeeds => "too_few_distinct_speeds",
            WeibullNoSolution { .. } => "weibull_no_solution",
            NonPositiveShape => "non_positive_shape",
            RuleRangeInvalid { .. } => "rule_range_invalid",
            RuleThresholdInvalid { .. } => "rule_threshold_invalid",
            RuleMinStepsTooSmall { .. } => "rule_min_steps_too_small",
            TooManyRules { .. } => "too_many_rules",
            TooManyRuleHits { .. } => "too_many_rule_hits",
            EvidenceNotFlagged { .. } => "evidence_not_flagged",
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CoreError::*;
        match self {
            EmptyFile => write!(formatter, "the file is empty"),
            NonAsciiDelimiter { delimiter } => write!(formatter, "delimiter {delimiter:?} must be an ASCII character"),
            NoTimestampInHead { lines } => write!(
                formatter,
                "no row with a recognizable timestamp in the first {lines} lines; set time_format or time_columns"
            ),
            SkipRowsPastEnd => write!(formatter, "skip_rows is past the end of the file"),
            FirstRowWithoutTimestamp => write!(formatter, "the first data row has no recognizable timestamp"),
            NoDataColumns => write!(formatter, "no data columns besides the timestamp"),
            TooFewTimestamps => write!(formatter, "need at least two rows with valid timestamps"),
            StepNotPositive { step_s } => write!(formatter, "step_s must be positive, got {step_s}"),
            IdenticalTimestamps => write!(formatter, "all timestamps are identical; cannot infer the time step"),
            ShapeTooFewVertices { vertices } => write!(formatter, "a selection shape needs at least 3 corners, got {vertices}"),
            AppendStepMismatch { master_s, appended_s } => {
                write!(formatter, "the file's time step is {appended_s} s and the dataset's {master_s} s")
            }
            AppendOutOfPhase { offset_s } => write!(formatter, "the file's time steps fall {offset_s} s into the dataset's"),
            AppendedBytesExceedBuffer { buffer } => write!(formatter, "the appended files are larger than the {buffer}-byte buffer"),
            TimestampsSpanTooMany { steps, step_s, rows } => write!(
                formatter,
                "timestamps span {steps} steps of {step_s} s for only {rows} rows; \
                 check the time format, the time step or outlier timestamps"
            ),
            TimeFormatNotRecognized { parsed, sampled } => write!(
                formatter,
                "timestamp format not recognized ({parsed} of {sampled} sampled rows parsed); set time_format"
            ),
            MalformedCsv { detail } => write!(formatter, "could not read the CSV: {detail}"),
            ElevationOutOfRange { elevation_m, min_m, max_m } => {
                write!(formatter, "site elevation {elevation_m} m is outside {min_m}..={max_m} m")
            }
            DecimalCommaWithCommaDelimiter => write!(formatter, "a decimal comma needs a delimiter other than ','"),
            InvalidTimeFormat { format } => write!(formatter, "invalid time format {format:?}"),
            TimestampOutOfRange { seconds } => write!(formatter, "timestamp {seconds} s is out of the calendar's range"),
            SiteLocationUnknown => write!(formatter, "the site's latitude and longitude are unknown; give them"),
            SiteLocationOutOfRange { latitude, longitude } => write!(
                formatter,
                "latitude {latitude} must be within ±90° and longitude {longitude} within ±180°"
            ),
            DisplacementHeight { height_m } => {
                write!(formatter, "displacement height {height_m} m must be at least 0, twelve when by month, and below the sensors")
            }
            EpeStationInvalid { station } => write!(formatter, "EPE station {station:?} must be its numeric code"),
            EpeNeedsTenMinutes { step_s } => write!(formatter, "EPE takes 10-minute intervals; this dataset steps {step_s} s"),
            UtcOffsetOutOfRange { minutes } => write!(formatter, "a clock offset of {minutes} min from UTC is outside UTC-12 to UTC+14"),
            HeightNotPositive { height_m } => write!(formatter, "height {height_m} m must be positive"),
            NotAProject => write!(formatter, "the file is not a project or is damaged"),
            NewerProjectFormat { found, supported } => write!(
                formatter,
                "the project was saved by a newer program (layout {found}; this one reads layout {supported})"
            ),
            NoSuchFlag { flag } => write!(formatter, "no flag {flag}"),
            FlagNameInvalid { name } => write!(formatter, "flag name {name:?} is empty or already taken"),
            NoSuchColumn { column } => write!(formatter, "no column {column}"),
            SegmentOutOfRange { start, end, len } => {
                write!(formatter, "segment {start}..{end} is empty or past the record's {len} steps")
            }
            UnknownColumn { id } => write!(formatter, "no column named {id:?}"),
            NoSuchDataset { dataset } => write!(formatter, "no dataset {dataset}"),
            DerivedColumnNotFlaggable { column } => {
                write!(formatter, "column {column} is derived: flag the columns it is computed from")
            }
            DerivedColumnNotEditable { column } => {
                write!(formatter, "column {column} is derived: edit the columns it is computed from")
            }
            ScaleOffsetNotFinite { scale, offset } => write!(formatter, "scale {scale} and offset {offset} must be finite numbers"),
            AveragingShorterThanStep { period_s, step_s } => {
                write!(formatter, "an averaging period of {period_s} s is shorter than the {step_s} s step")
            }
            TooFewConcurrentPeriods { found, needed } => write!(
                formatter,
                "{found} periods where the site and the reference both have data; the fit needs {needed}"
            ),
            McpMethodUnavailable => write!(formatter, "this MCP method could not be fitted to these data; choose another"),
            MarkovNeedsLinearColumn { column } => {
                write!(formatter, "column {column} is a direction: reconstruct it from another sensor, not by Markov")
            }
            TooFewPeaks { found, needed } => write!(
                formatter,
                "{found} peaks found; the Gumbel fit needs {needed} (longer record, lower recovery or a lower storm threshold)"
            ),
            GumbelNoSolution => write!(formatter, "the Gumbel fit has no solution: the peaks have no spread"),
            NoTurbulenceIntensity { column } => {
                write!(formatter, "column {column} has no turbulence intensity: associate its standard deviation column")
            }
            BinWidthNotPositive => write!(formatter, "bin width must be positive"),
            NoDataAfterFilter => write!(formatter, "no data left after filtering"),
            TooManyBins { bins, low, high, max } => write!(
                formatter,
                "{bins:.0} bins from {low} to {high} (limit {max}); use a wider bin or flag the outliers"
            ),
            SectorsOutOfRange { sectors, max } => write!(formatter, "sectors must be between 1 and {max}, got {sectors}"),
            CalmThresholdInvalid { calm_threshold } => write!(formatter, "the calm threshold must be a speed of 0 or more, got {calm_threshold}"),
            FilterCriterionInvalid { criterion, value } => write!(formatter, "filter criterion {criterion} has an invalid value {value}"),
            ColumnWithoutHeight { column } => write!(formatter, "column {column} has no height"),
            ShearNeedsTwoHeights => write!(formatter, "shear needs speed columns at two or more heights"),
            TooManyDivisions { divisions, max } => write!(formatter, "a shear display subdivides by at most {max} divisions, got {divisions}"),
            YearlyDivisionsInvalid { divisions } => write!(formatter, "yearly divisions must be 1, 2, 3, 4, 6 or 12, got {divisions}"),
            NoConcurrentStep => write!(formatter, "no time step where all selected columns have data"),
            TooFewSpeeds => write!(formatter, "need at least two speed values to fit a Weibull"),
            TooFewDistinctSpeeds => write!(formatter, "need at least two distinct positive speeds to fit a Weibull"),
            WeibullNoSolution { method } => write!(formatter, "the {method:?} Weibull fit has no solution for these speeds"),
            NonPositiveShape => write!(formatter, "the least-squares Weibull fit gave a non-positive shape k"),
            RuleRangeInvalid { min, max } => write!(formatter, "a range rule needs min < max, got {min} and {max}"),
            RuleThresholdInvalid { threshold } => {
                write!(formatter, "a rule threshold must be finite and not negative, got {threshold}")
            }
            RuleMinStepsTooSmall { min_steps, least } => {
                write!(formatter, "this rule needs at least {least} steps, got {min_steps}")
            }
            TooManyRules { rules, max } => write!(formatter, "{rules} rules in one run; the limit is {max}"),
            TooManyRuleHits { max } => write!(
                formatter,
                "the rules found more than {max} stretches; narrow the thresholds or raise the minimum duration"
            ),
            EvidenceNotFlagged { column, start, end } => {
                write!(formatter, "evidence for steps {start}..{end} of column {column} has no flag under it")
            }
        }
    }
}

impl std::error::Error for CoreError {}

/// Result type of every fallible core function.
pub type CoreResult<T> = Result<T, CoreError>;

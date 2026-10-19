//! Arquivo › Anexar dados on the wire: the appended files travel in the
//! import buffer after the dataset's first file, each named with its size,
//! and come back with what became of each ([`wind_core::append`]).
use crate::ErrorDto;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::append::{self, AppendOutcome};
use wind_core::error::{CoreError, CoreResult};
use wind_core::import::{decode_text, import_text, ImportConfig, ImportReport};
use wind_core::model::Dataset;

/// One appended file in the import buffer.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendedSourceDto {
    /// File name.
    pub name: String,
    /// Its byte length in the buffer.
    pub bytes: usize,
}

/// What became of one appended file.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppendStatusDto {
    /// File name.
    pub name: String,
    /// Its steps; 0 when it was refused.
    pub steps: usize,
    /// Columns it added to the dataset, by id.
    pub new_columns: Vec<String>,
    /// Its columns that went into a dataset column of another name, linked
    /// by type, subtype, units and height: [its id, the dataset's id].
    pub linked: Vec<[String; 2]>,
    /// Values it put where the dataset had none.
    pub filled: usize,
    /// Values it had where the dataset already had data (left as they were).
    pub overlapped: usize,
    /// Why it was refused; `null` when it was appended.
    pub error: Option<ErrorDto>,
}

impl AppendStatusDto {
    fn refused(name: String, error: &CoreError) -> Self {
        AppendStatusDto { name, steps: 0, new_columns: Vec::new(), linked: Vec::new(), filled: 0, overlapped: 0, error: Some(error.into()) }
    }
}

/// Reads `bytes` as the dataset's first file followed by `appended`, each
/// with `config`, and merges them: the first file's report comes back, and
/// the status of each appended file. A file that does not read or does not
/// fit is refused alone, as the original reports it per file.
///
/// # Errors
/// [`CoreError::AppendedBytesExceedBuffer`] when the sizes overrun `bytes`;
/// the first file's import error.
pub fn import_with_appended(
    name: &str,
    bytes: &[u8],
    appended: &[AppendedSourceDto],
    config: &ImportConfig,
) -> CoreResult<(Dataset, ImportReport, Vec<AppendStatusDto>)> {
    let appended_bytes = appended.iter().try_fold(0usize, |total, source| total.checked_add(source.bytes));
    let Some(first_length) = appended_bytes.and_then(|total| bytes.len().checked_sub(total)) else {
        return Err(CoreError::AppendedBytesExceedBuffer { buffer: bytes.len() });
    };
    let (first, mut rest) = bytes.split_at(first_length);
    let (master, report) = import_text(name, &decode_text(first), config)?;
    let mut statuses: Vec<Option<AppendStatusDto>> = Vec::with_capacity(appended.len());
    let mut readable = Vec::with_capacity(appended.len());
    for source in appended {
        let (chunk, tail) = rest.split_at(source.bytes);
        rest = tail;
        match import_text(&source.name, &decode_text(chunk), config) {
            Ok((dataset, _)) => {
                statuses.push(None);
                readable.push((statuses.len() - 1, dataset));
            }
            Err(error) => statuses.push(Some(AppendStatusDto::refused(source.name.clone(), &error))),
        }
    }
    let (positions, datasets): (Vec<usize>, Vec<Dataset>) = readable.into_iter().unzip();
    let (merged, outcomes) = append::merge(master, datasets);
    for (position, outcome) in positions.into_iter().zip(outcomes) {
        let name = appended[position].name.clone();
        statuses[position] = Some(match outcome {
            AppendOutcome::Appended { steps, new_columns, linked, filled, overlapped } => {
                let linked = linked.into_iter().map(|(appended, master)| [appended, master]).collect();
                AppendStatusDto { name, steps, new_columns, linked, filled, overlapped, error: None }
            }
            AppendOutcome::Refused(error) => AppendStatusDto::refused(name, &error),
        });
    }
    Ok((merged, report, statuses.into_iter().map(|status| status.expect("every file has a status")).collect()))
}

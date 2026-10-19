//! The project file: what reopens a session. It keeps each dataset's source
//! file whole, the import configuration that was applied and the flags, so a
//! reopened dataset can still be re-imported with new corrections, and the
//! file is a reproducible recipe: source + configuration + flags
//! (ARQUITETURA C3).
//!
//! Layout: [`PROJECT_MAGIC`]; the manifest's byte length as a little-endian
//! `u32`; the manifest ([`ProjectDto`]) as UTF-8 JSON; then each dataset's
//! source file, back to back in manifest order. The sources are written as
//! they are, so saving never reads them into memory (the browser concatenates
//! `Blob`s).
//!
//! Two readers implement this one definition: web/src/project.ts in the
//! browser and [`read_project`] here (CLI, server). Both check the same
//! framing; see [`read_project`] for the one deliberate difference.
use crate::requests::{ImportConfigDto, RestoreDto};
use crate::rules::FlagRuleDto;
use crate::{ColumnKind, ColumnSubtype};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use wind_core::error::{CoreError, CoreResult};

/// First bytes of every project file.
pub const PROJECT_MAGIC: &str = "WPRJ";
/// Version of the layout and manifest; readers refuse other versions.
pub const PROJECT_FORMAT: u32 = 1;
/// Bytes of the manifest's length, a little-endian `u32` right after the magic.
const MANIFEST_LENGTH_BYTES: usize = 4;
/// Manifest key holding [`PROJECT_FORMAT`]; read before the rest, so a newer
/// layout is reported as newer rather than as damaged.
const FORMAT_KEY: &str = "format";

/// The manifest of a project file.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectDto {
    /// [`PROJECT_FORMAT`] of the writer.
    pub format: u32,
    /// One entry per dataset, in the order their sources follow the manifest.
    pub datasets: Vec<ProjectDatasetDto>,
}

/// One dataset of a project.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectDatasetDto {
    /// File name of the source, shown as the dataset's name.
    pub source_name: String,
    /// Byte length of the source within the project file.
    pub source_bytes: usize,
    /// The configuration of the last successful import.
    pub config: ImportConfigDto,
    /// The flags, segments and rule evidence to put back.
    pub restore: RestoreDto,
    /// The flag rules defined for the dataset; absent in older projects.
    #[serde(default)]
    #[ts(optional)]
    pub rules: Option<Vec<FlagRuleDto>>,
    /// How the columns are shown; absent = as read.
    #[serde(default)]
    #[ts(optional)]
    pub display: Option<ColumnDisplayDto>,
    /// Files appended to the source (Arquivo › Anexar dados), in order;
    /// their bytes follow the source's. Absent = none.
    #[serde(default)]
    #[ts(optional)]
    pub appended: Option<Vec<crate::append::AppendedSourceDto>>,
}

/// How a dataset's columns are named, ordered, shown and coloured
/// (Configurar dataset › Exibição de colunas, ESPEC-INTERFACE §2.3). Only the
/// browser draws with it; requests always name columns by id.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColumnDisplayDto {
    /// The names lists and charts show.
    pub naming: ColumnNaming,
    /// Decimals of the height in a systematic name.
    pub height_decimals: u8,
    /// The order of lists and legends.
    pub order: ColumnOrder,
    /// Within a type (or overall, by height), the highest first.
    pub highest_first: bool,
    /// Types and statistics left out of lists and charts.
    pub hidden: Vec<HiddenColumnsDto>,
    /// The colours of the time series lines.
    pub colors: ColumnColors,
}

/// How columns are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnNaming {
    /// The id read from the file.
    File,
    /// Type, height and statistic, the same pattern for every file.
    Systematic,
}

/// How columns are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnOrder {
    /// As in the file.
    File,
    /// By type, then height, then statistic.
    TypeThenHeight,
    /// By height, then type, then statistic.
    HeightThenType,
}

/// One type and statistic hidden.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HiddenColumnsDto {
    /// The type.
    pub kind: ColumnKind,
    /// The statistic.
    pub subtype: ColumnSubtype,
}

/// How time series lines are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColumnColors {
    /// The chart's palette, in turn.
    Palette,
    /// A hue per type, darker the higher the sensor.
    ByType,
}

/// One dataset of a project file, its source still borrowed from the file.
#[derive(Debug, Clone)]
pub struct ProjectSource<'file> {
    /// File name of the source, shown as the dataset's name.
    pub name: String,
    /// The source file's bytes followed by its appended files', a slice of
    /// the project file (no copy).
    pub bytes: &'file [u8],
    /// The files appended to the source, sizing the rest of `bytes`.
    pub appended: Vec<crate::append::AppendedSourceDto>,
    /// The configuration of the last successful import.
    pub config: ImportConfigDto,
    /// The flags and segments to put back after re-importing.
    pub restore: RestoreDto,
}

/// Splits a project file into its datasets, checking what
/// web/src/project.ts checks: the magic, the manifest's length within the
/// file, a JSON object manifest of format [`PROJECT_FORMAT`], the shape of
/// every entry, every source within the file, and the sources filling the
/// file exactly. The file comes from disk, so nothing in it is trusted;
/// sizes are added with overflow checks.
///
/// One deliberate difference: the browser checks each entry's `config` and
/// `restore` only as objects and leaves their fields to the import, where a
/// bad entry fails that dataset alone. Here the manifest is typed whole, so
/// a bad entry fails the file: a command line run is all or nothing.
///
/// Cost: O(manifest); the sources are sliced, never copied.
///
/// # Errors
/// [`CoreError::NewerProjectFormat`] for a manifest whose format is newer
/// than [`PROJECT_FORMAT`]; [`CoreError::NotAProject`] for anything else
/// that does not fit the layout.
pub fn read_project(file: &[u8]) -> CoreResult<Vec<ProjectSource<'_>>> {
    let magic_end = PROJECT_MAGIC.len();
    let header_end = magic_end + MANIFEST_LENGTH_BYTES;
    if file.get(..magic_end) != Some(PROJECT_MAGIC.as_bytes()) {
        return Err(CoreError::NotAProject);
    }
    let length: [u8; MANIFEST_LENGTH_BYTES] =
        file.get(magic_end..header_end).and_then(|bytes| bytes.try_into().ok()).ok_or(CoreError::NotAProject)?;
    let manifest_end = usize::try_from(u32::from_le_bytes(length))
        .ok()
        .and_then(|manifest_bytes| header_end.checked_add(manifest_bytes))
        .filter(|&end| end <= file.len())
        .ok_or(CoreError::NotAProject)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&file[header_end..manifest_end]).map_err(|_| CoreError::NotAProject)?;
    let format = manifest.as_object().and_then(|fields| fields.get(FORMAT_KEY)).and_then(serde_json::Value::as_f64);
    match format {
        Some(found) if found == f64::from(PROJECT_FORMAT) => {}
        Some(found) if found > f64::from(PROJECT_FORMAT) => {
            return Err(CoreError::NewerProjectFormat { found, supported: PROJECT_FORMAT })
        }
        _ => return Err(CoreError::NotAProject),
    }
    let project: ProjectDto = serde_json::from_value(manifest).map_err(|_| CoreError::NotAProject)?;
    let mut offset = manifest_end;
    let mut sources = Vec::with_capacity(project.datasets.len());
    for saved in project.datasets {
        let appended = saved.appended.unwrap_or_default();
        let end = appended
            .iter()
            .try_fold(saved.source_bytes, |total, source| total.checked_add(source.bytes))
            .and_then(|bytes| offset.checked_add(bytes))
            .filter(|&end| end <= file.len())
            .ok_or(CoreError::NotAProject)?;
        sources.push(ProjectSource {
            name: saved.source_name,
            bytes: &file[offset..end],
            appended,
            config: saved.config,
            restore: saved.restore,
        });
        offset = end;
    }
    if offset != file.len() {
        return Err(CoreError::NotAProject);
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two small sources, as a browser would save them.
    const FIRST_SOURCE: &[u8] = b"t,ws\n2014-01-01 00:00,5\n2014-01-01 00:10,6\n";
    const SECOND_SOURCE: &[u8] = b"t,ws\n2014-01-01 00:00,7\n";
    /// A flag on steps 0..1 of `ws`.
    const RESTORE: &str = r##"{"flags": [{"name": "Gelo", "color": "#2a9fc0", "include": false}],
        "segments": [{"column": "ws", "flag": 0, "startStep": 0, "endStep": 1}]}"##;

    /// The manifest of a project holding `sources`, with `format` as given.
    fn manifest(format: &str, sources: &[&[u8]]) -> String {
        let entries: Vec<String> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                format!(
                    r#"{{"sourceName": "torre_{index}.csv", "sourceBytes": {}, "config": {{"columns": []}}, "restore": {RESTORE}}}"#,
                    source.len()
                )
            })
            .collect();
        format!(r#"{{"format": {format}, "datasets": [{}]}}"#, entries.join(","))
    }

    /// The layout written out by hand, independently of [`read_project`]:
    /// magic, manifest length (u32 LE), manifest, sources.
    fn framed(manifest: &str, sources: &[&[u8]]) -> Vec<u8> {
        let mut file = PROJECT_MAGIC.as_bytes().to_vec();
        file.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
        file.extend_from_slice(manifest.as_bytes());
        sources.iter().for_each(|source| file.extend_from_slice(source));
        file
    }

    fn project() -> Vec<u8> {
        let sources = [FIRST_SOURCE, SECOND_SOURCE];
        framed(&manifest(&PROJECT_FORMAT.to_string(), &sources), &sources)
    }

    #[test]
    fn sources_come_back_byte_for_byte_with_their_entries() {
        let file = project();
        let datasets = read_project(&file).unwrap();
        assert_eq!(datasets.len(), 2);
        assert_eq!((datasets[0].name.as_str(), datasets[0].bytes), ("torre_0.csv", FIRST_SOURCE));
        assert_eq!((datasets[1].name.as_str(), datasets[1].bytes), ("torre_1.csv", SECOND_SOURCE));
        assert_eq!(datasets[1].restore.segments[0].column, "ws");
    }

    #[test]
    fn damaged_files_are_refused() {
        let file = project();
        let with_trailing_bytes = [file.as_slice(), b"lixo"].concat();
        let mut wrong_magic = file.clone();
        wrong_magic[0] = b'X';
        let mut length_past_end = file.clone();
        length_past_end[PROJECT_MAGIC.len()..PROJECT_MAGIC.len() + MANIFEST_LENGTH_BYTES]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        let sources = [FIRST_SOURCE];
        let entry_without_restore = manifest("1", &sources).replace(&format!(r#", "restore": {RESTORE}"#), "");
        let damaged: [(&str, Vec<u8>); 9] = [
            ("empty", Vec::new()),
            ("magic only", PROJECT_MAGIC.as_bytes().to_vec()),
            ("truncated", file[..file.len() - 1].to_vec()),
            ("trailing bytes", with_trailing_bytes),
            ("wrong magic", wrong_magic),
            ("length past the end", length_past_end),
            ("null manifest", framed("null", &[])),
            ("format as text", framed(&manifest(r#""1""#, &sources), &sources)),
            ("entry without restore", framed(&entry_without_restore, &sources)),
        ];
        for (case, bytes) in damaged {
            assert_eq!(read_project(&bytes).unwrap_err(), CoreError::NotAProject, "{case}");
        }
    }

    #[test]
    fn a_newer_layout_is_reported_as_newer() {
        let newer = f64::from(PROJECT_FORMAT + 1);
        let file = framed(&manifest(&newer.to_string(), &[]), &[]);
        assert_eq!(
            read_project(&file).unwrap_err(),
            CoreError::NewerProjectFormat { found: newer, supported: PROJECT_FORMAT }
        );
    }
}

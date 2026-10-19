/**
 * Project files: every dataset's source file, the import configuration last
 * applied to it and its flags, so a session can be closed and reopened. The
 * layout and the manifest are defined in crates/wind-api/src/project.rs.
 * Saving concatenates `Blob`s and reading slices them, so neither copies the
 * source files into memory.
 */
import type { ColumnDisplayDto } from './api/generated/ColumnDisplayDto';
import type { CriterionDto } from './api/generated/CriterionDto';
import { isDisplay } from './display';
import type { FlagRuleDto } from './api/generated/FlagRuleDto';
import type { ImportConfigDto } from './api/generated/ImportConfigDto';
import type { ProjectDto } from './api/generated/ProjectDto';
import type { RestoreDto } from './api/generated/RestoreDto';
import { PROJECT_FORMAT, PROJECT_MAGIC } from './api/generated/version';

/** File name extension of projects. */
export const PROJECT_EXTENSION = '.wproj';
/** The manifest's byte length is a little-endian u32. */
const LENGTH_BYTES = 4;
const LITTLE_ENDIAN = true;
const MAGIC_BYTES = new TextEncoder().encode(PROJECT_MAGIC).length;

/** Tests a saved flag rule may use. */
const CRITERION_KINDS: ReadonlySet<unknown> = new Set<CriterionDto['kind']>(['outside_range', 'change_above', 'stuck', 'twin_drift', 'clock_shift', 'direction_in_sector']);

const NOT_A_PROJECT = 'o arquivo não é um projeto ou está corrompido';
const NEWER_FORMAT = 'o projeto foi salvo numa versão mais nova do programa';

/** One dataset of a project: its source file and what to apply to it. */
export interface SavedDataset {
  file: File;
  /** Files appended to `file`, in order; their bytes follow its bytes. */
  appended?: File[];
  /** The configuration of the last successful import. */
  config: ImportConfigDto;
  restore: RestoreDto;
  /** Flag rules defined for the dataset; absent in older projects. */
  rules?: FlagRuleDto[];
  /** How its columns are shown; absent in older projects. */
  display?: ColumnDisplayDto;
}

/** The project file for `datasets`. */
export function writeProject(datasets: SavedDataset[]): Blob {
  const manifest: ProjectDto = {
    format: PROJECT_FORMAT,
    datasets: datasets.map(({ file, appended = [], config, restore, rules, display }) => ({
      sourceName: file.name,
      sourceBytes: file.size,
      config,
      restore,
      rules,
      display,
      ...(appended.length ? { appended: appended.map((part) => ({ name: part.name, bytes: part.size })) } : {}),
    })),
  };
  const json = new TextEncoder().encode(JSON.stringify(manifest));
  const length = new DataView(new ArrayBuffer(LENGTH_BYTES));
  length.setUint32(0, json.length, LITTLE_ENDIAN);
  return new Blob([PROJECT_MAGIC, length, json, ...datasets.flatMap((dataset) => [dataset.file, ...(dataset.appended ?? [])])]);
}

/** The datasets of a project file. The configurations and flags are checked
 *  by the core when each dataset is imported; this checks the framing. */
export async function readProject(project: Blob): Promise<SavedDataset[]> {
  const headerBytes = MAGIC_BYTES + LENGTH_BYTES;
  if (project.size < headerBytes) throw new Error(NOT_A_PROJECT);
  const header = await project.slice(0, headerBytes).arrayBuffer();
  if (new TextDecoder().decode(header.slice(0, MAGIC_BYTES)) !== PROJECT_MAGIC) throw new Error(NOT_A_PROJECT);
  const manifestBytes = new DataView(header).getUint32(MAGIC_BYTES, LITTLE_ENDIAN);
  let offset = headerBytes + manifestBytes;
  if (offset > project.size) throw new Error(NOT_A_PROJECT);

  let manifest: ProjectDto;
  try {
    manifest = JSON.parse(await project.slice(headerBytes, offset).text()) as ProjectDto;
  } catch {
    throw new Error(NOT_A_PROJECT);
  }
  if (!isObject(manifest)) throw new Error(NOT_A_PROJECT);
  if (manifest.format !== PROJECT_FORMAT) throw new Error(manifest.format > PROJECT_FORMAT ? NEWER_FORMAT : NOT_A_PROJECT);
  if (!Array.isArray(manifest.datasets)) throw new Error(NOT_A_PROJECT);

  const datasets = manifest.datasets.map((saved) => {
    // The core checks every value; this keeps a damaged file from failing
    // later with a cryptic message, or loading without its flags.
    const shaped =
      isObject(saved) &&
      typeof saved.sourceName === 'string' &&
      isObject(saved.config) &&
      Array.isArray(saved.config.columns) &&
      isObject(saved.restore) &&
      Array.isArray(saved.restore.flags) &&
      Array.isArray(saved.restore.segments) &&
      (saved.restore.evidence === undefined || Array.isArray(saved.restore.evidence)) &&
      (saved.restore.edits === undefined || Array.isArray(saved.restore.edits)) &&
      (saved.rules === undefined || (Array.isArray(saved.rules) && saved.rules.every(isRule))) &&
      (saved.appended === undefined ||
        (Array.isArray(saved.appended) && saved.appended.every((part) => isObject(part) && typeof part.name === 'string' && Number.isSafeInteger(part.bytes) && part.bytes >= 0)));
    const { sourceBytes } = saved;
    if (!shaped || !Number.isSafeInteger(sourceBytes) || sourceBytes < 0 || offset + sourceBytes > project.size) {
      throw new Error(NOT_A_PROJECT);
    }
    const file = new File([project.slice(offset, offset + sourceBytes)], saved.sourceName);
    offset += sourceBytes;
    const appended = (saved.appended ?? []).map((part) => {
      if (offset + part.bytes > project.size) throw new Error(NOT_A_PROJECT);
      const appendedFile = new File([project.slice(offset, offset + part.bytes)], part.name);
      offset += part.bytes;
      return appendedFile;
    });
    // Display settings only change the drawing: damaged ones fall back to the columns as read.
    const display = isDisplay(saved.display) ? saved.display : undefined;
    return { file, appended, config: saved.config, restore: saved.restore, rules: saved.rules, display };
  });
  if (offset !== project.size) throw new Error(NOT_A_PROJECT);
  return datasets;
}

/** A flag rule with the shape the page shows; the core checks its values when it runs. */
function isRule(value: unknown): boolean {
  return (
    isObject(value) &&
    isObject(value.criterion) &&
    CRITERION_KINDS.has(value.criterion.kind) &&
    Array.isArray(value.columns) &&
    value.columns.every((column) => typeof column === 'string') &&
    Number.isSafeInteger(value.flag) &&
    Number.isSafeInteger(value.minSteps)
  );
}

/** A JSON object (not null, not an array). */
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * The workbook as the page keeps it: one entry per dataset, with what the
 * core answered and what only the page needs (colour, visibility, pending
 * corrections, plotted columns, filter panel). Pure helpers only; the
 * actions live in useWorkbook.ts.
 */
import type { ColumnCorrectionDto } from '../api/generated/ColumnCorrectionDto';
import type { ColumnDisplayDto } from '../api/generated/ColumnDisplayDto';
import type { ColumnKind } from '../api/generated/ColumnKind';
import type { ColumnDto } from '../api/generated/ColumnDto';
import type { DatasetDto } from '../api/generated/DatasetDto';
import type { FlagRuleDto } from '../api/generated/FlagRuleDto';
import type { ImportConfigDto } from '../api/generated/ImportConfigDto';
import type { ImportResultDto } from '../api/generated/ImportResultDto';
import type { RestoreDto } from '../api/generated/RestoreDto';
import type { TimestampMeaning } from '../api/generated/TimestampMeaning';
import type { FilterState } from '../FilterBy';

/** One dataset of the workbook. */
export interface Entry {
  file: File;
  /** Files appended to `file` (Arquivo › Anexar dados), in order; every import reads them all. */
  appended: File[];
  result: ImportResultDto;
  /** The configuration of the last successful import; saved with the project. */
  config: ImportConfigDto;
  /** Pending corrections, by file column index. */
  corrections: Record<number, ColumnCorrectionDto>;
  /** Corrections changed since the last import. */
  dirty: boolean;
  /** Ids of the columns shown in the time-series windows. */
  plotted: string[];
  /** Whether the core has a flag edit to undo / an undone one to redo. */
  canUndo: boolean;
  canRedo: boolean;
  /** Flag rules defined for the dataset; saved with the project. */
  rules: FlagRuleDto[];
  /** The analyses' "Filtrar por" panel, kept across windows. */
  filter: FilterState;
  /** Colour the dataset is drawn with everywhere (explorer, map, summary). */
  color: string;
  /** Shown in the summary graphs ("visível" in the explorer). */
  visible: boolean;
  /** How its columns are named, ordered, shown and coloured; saved with the project. */
  display: ColumnDisplayDto;
}

/** A file to import, with what a project saved for it. */
export interface ImportJob {
  file: File;
  /** Files appended to it, in order. */
  appended?: File[];
  config: ImportConfigDto;
  restore?: RestoreDto;
  rules?: FlagRuleDto[];
  display?: ColumnDisplayDto;
}

/** The import options of the Configure-dataset dialog; empty strings mean "detect". */
export interface ImportOptions {
  delimiter: string;
  decimal: '' | 'dot' | 'comma';
  timestampMeaning: TimestampMeaning;
  timeFormat: string;
  /** Site elevation above sea level, m; empty = unknown. */
  elevationM: string;
  /** Site coordinates, degrees; empty = from the logger preamble. */
  latitude: string;
  longitude: string;
}

export const DEFAULT_IMPORT_OPTIONS: ImportOptions = {
  delimiter: '',
  decimal: '',
  timestampMeaning: 'start',
  timeFormat: '',
  elevationM: '',
  latitude: '',
  longitude: '',
};

const MS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const SECONDS_PER_DAY = 86_400;
const DAYS_PER_YEAR = 365.25;
const MINUTES_PER_HOUR = 60;

/** An optional number typed in a field: empty = none, decimal comma accepted. */
export const typedNumber = (typed: string) => (typed.trim() === '' ? undefined : Number(typed.replace(',', '.')));

/** The configuration an import sends, from the dialog's options and the column corrections. */
export function importConfig(options: ImportOptions, corrections: Record<number, ColumnCorrectionDto>): ImportConfigDto {
  return {
    delimiter: options.delimiter || undefined,
    decimalComma: options.decimal === '' ? undefined : options.decimal === 'comma',
    timestampMeaning: options.timestampMeaning,
    timeFormat: options.timeFormat.trim() || undefined,
    columns: Object.values(corrections),
    elevationM: typedNumber(options.elevationM),
    latitude: typedNumber(options.latitude),
    longitude: typedNumber(options.longitude),
  };
}

/** Decimals a coordinate shows in a field: 10⁻⁶° is about 0.1 m. */
const SITE_DECIMALS = 6;
/** A coordinate as a field shows it: −25.381667, not −25.381666666666668. */
export const coordinateText = (degrees: number | null | undefined) => (degrees == null ? '' : String(Number(degrees.toFixed(SITE_DECIMALS))));

/** The dialog's view of an import configuration. */
export function optionsOf(config: ImportConfigDto): ImportOptions {
  const text = (value: number | undefined) => (value == null ? '' : String(value));
  return {
    delimiter: config.delimiter ?? '',
    decimal: config.decimalComma === undefined ? '' : config.decimalComma ? 'comma' : 'dot',
    timestampMeaning: config.timestampMeaning ?? DEFAULT_IMPORT_OPTIONS.timestampMeaning,
    timeFormat: config.timeFormat ?? '',
    elevationM: text(config.elevationM),
    latitude: text(config.latitude),
    longitude: text(config.longitude),
  };
}

export const isMeanSpeed = (column: ColumnDto) => column.kind === 'wind_speed' && column.subtype === 'mean';
export const isMeanDirection = (column: ColumnDto) => column.kind === 'wind_direction' && column.subtype === 'mean';
/** TI, WPD and air density: computed by the core from the measured columns. */
export const isDerived = (column: ColumnDto) => column.derivedFrom !== undefined;

/** The boom orientation of each column that has one (Configurar dataset ›
 *  Lança), degrees from north, by column id. */
export function boomsOf(entry: Entry): Record<string, number> {
  const { dataset, report } = entry.result;
  return Object.fromEntries(
    dataset.columns.flatMap((column, position) => {
      const boom = entry.corrections[report.fileColumnByColumn[position]]?.boomDeg;
      return boom == null ? [] : [[column.id, boom]];
    }),
  );
}

/** The dataset's primary speed, direction and temperature (ESPEC-INTERFACE
 *  §2.3, "Colunas primárias"), as the core picked them from the dataset's
 *  configuration: by default the highest, and the direction nearest the speed. */
export function primaryColumns(dataset: DatasetDto): { speed?: ColumnDto; direction?: ColumnDto; temperature?: ColumnDto } {
  const of = (kind: ColumnKind) => {
    const id = dataset.primary.find((primary) => primary.kind === kind)?.column;
    return dataset.columns.find((column) => column.id === id);
  };
  return { speed: of('wind_speed'), direction: of('wind_direction'), temperature: of('temperature') };
}

/** The first mean wind speed, else the first column: what a time series starts with. */
export function defaultPlot(dataset: DatasetDto): string[] {
  const first = primaryColumns(dataset).speed ?? dataset.columns[0];
  return first ? [first.id] : [];
}

export const endMs = (dataset: DatasetDto) => dataset.startMs + dataset.stepCount * dataset.stepS * MS_PER_SECOND;
export const lastStepMs = (dataset: DatasetDto) => dataset.startMs + (dataset.stepCount - 1) * dataset.stepS * MS_PER_SECOND;
export const recordDays = (dataset: DatasetDto) => (dataset.stepCount * dataset.stepS) / SECONDS_PER_DAY;
export const recordYears = (dataset: DatasetDto) => recordDays(dataset) / DAYS_PER_YEAR;

export const formatDay = (epochMs: number) => new Date(epochMs).toISOString().slice(0, 10);
export const formatDateTime = (epochMs: number) => new Date(epochMs).toISOString().slice(0, 16).replace('T', ' ');
export const formatStep = (seconds: number) =>
  seconds % SECONDS_PER_HOUR === 0
    ? `${seconds / SECONDS_PER_HOUR} h`
    : seconds % SECONDS_PER_MINUTE === 0
      ? `${seconds / SECONDS_PER_MINUTE} min`
      : `${seconds} s`;
/** "UTC-03" style, "—" when the clock's zone is unknown. */
export function formatUtcOffset(minutes: number | null | undefined) {
  if (minutes == null) return '—';
  const sign = minutes < 0 ? '-' : '+';
  const hours = Math.floor(Math.abs(minutes) / MINUTES_PER_HOUR);
  const rest = Math.abs(minutes) % MINUTES_PER_HOUR;
  return `UTC${sign}${String(hours).padStart(2, '0')}${rest ? `:${String(rest).padStart(2, '0')}` : ''}`;
}
export const baseName = (file: File) => file.name.replace(/\.[^.]+$/, '');

/** Files in name order: a logger names its daily files by date. */
export const byName = (files: Iterable<File>) => Array.from(files).sort((left, right) => left.name.localeCompare(right.name));

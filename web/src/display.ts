/**
 * Column display settings (Configurar dataset › Exibição de colunas,
 * ESPEC-INTERFACE §2.3): how each dataset's columns are named, ordered, shown
 * and coloured in lists, legends and the time series. Display only: requests
 * name columns by id. Each dataset keeps its own (saved in the project); new
 * datasets take the viewer's default, kept in this browser.
 */
import { createContext, useContext } from 'react';
import type { ColumnDisplayDto } from './api/generated/ColumnDisplayDto';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { ColumnKind } from './api/generated/ColumnKind';
import type { ColumnSubtype } from './api/generated/ColumnSubtype';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { DatasetId } from './api/generated/DatasetId';
import { COLUMN_KIND_LABEL } from './labels';

/** Columns as read: the file's ids, its order, all shown, the chart's palette. */
export const DEFAULT_DISPLAY: ColumnDisplayDto = { naming: 'file', heightDecimals: 0, order: 'file', highestFirst: true, hidden: [], colors: 'palette' };
/** Where this browser keeps the default for new datasets. */
const DEFAULT_KEY = 'openwindaxis.columnDisplay';

/** Short tags of a systematic name, per type. */
const KIND_TAG: Record<ColumnKind, string> = {
  wind_speed: 'Vel',
  vertical_wind_speed: 'VelV',
  wind_direction: 'Dir',
  temperature: 'Temp',
  pressure: 'Pres',
  relative_humidity: 'UR',
  solar_radiation: 'Rad',
  battery_voltage: 'Bat',
  turbine_output: 'Pot',
  quality: 'Qual',
  other: 'Outro',
  turbulence_intensity: 'TI',
  power_density: 'DPot',
  air_density: 'DAr',
};
export const SUBTYPE_TAG: Record<ColumnSubtype, string> = { mean: 'Média', std_dev: 'DP', min: 'Mín', max: 'Máx', gust: 'Raj' };
/** Type order of the ordered lists: as the labels list them. */
const KIND_RANK = new Map(Object.keys(COLUMN_KIND_LABEL).map((kind, rank) => [kind, rank]));
const SUBTYPE_RANK = new Map((['mean', 'std_dev', 'min', 'max', 'gust'] as ColumnSubtype[]).map((subtype, rank) => [subtype, rank]));
/** Hue of each type when colouring by type, degrees. */
const KIND_HUE: Partial<Record<ColumnKind, number>> = {
  wind_speed: 220,
  vertical_wind_speed: 260,
  wind_direction: 140,
  temperature: 10,
  pressure: 290,
  relative_humidity: 190,
  solar_radiation: 45,
  turbulence_intensity: 330,
  power_density: 25,
  air_density: 170,
};
const OTHER_HUE = 0;
const COLOR_SATURATION = 65;
/** Lightness of the highest and the lowest sensor of a type, %. */
const DARKEST = 30;
const LIGHTEST = 65;

/** The view of a dataset's columns its display settings give. */
export interface ColumnView {
  /** The name to show. */
  name: (column: ColumnDto) => string;
  /** `columns` (all by default) without the hidden ones, in display order. */
  shown: (columns?: ColumnDto[]) => ColumnDto[];
  /** Whether a column is shown. */
  visible: (column: ColumnDto) => boolean;
  /** The line colour, or `undefined` for the chart's palette. */
  color: (column: ColumnDto) => string | undefined;
}

/** `column`'s name under `display`; derived columns keep their id, which already says what they are. */
function nameOf(column: ColumnDto, display: ColumnDisplayDto): string {
  if (display.naming === 'file' || column.derivedFrom) return column.id;
  const height = column.heightM == null ? '' : ` ${column.heightM.toFixed(display.heightDecimals)}m`;
  return `${KIND_TAG[column.kind]}${height} ${SUBTYPE_TAG[column.subtype]}`;
}

/** The view `display` gives of `dataset`'s columns. Two columns that would
 *  share a systematic name keep their ids after it. */
export function columnView(dataset: DatasetDto, display: ColumnDisplayDto): ColumnView {
  const position = new Map(dataset.columns.map((column, index) => [column.id, index]));
  const names = new Map(dataset.columns.map((column) => [column.id, nameOf(column, display)]));
  const taken = new Map<string, number>();
  names.forEach((name) => taken.set(name, (taken.get(name) ?? 0) + 1));
  const hidden = new Set(display.hidden.map(({ kind, subtype }) => `${kind}/${subtype}`));
  const visible = (column: ColumnDto) => !hidden.has(`${column.kind}/${column.subtype}`);
  const sign = display.highestFirst ? -1 : 1;
  // Columns without a height come after those with one, either way.
  const byHeight = (first: ColumnDto, second: ColumnDto) =>
    first.heightM == null || second.heightM == null ? Number(first.heightM == null) - Number(second.heightM == null) : sign * (first.heightM - second.heightM);
  const byKind = (first: ColumnDto, second: ColumnDto) => KIND_RANK.get(first.kind)! - KIND_RANK.get(second.kind)!;
  const bySubtype = (first: ColumnDto, second: ColumnDto) => SUBTYPE_RANK.get(first.subtype)! - SUBTYPE_RANK.get(second.subtype)!;
  const byFile = (first: ColumnDto, second: ColumnDto) => position.get(first.id)! - position.get(second.id)!;
  const keys = display.order === 'type_then_height' ? [byKind, byHeight, bySubtype, byFile] : display.order === 'height_then_type' ? [byHeight, byKind, bySubtype, byFile] : [byFile];
  const compare = (first: ColumnDto, second: ColumnDto) => keys.reduce((order, key) => order || key(first, second), 0);
  // Lightness by the height's rank among the type's sensors, highest darkest.
  const color = (column: ColumnDto) => {
    if (display.colors === 'palette') return undefined;
    const heights = [...new Set(dataset.columns.filter((other) => other.kind === column.kind && other.heightM != null).map((other) => other.heightM!))].sort((a, b) => b - a);
    const rank = column.heightM == null ? heights.length : heights.indexOf(column.heightM);
    const share = heights.length > 1 ? Math.min(1, rank / (heights.length - 1)) : 0;
    return `hsl(${KIND_HUE[column.kind] ?? OTHER_HUE}, ${COLOR_SATURATION}%, ${DARKEST + share * (LIGHTEST - DARKEST)}%)`;
  };
  return {
    name: (column) => {
      const name = names.get(column.id) ?? column.id;
      return (taken.get(name) ?? 0) > 1 && name !== column.id ? `${name} (${column.id})` : name;
    },
    shown: (columns = dataset.columns) => columns.filter(visible).sort(compare),
    visible,
    color,
  };
}

/** Each dataset's display settings, for the components that list columns. */
export const DisplayContext = createContext<ReadonlyMap<DatasetId, ColumnDisplayDto>>(new Map());

/** The view of `dataset`'s columns under its display settings. */
export const useColumnView = (dataset: DatasetDto) => columnView(dataset, useContext(DisplayContext).get(dataset.id) ?? DEFAULT_DISPLAY);

/** The default display for new datasets this browser keeps; as read when none
 *  (or when the browser keeps nothing). */
export function storedDefaultDisplay(): ColumnDisplayDto {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(DEFAULT_KEY) ?? 'null');
    return isDisplay(saved) ? saved : DEFAULT_DISPLAY;
  } catch {
    return DEFAULT_DISPLAY;
  }
}

const NAMINGS: ReadonlySet<unknown> = new Set<ColumnDisplayDto['naming']>(['file', 'systematic']);
const ORDERS: ReadonlySet<unknown> = new Set<ColumnDisplayDto['order']>(['file', 'type_then_height', 'height_then_type']);
const COLORINGS: ReadonlySet<unknown> = new Set<ColumnDisplayDto['colors']>(['palette', 'by_type']);
/** Most decimals a systematic name gives a height. */
export const MAX_HEIGHT_DECIMALS = 2;

/** Whether a saved value is display settings this version draws with; the core never reads them. */
export function isDisplay(value: unknown): value is ColumnDisplayDto {
  if (typeof value !== 'object' || value === null) return false;
  const display = value as Record<string, unknown>;
  const decimals = display.heightDecimals;
  return (
    NAMINGS.has(display.naming) &&
    ORDERS.has(display.order) &&
    COLORINGS.has(display.colors) &&
    typeof display.highestFirst === 'boolean' &&
    Number.isInteger(decimals) &&
    (decimals as number) >= 0 &&
    (decimals as number) <= MAX_HEIGHT_DECIMALS &&
    Array.isArray(display.hidden) &&
    display.hidden.every((hidden) => typeof hidden === 'object' && hidden !== null && 'kind' in hidden && 'subtype' in hidden)
  );
}

/** Keeps `display` as this browser's default for new datasets; false when the browser keeps nothing. */
export function storeDefaultDisplay(display: ColumnDisplayDto): boolean {
  try {
    localStorage.setItem(DEFAULT_KEY, JSON.stringify(display));
    return true;
  } catch {
    return false;
  }
}


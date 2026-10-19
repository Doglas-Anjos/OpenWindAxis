/**
 * The "Filtrar por" group of the analysis windows (ESPEC-INTERFACE §3.2,
 * tips in §6.1): one row per criterion, each switched on by its own box, the
 * row's controls disabled while it is off. A step is read only when it meets
 * every active criterion. The state lives per dataset in App, so switching
 * tabs keeps it; {@link filterRequest} turns it into the `filter` of a request.
 */
import type { AnalysisKind } from './Analyses';
import { columnLabel, isMeanDirection, sectorLabels, withUnits } from './Analyses';
import { useColumnView } from './display';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { FilterDto } from './api/generated/FilterDto';
import { MONTH_ABBREVIATIONS } from './labels';

const MS_PER_SECOND = 1000;
const MS_PER_DAY = 86_400_000;
const HOURS_PER_DAY = 24;
const FIRST_MONTH = 1;
/** Sector counts offered, and the default (16 in the reference screens). */
const SECTOR_CHOICES = [4, 8, 12, 16, 36];
const DEFAULT_SECTORS = 16;
/** `AAAA-MM-DD` of an instant, the value of a date input. */
const isoDay = (epochMs: number) => new Date(epochMs).toISOString().slice(0, 10);
/** Midnight UTC of a date input's value. */
const dayStartMs = (day: string) => Date.parse(`${day}T00:00:00Z`);
const hourLabel = (hour: number) => `${String(hour).padStart(2, '0')}:00`;

/** What the panel edits; every row keeps its values while switched off. */
export interface FilterState {
  /** The flag row is on by default: it is default flag filtering. Off, flags are ignored. */
  flagsOn: boolean;
  /** Per flag, keep its data; `null` = each flag's "include by default". */
  keep: boolean[] | null;
  keepUnflagged: boolean;
  dateOn: boolean;
  /** First and last day, `AAAA-MM-DD`, both included. */
  fromDay: string;
  toDay: string;
  monthsOn: boolean;
  firstMonth: number;
  lastMonth: number;
  hoursOn: boolean;
  startHour: number;
  endHour: number;
  sectorOn: boolean;
  direction: string | null;
  sectors: number;
  sector: number;
  valueOn: boolean;
  column: string | null;
  /** Bounds as typed; empty = none. */
  min: string;
  max: string;
}

/** The panel's starting state for `dataset`: only default flag filtering. */
export function initialFilter(dataset: DatasetDto): FilterState {
  const lastStepMs = dataset.startMs + (dataset.stepCount - 1) * dataset.stepS * MS_PER_SECOND;
  return {
    flagsOn: true,
    keep: null,
    keepUnflagged: true,
    dateOn: false,
    fromDay: isoDay(dataset.startMs),
    toDay: isoDay(lastStepMs),
    monthsOn: false,
    firstMonth: FIRST_MONTH,
    lastMonth: MONTH_ABBREVIATIONS.length,
    hoursOn: false,
    startHour: 0,
    endHour: HOURS_PER_DAY,
    sectorOn: false,
    direction: dataset.columns.find(isMeanDirection)?.id ?? null,
    sectors: DEFAULT_SECTORS,
    sector: 0,
    valueOn: false,
    column: dataset.columns[0]?.id ?? null,
    min: '',
    max: '',
  };
}

/** The flags the window keeps: its own choice while the dataset has the same
 *  flags, else (flags added or deleted since) each flag's default treatment. */
const keptFlags = (state: FilterState, dataset: DatasetDto) =>
  state.keep?.length === dataset.flags.length ? state.keep : dataset.flags.map((flag) => flag.include);

/** Whether `kind` offers the sector row: the wind rose already splits by sector. */
const hasSectorRow = (kind: AnalysisKind) => kind !== 'rose';

/** The `filter` of a request, or `undefined` when only default flag filtering is on. */
export function filterRequest(state: FilterState, dataset: DatasetDto, kind: AnalysisKind): FilterDto | undefined {
  const filter: FilterDto = {};
  const defaults = dataset.flags.map((flag) => flag.include);
  const keep = keptFlags(state, dataset);
  if (!state.flagsOn) {
    filter.flags = { keep: dataset.flags.map(() => true), keepUnflagged: true };
  } else if (!state.keepUnflagged || keep.some((kept, flag) => kept !== defaults[flag])) {
    filter.flags = { keep, keepUnflagged: state.keepUnflagged };
  }
  if (state.dateOn && state.fromDay && state.toDay) {
    filter.dateRange = { fromMs: dayStartMs(state.fromDay), toMs: dayStartMs(state.toDay) + MS_PER_DAY };
  }
  if (state.monthsOn) filter.months = { first: state.firstMonth, last: state.lastMonth };
  if (state.hoursOn) filter.hours = { start: state.startHour, end: state.endHour };
  if (state.sectorOn && state.direction && hasSectorRow(kind)) {
    filter.sector = { direction: state.direction, sectors: state.sectors, sector: state.sector };
  }
  if (state.valueOn && state.column && (state.min.trim() || state.max.trim())) {
    const bound = (typed: string) => (typed.trim() === '' ? null : Number(typed.replace(',', '.')));
    filter.value = { column: state.column, min: bound(state.min), max: bound(state.max) };
  }
  return Object.keys(filter).length ? filter : undefined;
}

/** The years the record touches, for the "ano AAAA" shortcut. */
function recordYears(dataset: DatasetDto): number[] {
  const first = new Date(dataset.startMs).getUTCFullYear();
  // The year of the last step's start: a record ending at midnight on 1 January has no step in that year.
  const last = new Date(dataset.startMs + (dataset.stepCount - 1) * dataset.stepS * MS_PER_SECOND).getUTCFullYear();
  return Array.from({ length: last - first + 1 }, (_, index) => first + index);
}

export function FilterPanel(props: {
  dataset: DatasetDto;
  kind: AnalysisKind;
  state: FilterState;
  onChange: (state: FilterState) => void;
}) {
  const { dataset, kind, state, onChange } = props;
  const set = (patch: Partial<FilterState>) => onChange({ ...state, ...patch });
  const view = useColumnView(dataset);
  const keep = keptFlags(state, dataset);
  const units = dataset.columns.find((column) => column.id === state.column)?.units;
  const row = (on: boolean, toggle: (on: boolean) => void, label: string, title: string, controls: JSX.Element) => (
    <div className={`criterion ${on ? '' : 'off'}`} title={title}>
      <label className="switch">
        <input type="checkbox" checked={on} onChange={(event) => toggle(event.target.checked)} />
        {label}
      </label>
      <fieldset disabled={!on}>{controls}</fieldset>
    </div>
  );
  return (
    <details className="opts filterby" open>
      <summary>Filtrar por</summary>
      {row(
        state.flagsOn,
        (flagsOn) => set({ flagsOn }),
        'Flag',
        'Marque as categorias de flag que devem entrar no cálculo; se um ponto tiver uma flag desmarcada, ele sai mesmo que tenha outra marcada.',
        <span className="choices">
          <label title="Inclui os pontos que nunca receberam flag; desmarque só para analisar exclusivamente dados marcados.">
            <input type="checkbox" checked={state.keepUnflagged} onChange={(event) => set({ keepUnflagged: event.target.checked })} />
            &lt;Dados sem flag&gt;
          </label>
          {dataset.flags.map((flag, index) => (
            <label key={index}>
              <input
                type="checkbox"
                checked={keep[index]}
                onChange={(event) => set({ keep: keep.map((kept, other) => (other === index ? event.target.checked : kept)) })}
              />
              {flag.name}
            </label>
          ))}
        </span>,
      )}
      {row(
        state.dateOn,
        (dateOn) => set({ dateOn }),
        'Intervalo de datas',
        'Restringe a análise a um período, da meia-noite do primeiro dia à meia-noite depois do último.',
        <span className="choices">
          <select
            aria-label="Ano"
            value=""
            onChange={(event) => event.target.value && set({ fromDay: `${event.target.value}-01-01`, toDay: `${event.target.value}-12-31` })}
          >
            <option value="">Específico</option>
            {recordYears(dataset).map((year) => (
              <option key={year} value={year}>
                ano {year}
              </option>
            ))}
          </select>
          <input type="date" aria-label="De" value={state.fromDay} onChange={(event) => set({ fromDay: event.target.value })} />a
          <input type="date" aria-label="Até" value={state.toDay} onChange={(event) => set({ toDay: event.target.value })} />
        </span>,
      )}
      {row(
        state.monthsOn,
        (monthsOn) => set({ monthsOn }),
        'Mês',
        'Restringe a meses (aceita faixas que cruzam dezembro, como Dez–Fev), útil para estudar estações.',
        <span className="choices">
          {(['firstMonth', 'lastMonth'] as const).map((field, position) => (
            <select key={field} aria-label={position ? 'Mês final' : 'Mês inicial'} value={state[field]} onChange={(event) => set({ [field]: Number(event.target.value) })}>
              {MONTH_ABBREVIATIONS.map((name, index) => (
                <option key={name} value={index + FIRST_MONTH}>
                  {name}
                </option>
              ))}
            </select>
          ))}
        </span>,
      )}
      {row(
        state.hoursOn,
        (hoursOn) => set({ hoursOn }),
        'Hora',
        'Restringe a horas do dia (aceita faixas que cruzam a meia-noite, como 20:00–05:00), útil para separar noite e dia.',
        <span className="choices">
          <select aria-label="Hora inicial" value={state.startHour} onChange={(event) => set({ startHour: Number(event.target.value) })}>
            {Array.from({ length: HOURS_PER_DAY }, (_, hour) => (
              <option key={hour} value={hour}>
                {hourLabel(hour)}
              </option>
            ))}
          </select>
          a
          <select aria-label="Hora final" value={state.endHour} onChange={(event) => set({ endHour: Number(event.target.value) })}>
            {Array.from({ length: HOURS_PER_DAY }, (_, index) => index + 1).map((hour) => (
              <option key={hour} value={hour}>
                {hourLabel(hour)}
              </option>
            ))}
          </select>
        </span>,
      )}
      {hasSectorRow(kind) &&
        row(
          state.sectorOn,
          (sectorOn) => set({ sectorOn }),
          'Setor de direção',
          'Usa apenas os passos em que o sensor escolhido aponta para o setor indicado.',
          <span className="choices">
            <select aria-label="Sensor de direção" value={state.direction ?? ''} onChange={(event) => set({ direction: event.target.value || null })}>
              {dataset.columns.filter(isMeanDirection).map((column) => (
                <option key={column.id} value={column.id}>
                  {columnLabel(column)}
                </option>
              ))}
            </select>
            <select
              aria-label="Nº de setores"
              title="Define o tamanho dos setores (360° ÷ n) usados neste filtro."
              value={state.sectors}
              onChange={(event) => set({ sectors: Number(event.target.value), sector: 0 })}
            >
              {SECTOR_CHOICES.map((count) => (
                <option key={count} value={count}>
                  {count} setores
                </option>
              ))}
            </select>
            <select aria-label="Setor" value={state.sector} onChange={(event) => set({ sector: Number(event.target.value) })}>
              {sectorLabels(state.sectors).map((name, index) => (
                <option key={index} value={index}>
                  {name}
                </option>
              ))}
            </select>
          </span>,
        )}
      {row(
        state.valueOn,
        (valueOn) => set({ valueOn }),
        'Coluna de dados',
        'Mantém só passos em que a coluna escolhida está na faixa [mín, máx); útil para isolar faixas de velocidade ou temperatura.',
        <span className="choices">
          <select aria-label="Coluna" value={state.column ?? ''} onChange={(event) => set({ column: event.target.value || null })}>
            {view.shown().map((column) => (
              <option key={column.id} value={column.id}>
                {withUnits(view.name(column), column)}
              </option>
            ))}
          </select>
          <input aria-label="Mínimo" placeholder="mín" inputMode="decimal" value={state.min} onChange={(event) => set({ min: event.target.value })} />
          <input aria-label="Máximo" placeholder="máx" inputMode="decimal" value={state.max} onChange={(event) => set({ max: event.target.value })} />
          {units}
        </span>,
      )}
    </details>
  );
}

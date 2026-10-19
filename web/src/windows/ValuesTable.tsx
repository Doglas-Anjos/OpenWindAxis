/**
 * The values table of Flag Manually (ESPEC-INTERFACE §5.1): one row per step,
 * the step's start and the plotted columns' raw values, each cell in the
 * colour of a flag on it. Only the rows in view are drawn and fetched; a drag
 * over rows (or a click, Shift+click to extend) selects the segment the flag
 * commands act on, mirrored on the chart.
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import type { WindApi } from '../api/client';
import type { ColumnDto } from '../api/generated/ColumnDto';
import type { DatasetDto } from '../api/generated/DatasetDto';
import { errorMessage, formatNumber } from '../format';
import { formatDateTime } from '../workbook/model';

/** Height of a row, px: rows are laid out by index, so all are equal. */
const ROW_HEIGHT_PX = 22;
/** Rows fetched per page; a new fetch only when the view crosses a page. */
const PAGE_ROWS = 50;
/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** Decimals shown in a cell. */
const CELL_DIGITS = 3;
/** Share of the flag colour in a flagged cell, %. */
const FLAG_TINT_PERCENT = 40;
/** Short labels of the kinds, as in the original's column headings (WS90, WD97, T97). */
const KIND_SHORT: Partial<Record<ColumnDto['kind'], string>> = {
  wind_speed: 'WS',
  vertical_wind_speed: 'VWS',
  wind_direction: 'WD',
  temperature: 'T',
  pressure: 'P',
  relative_humidity: 'RH',
  solar_radiation: 'SR',
  battery_voltage: 'BV',
  turbulence_intensity: 'TI',
  power_density: 'WPD',
  air_density: 'AD',
};
const SUBTYPE_SHORT: Record<ColumnDto['subtype'], string> = { mean: '', std_dev: ' DP', min: ' mín', max: ' máx', gust: ' raj' };

/** "WS90", "WD97 DP": kind and height, the statistic when not the mean. */
const shortLabel = (column: ColumnDto) =>
  `${KIND_SHORT[column.kind] ?? column.id}${column.heightM ?? ''}${KIND_SHORT[column.kind] ? SUBTYPE_SHORT[column.subtype] : ''}`;

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** Ids of the columns shown. */
  columns: string[];
  /** The selected steps, inclusive. */
  selection: [number, number] | null;
  /** A drag or click picked these steps, inclusive. */
  onSelect: (first: number, last: number) => void;
}

// ponytail: rows laid out by index; past ~1.5 M steps (browser element height cap) this needs paging by scroll position.
export function ValuesTable({ api, dataset, columns, selection, onSelect }: Props) {
  const pane = useRef<HTMLDivElement>(null);
  const [firstRow, setFirstRow] = useState(0);
  const [viewRows, setViewRows] = useState(PAGE_ROWS);
  const [page, setPage] = useState<{ from: number; values: Float64Array[] } | null>(null);
  const [error, setError] = useState('');
  /** Where a drag started, and the rows it covers now. */
  const [drag, setDrag] = useState<{ anchor: number; last: number } | null>(null);
  const stepMs = dataset.stepS * MS_PER_SECOND;
  const timeOf = (step: number) => dataset.startMs + step * stepMs;
  const byId = new Map(dataset.columns.map((column) => [column.id, column]));
  const shown = columns.filter((id) => byId.has(id)).map((id) => byId.get(id)!);

  const block = Math.floor(firstRow / PAGE_ROWS);
  const from = Math.max(0, (block - 1) * PAGE_ROWS);
  const to = Math.min(dataset.stepCount, (block + 2) * PAGE_ROWS + viewRows);
  const columnsKey = columns.join('\n');
  useEffect(() => {
    let current = true;
    // Short ranges come back raw from the decimation: one point per step, gaps as NaN.
    Promise.all(columns.map((column) => api.series({ dataset: dataset.id, column, fromMs: timeOf(from), toMs: timeOf(to - 1), buckets: to - from })))
      .then((packed) => {
        if (!current) return;
        const values = packed.map((points) => {
          const row = new Float64Array(to - from).fill(NaN);
          for (let index = 0; index < points.length; index += 2) row[Math.round((points[index] - timeOf(from)) / stepMs)] = points[index + 1];
          return row;
        });
        setPage({ from, values });
        setError('');
      })
      .catch((thrown) => current && setError(errorMessage(thrown)));
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, dataset.id, dataset.columns, dataset.startMs, columnsKey, from, to]);

  useEffect(() => {
    const element = pane.current!;
    const observer = new ResizeObserver(() => setViewRows(Math.ceil(element.clientHeight / ROW_HEIGHT_PX)));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // A selection made on the chart scrolls its start into view.
  useEffect(() => {
    const element = pane.current;
    if (!selection || !element || drag) return;
    const top = selection[0] * ROW_HEIGHT_PX;
    if (top < element.scrollTop || top > element.scrollTop + element.clientHeight - 2 * ROW_HEIGHT_PX) element.scrollTop = top;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection?.[0], selection?.[1]]);

  /** Per shown column, the flag colours on it between `from` and `to`. */
  const tints = useMemo(() => {
    const parentOf = new Map(dataset.columns.map((column) => [column.id, column.parent]));
    const nearby = dataset.segments.filter((segment) => segment.endStep > from && segment.startStep < to);
    return shown.map((column) => {
      // A derived column takes the flags of the measured columns it is computed from.
      const inputs = [column.id, ...(column.derivedFrom ?? [])];
      return nearby.filter((segment) => inputs.some((input) => input === segment.column || parentOf.get(input) === segment.column));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dataset.segments, dataset.columns, columnsKey, from, to]);
  const tintOf = (position: number, step: number) => {
    const segment = tints[position].find((candidate) => candidate.startStep <= step && step < candidate.endStep);
    return segment && `color-mix(in srgb, ${dataset.flags[segment.flag].color} ${FLAG_TINT_PERCENT}%, transparent)`;
  };

  const [low, high] = drag ? [Math.min(drag.anchor, drag.last), Math.max(drag.anchor, drag.last)] : (selection ?? [-1, -1]);
  const finish = () => {
    if (!drag) return;
    onSelect(Math.min(drag.anchor, drag.last), Math.max(drag.anchor, drag.last));
    setDrag(null);
  };
  const rows = [];
  for (let step = from; step < to; step++) {
    const values = page && page.from === from ? page.values : null;
    rows.push(
      <tr
        key={step}
        className={step >= low && step <= high ? 'sel' : undefined}
        onMouseDown={(event) => {
          event.preventDefault();
          setDrag({ anchor: event.shiftKey && selection ? selection[0] : step, last: step });
        }}
        onMouseEnter={() => drag && setDrag({ ...drag, last: step })}
      >
        <td>{formatDateTime(timeOf(step))}</td>
        {shown.map((column, position) => (
          <td key={column.id} className="r" style={{ background: tintOf(position, step) }}>
            {values ? formatNumber(values[position][step - from], CELL_DIGITS) : ''}
          </td>
        ))}
      </tr>,
    );
  }

  return (
    <div className="values-pane" ref={pane} onScroll={(event) => setFirstRow(Math.floor(event.currentTarget.scrollTop / ROW_HEIGHT_PX))} onMouseUp={finish} onMouseLeave={finish}>
      {error && <div className="note err">{error}</div>}
      <table className="grid values">
        <thead>
          <tr>
            <th>Início do passo</th>
            {shown.map((column) => (
              <th key={column.id} title={column.id}>
                {shortLabel(column)}
              </th>
            ))}
          </tr>
          <tr className="units">
            <th />
            {shown.map((column) => (
              <th key={column.id}>{column.units}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr style={{ height: from * ROW_HEIGHT_PX }} />
          {rows}
          <tr style={{ height: (dataset.stepCount - to) * ROW_HEIGHT_PX }} />
        </tbody>
      </table>
    </div>
  );
}

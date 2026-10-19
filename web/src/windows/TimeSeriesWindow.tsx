/**
 * The Time Series window (ESPEC-INTERFACE §4.1) and, with `flagging`, the
 * Flag Manually window (§5.1) built on it: the chart on the left, the
 * legend-selector of columns on the right; zoom with the wheel, the slider or
 * a drag. Flag Manually adds the segment's start and end (the visible
 * stretch, or rows dragged in its values table), the flag to apply or
 * remove and the three commands, with their Alt shortcuts.
 */
import { useEffect, useMemo, useState } from 'react';
import { Plot, SAFE_TOOLTIP, useQuery } from '../Analyses';
import type { MeanPeriodDto } from '../api/generated/MeanPeriodDto';
import type { WindApi } from '../api/client';
import { Chart } from '../Chart';
import { useColumnView } from '../display';
import { formatDateTime, isDerived, type Entry } from '../workbook/model';
import type { Workbook } from '../workbook/useWorkbook';
import { ValuesTable } from './ValuesTable';

/** What the chart shows: the measured steps, or means per period. */
const MEASURED = 'measured';
type Display = typeof MEASURED | MeanPeriodDto;
const DISPLAY_LABEL: Record<Display, string> = { measured: 'Dados medidos', day: 'Médias diárias', month: 'Médias mensais', year: 'Médias anuais' };
const FLAGS_ON_MEASURED = 'A marcação é feita sobre os dados medidos';
/** Milliseconds in half a period's span: means are drawn at the period's middle. */
const HALF = 0.5;
/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;

/** Means of the plotted columns per day, month or year, from the core's
 *  per-day summaries (default flag filtering), one line per column. */
function PeriodMeansChart({ api, entry, period }: { api: WindApi; entry: Entry; period: MeanPeriodDto }) {
  const { dataset } = entry.result;
  const { data: answer, error } = useQuery(
    () => Promise.all(entry.plotted.map((column) => api.periodMeans({ dataset: dataset.id, column, period }).then((means) => ({ column, means })))),
    [api, entry.result, JSON.stringify(entry.plotted), period],
  );
  const option = useMemo(
    () =>
      answer && {
        animation: false,
        useUTC: true,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' },
        legend: { top: 4, type: 'scroll' },
        grid: { left: 56, right: 24, top: 40, bottom: 40 },
        xAxis: { type: 'time' },
        yAxis: { type: 'value', scale: true },
        series: answer.map(({ column, means }) => ({
          type: 'line',
          name: column,
          connectNulls: false,
          showSymbol: means.means.length < MANY_POINTS,
          data: means.means.map((mean, index) => [means.fromMs[index] + (means.toMs[index] - means.fromMs[index]) * HALF, mean]),
        })),
      },
    [answer],
  );
  if (error) return <div className="note err">{error}</div>;
  return option ? <Plot option={option} /> : null;
}
/** Above this many points a line drops its markers. */
const MANY_POINTS = 60;

/** Alt shortcuts of the flag commands (ESPEC-INTERFACE §5.1). */
const APPLY_KEY = 'f';
const REMOVE_KEY = 'r';
const REMOVE_ALL_KEY = 'a';

interface Props {
  api: WindApi;
  entry: Entry;
  workbook: Workbook;
  /** Show the flag controls (Flag Manually). */
  flagging?: boolean;
  /** Whether a command can run now. */
  ready: boolean;
}

export function TimeSeriesWindow({ api, entry, workbook, flagging = false, ready }: Props) {
  const { dataset } = entry.result;
  /** The visible stretch, epoch ms: the segment the flag commands act on. */
  const [segment, setSegment] = useState<[number, number]>([0, 0]);
  const [flagIndex, setFlagIndex] = useState(0);
  const [display, setDisplay] = useState<Display>(MEASURED);
  /** The window the values table asked the chart to zoom to. */
  const [zoom, setZoom] = useState<[number, number]>();
  const stepMs = dataset.stepS * MS_PER_SECOND;
  const stepOf = (epochMs: number) => Math.min(dataset.stepCount - 1, Math.max(0, Math.round((epochMs - dataset.startMs) / stepMs)));
  const timeOf = (step: number) => dataset.startMs + step * stepMs;
  /** The chart's window, on the step grid; its echo of a table zoom keeps the table's rows. */
  const onRange = (fromMs: number, toMs: number) => {
    if (zoom && Math.abs(fromMs - zoom[0]) < stepMs && Math.abs(toMs - zoom[1]) < stepMs) return;
    setSegment([timeOf(stepOf(fromMs)), timeOf(stepOf(toMs))]);
  };
  const selectRows = (first: number, last: number) => {
    setSegment([timeOf(first), timeOf(last)]);
    // Half a step either side: the chart shows the rows' points, not their edges.
    setZoom([timeOf(first) - stepMs * HALF, timeOf(last) + stepMs * HALF]);
  };
  const view = useColumnView(dataset);
  // Hidden columns leave the chart, the table and the flag commands too.
  const hiddenIds = new Set(dataset.columns.filter((column) => !view.visible(column)).map((column) => column.id));
  const plotted = entry.plotted.filter((id) => !hiddenIds.has(id));
  const derived = new Set(dataset.columns.filter(isDerived).map((column) => column.id));
  const flaggable = plotted.filter((id) => !derived.has(id));

  const apply = () => workbook.editFlags(entry, plotted, segment, true, flagIndex);
  const removeOne = () => workbook.editFlags(entry, plotted, segment, false, flagIndex);
  const removeAll = () => workbook.editFlags(entry, plotted, segment, false, undefined);

  useEffect(() => {
    if (!flagging) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey || !ready) return;
      const command = { [APPLY_KEY]: apply, [REMOVE_KEY]: removeOne, [REMOVE_ALL_KEY]: removeAll }[event.key.toLowerCase()];
      if (!command) return;
      event.preventDefault();
      void command();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  return (
    <div className={`timeseries ${flagging ? 'flagging' : ''}`}>
      <div className="ts-display" role="radiogroup" aria-label="Exibir">
        <span>Exibir</span>
        {(Object.keys(DISPLAY_LABEL) as Display[]).map((choice) => (
          <label key={choice} title={flagging && choice !== MEASURED ? FLAGS_ON_MEASURED : undefined}>
            <input type="radio" checked={display === choice} disabled={flagging && choice !== MEASURED} onChange={() => setDisplay(choice)} /> {DISPLAY_LABEL[choice]}
          </label>
        ))}
      </div>
      <div className="ts-body">
        <div className="ts-chart">
          {display === MEASURED ? (
            <Chart api={api} dataset={dataset} columns={plotted} onRange={onRange} zoom={zoom} />
          ) : (
            <PeriodMeansChart api={api} entry={entry} period={display} />
          )}
        </div>
        <aside className="ts-legend" aria-label="Colunas no gráfico">
          {view.shown().map((column) => (
            <label key={column.id} className="check" title={column.derivedFrom ? `Derivada de ${column.derivedFrom.join(', ')}` : column.id}>
              <input type="checkbox" checked={entry.plotted.includes(column.id)} onChange={() => workbook.togglePlot(entry, column.id)} />
              <span className={column.derivedFrom ? 'derived' : ''} style={{ borderLeftColor: view.color(column) }}>
                {view.name(column)}
              </span>
            </label>
          ))}
          {dataset.flags.length > 0 && (
            <div className="ts-flags">
              {dataset.flags.map((flag, index) => (
                <span key={index} className="flagname">
                  <span className="swatch" style={{ background: flag.color }} />
                  {flag.name}
                </span>
              ))}
            </div>
          )}
        </aside>
      </div>
      {flagging && display === MEASURED && (
        <ValuesTable api={api} dataset={dataset} columns={plotted} selection={segment[1] >= segment[0] && dataset.stepCount ? [stepOf(segment[0]), stepOf(segment[1])] : null} onSelect={selectRows} />
      )}
      {flagging && (
        <div className="flag-controls">
          <div className="segment">
            <label>
              Início
              <input readOnly value={segment[1] >= segment[0] && dataset.stepCount ? formatDateTime(segment[0]) : ''} />
            </label>
            <label>
              Fim
              <input readOnly value={segment[1] >= segment[0] && dataset.stepCount ? formatDateTime(segment[1]) : ''} />
            </label>
          </div>
          <div className="target">
            <span>Colunas a marcar</span>
            <span className="muted">{flaggable.length ? flaggable.join(', ') : 'marque colunas medidas na legenda'} (DP, mín., máx. e rajada seguem a média)</span>
          </div>
          <label>
            Flag a aplicar ou remover
            <select value={flagIndex} onChange={(event) => setFlagIndex(Number(event.target.value))}>
              {dataset.flags.map((flag, index) => (
                <option key={index} value={index}>
                  {flag.name}
                </option>
              ))}
            </select>
          </label>
          <div className="actions">
            <button className="btn" disabled={!ready || !flaggable.length} onClick={apply} title="Alt+F">
              Aplicar flag ao segmento
            </button>
            <button className="btn" disabled={!ready || !flaggable.length} onClick={removeOne} title="Alt+R">
              Remover flag do segmento
            </button>
            <button className="btn" disabled={!ready || !flaggable.length} onClick={removeAll} title="Alt+A">
              Remover todas as flags
            </button>
            <button className="btn" disabled={!ready || !entry.canUndo} onClick={() => workbook.stepHistory(entry, 'undo')} title="Ctrl+Z">
              Desfazer
            </button>
            <button className="btn" disabled={!ready || !entry.canRedo} onClick={() => workbook.stepHistory(entry, 'redo')} title="Ctrl+Shift+Z ou Ctrl+Y">
              Refazer
            </button>
          </div>
          <p className="hint">
            O segmento é o trecho visível do gráfico, escolhido com o zoom (roda do mouse, arrasto ou a barra embaixo), ou as linhas arrastadas na tabela
            (Shift+clique estende). Arraste a borda de baixo do gráfico para dividir o espaço com a tabela.
          </p>
        </div>
      )}
    </div>
  );
}

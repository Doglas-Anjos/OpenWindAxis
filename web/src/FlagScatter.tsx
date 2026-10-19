/**
 * Flag › Flag por dispersão (manual dlg-flag-flag-by-scatter-plot.htm):
 * the scatter of two columns, a selection drawn on it (free-form,
 * rectangle or triangle; Shift or Ctrl adds to it), and the flag applied to
 * or removed from the steps it holds, in the columns chosen. The core
 * resolves the selection over every step the filter keeps, not only the
 * points drawn (crates/wind-core/src/region.rs), as one undoable edit.
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import * as echarts from 'echarts/core';
import { BrushComponent, ToolboxComponent } from 'echarts/components';
import type { WindApi } from './api/client';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { FilterDto } from './api/generated/FilterDto';
import type { OpDto } from './api/generated/OpDto';
import { ColumnSelect, isMeanSpeed, SAFE_TOOLTIP, useQuery } from './Analyses';
import { CHART_LOCALE } from './chartLocale';
import { formatCount } from './format';

echarts.use([BrushComponent, ToolboxComponent]);

type Mode = 'lasso' | 'rect' | 'triangle';
const MODE_LABEL: Record<Mode, string> = { lasso: 'Livre', rect: 'Retângulo', triangle: 'Triângulo' };
/** Corners of a triangular selection, clicked one by one. */
const TRIANGLE_CORNERS = 3;
const CHART_HEIGHT = 460;
/** Points the plot draws at most; the selection still covers every step. */
const MAX_POINTS = 20_000;
/** Above this many points ECharts draws them in its large mode. */
const LARGE_SCATTER = 2_000;
const SELECTION_COLOR = '#d64545';

type Shape = [number, number][];

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** The window's "Filtrar por" criteria: flagged points show only when their flag is kept. */
  filter: FilterDto | undefined;
  onApply: (op: OpDto) => Promise<void>;
  disabled: boolean;
}

/** A selection from an ECharts brush area in data units: a rectangle or a free-form polygon. */
function shapeOf(area: { brushType: string; coordRange: unknown }): Shape | null {
  if (area.brushType === 'rect') {
    const [[x0, x1], [y0, y1]] = area.coordRange as [[number, number], [number, number]];
    return [
      [x0, y0],
      [x1, y0],
      [x1, y1],
      [x0, y1],
    ];
  }
  const corners = area.coordRange as [number, number][];
  return corners.length >= TRIANGLE_CORNERS ? corners : null;
}

export function FlagScatterCard({ api, dataset, filter, onApply, disabled }: Props) {
  const speeds = dataset.columns.filter(isMeanSpeed);
  const [xId, setXId] = useState<string | null>(() => speeds[1]?.id ?? dataset.columns[0]?.id ?? null);
  const [yId, setYId] = useState<string | null>(() => speeds[0]?.id ?? dataset.columns[1]?.id ?? null);
  const [mode, setMode] = useState<Mode>('rect');
  const [shapes, setShapes] = useState<Shape[]>([]);
  /** Triangle corners clicked so far. */
  const [corners, setCorners] = useState<Shape>([]);
  const [columns, setColumns] = useState<string[]>(() => [yId].filter((id): id is string => id != null));
  const reserved = dataset.flags.findIndex((flag) => flag.name === 'Inválido');
  const [flag, setFlag] = useState(Math.max(reserved, 0));
  const [working, setWorking] = useState(false);
  const adding = useRef(false);

  // A new pair of axes starts a new selection.
  useEffect(() => setShapes([]), [xId, yId]);
  useEffect(() => {
    const track = (event: KeyboardEvent) => (adding.current = event.shiftKey || event.ctrlKey);
    window.addEventListener('keydown', track);
    window.addEventListener('keyup', track);
    return () => {
      window.removeEventListener('keydown', track);
      window.removeEventListener('keyup', track);
    };
  }, []);

  const request = xId == null || yId == null ? null : { dataset: dataset.id, x: xId, y: yId, filter, maxPoints: MAX_POINTS };
  const { data: scatter, error } = useQuery(() => (request ? api.scatter(request) : undefined), [api, dataset, xId, yId, filter]);
  const selection = xId == null || yId == null ? null : { x: xId, y: yId, shapes, filter };
  const { data: count } = useQuery(
    () => (selection && shapes.length ? api.selectionCount({ dataset: dataset.id, ...selection }) : undefined),
    [api, dataset, shapes, filter],
  );

  const addShape = (shape: Shape) => setShapes((current) => (adding.current ? [...current, shape] : [shape]));

  const element = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  useEffect(() => {
    const instance = echarts.init(element.current!, undefined, { locale: CHART_LOCALE });
    chart.current = instance;
    instance.on('brushEnd', (params) => {
      const areas = (params as { areas: { brushType: string; coordRange: unknown }[] }).areas;
      const shape = areas.length ? shapeOf(areas[areas.length - 1]) : null;
      if (shape) addShape(shape);
      // Our own series draws the selection; the brush's rectangle goes away.
      instance.dispatchAction({ type: 'brush', areas: [] });
    });
    instance.getZr().on('click', (event) => {
      if (modeRef.current !== 'triangle') return;
      const point = instance.convertFromPixel({ seriesIndex: 0 }, [event.offsetX, event.offsetY]) as [number, number];
      adding.current = adding.current || Boolean((event.event as MouseEvent | undefined)?.shiftKey || (event.event as MouseEvent | undefined)?.ctrlKey);
      setCorners((current) => {
        const next = [...current, point];
        if (next.length < TRIANGLE_CORNERS) return next;
        addShape(next);
        return [];
      });
    });
    const resizeObserver = new ResizeObserver(() => instance.resize());
    resizeObserver.observe(element.current!);
    return () => {
      resizeObserver.disconnect();
      instance.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const option = useMemo(() => {
    if (!scatter) return null;
    const closed = (shape: Shape) => [...shape, shape[0]];
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
      grid: { left: 64, right: 24, top: 24, bottom: 48 },
      xAxis: { type: 'value', scale: true, name: xId, nameLocation: 'middle', nameGap: 28 },
      yAxis: { type: 'value', scale: true, name: yId, nameLocation: 'middle', nameGap: 44 },
      brush: { xAxisIndex: 0, yAxisIndex: 0, transformable: false, throttleType: 'debounce', brushStyle: { color: 'rgba(214, 69, 69, 0.1)', borderColor: SELECTION_COLOR } },
      series: [
        { type: 'scatter', name: 'Passos', data: scatter.xs.map((x, index) => [x, scatter.ys[index]]), symbolSize: 3, large: scatter.xs.length > LARGE_SCATTER },
        ...shapes.map((shape) => ({ type: 'line', data: closed(shape), showSymbol: false, silent: true, lineStyle: { color: SELECTION_COLOR, type: 'dashed' } })),
        ...(corners.length ? [{ type: 'line', data: corners, symbolSize: 6, silent: true, lineStyle: { color: SELECTION_COLOR } }] : []),
      ],
    };
  }, [scatter, shapes, corners, xId, yId]);
  useEffect(() => {
    if (!option || !chart.current) return;
    chart.current.setOption(option, { notMerge: true });
    const brushType = mode === 'triangle' ? false : mode === 'lasso' ? 'polygon' : 'rect';
    chart.current.dispatchAction({ type: 'takeGlobalCursor', key: 'brush', brushOption: { brushType, brushMode: 'single' } });
  }, [option, mode]);

  async function act(kind: 'apply' | 'remove' | 'remove_all') {
    if (!selection || !shapes.length || !columns.length) return;
    setWorking(true);
    const op: OpDto =
      kind === 'apply'
        ? { op: 'flag_region', ...selection, columns, flag }
        : { op: 'unflag_region', ...selection, columns, ...(kind === 'remove' ? { flag } : {}) };
    await onApply(op);
    setWorking(false);
  }

  const blocked = disabled || working || !shapes.length || !columns.length;
  const measured = dataset.columns.filter((column) => !column.derivedFrom);
  return (
    <section className="card flagscatter">
      <div className="cardhead">
        <h3>Flag por dispersão</h3>
      </div>
      <div className="flagscatter-body">
        <div className="controls">
          <ColumnSelect label="Eixo x" dataset={dataset} value={xId} onChange={setXId} />
          <ColumnSelect label="Eixo y" dataset={dataset} value={yId} onChange={setYId} />
          <fieldset className="checks" title="Arraste no gráfico para selecionar; com Shift ou Ctrl a seleção se soma à anterior.">
            <legend>Selecionar</legend>
            {(Object.keys(MODE_LABEL) as Mode[]).map((candidate) => (
              <label key={candidate} className="check">
                <input type="radio" name="flagscatter-mode" checked={mode === candidate} onChange={() => (setMode(candidate), setCorners([]))} />
                {MODE_LABEL[candidate]}
              </label>
            ))}
          </fieldset>
          <fieldset className="checks" title="Colunas que recebem ou perdem a flag; DP, mín., máx. e rajada seguem a média.">
            <legend>Colunas a marcar</legend>
            {measured.map((column) => (
              <label key={column.id} className="check">
                <input
                  type="checkbox"
                  checked={columns.includes(column.id)}
                  onChange={() => setColumns(columns.includes(column.id) ? columns.filter((id) => id !== column.id) : [...columns, column.id])}
                />
                {column.id}
              </label>
            ))}
          </fieldset>
          <label>
            Flag a aplicar ou remover
            <select value={flag} onChange={(event) => setFlag(Number(event.target.value))}>
              {dataset.flags.map((known, index) => (
                <option key={index} value={index}>
                  {known.name}
                </option>
              ))}
            </select>
          </label>
          <button className="btn primary" disabled={blocked} onClick={() => act('apply')}>
            Aplicar flag à seleção
          </button>
          <button className="btn" disabled={blocked} onClick={() => act('remove')}>
            Remover flag da seleção
          </button>
          <button className="btn" disabled={blocked} onClick={() => act('remove_all')}>
            Remover todas as flags da seleção
          </button>
          <button className="btn" disabled={!shapes.length && !corners.length} onClick={() => (setShapes([]), setCorners([]))}>
            Limpar seleção
          </button>
          <p className="hint selectioncount">
            Pontos selecionados: <b>{shapes.length ? formatCount(count?.steps ?? 0) : 0}</b>
            {scatter && scatter.stride > 1 && ` (o gráfico desenha 1 a cada ${formatCount(scatter.stride)} de ${formatCount(scatter.count)} passos; a seleção vale para todos)`}
          </p>
          {error && <div className="note err">{error}</div>}
        </div>
        <div ref={element} className="chart" style={{ height: CHART_HEIGHT }} />
      </div>
    </section>
  );
}

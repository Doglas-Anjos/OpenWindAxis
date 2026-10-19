import { useEffect, useRef } from 'react';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkAreaComponent,
  TooltipComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { WindApi } from './api/client';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import { CHART_LOCALE } from './chartLocale';
import { useColumnView } from './display';

echarts.use([LineChart, GridComponent, DataZoomComponent, LegendComponent, MarkAreaComponent, TooltipComponent, CanvasRenderer]);

/** Points requested for the whole-period overview. */
const OVERVIEW_BUCKETS = 2000;
/** Fewest buckets for the zoomed detail, whatever the chart width. */
const MIN_DETAIL_BUCKETS = 200;
/** Chart width assumed before the element is measured, px. */
const FALLBACK_WIDTH_PX = 800;
/** Wait after the last zoom event before fetching detail, ms. */
const ZOOM_DEBOUNCE_MS = 120;
/** Opacity of the shading over flagged stretches. */
const FLAG_SHADE_OPACITY = 0.18;
/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** Percent span of the zoom slider. */
const FULL_RANGE_PERCENT = 100;

type Point = [timeMs: number, value: number];

/** `[t, v, t, v, …]` from the core as chart points. */
function toPoints(packed: Float64Array): Point[] {
  const points: Point[] = new Array(packed.length / 2);
  for (let index = 0; index < packed.length; index += 2) points[index / 2] = [packed[index], packed[index + 1]];
  return points;
}

/** A failed series fetch only leaves the chart stale; log it rather than reject unhandled. */
const reportChartError = (error: unknown) => console.error('chart data unavailable:', error);

/** `name` with the column's units, when known. */
const withUnits = (name: string, column: ColumnDto) => (column.units ? `${name} (${column.units})` : name);

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** Ids of the columns to plot. */
  columns: string[];
  /** Receives the visible time window (epoch ms) whenever it changes. */
  onRange: (fromMs: number, toMs: number) => void;
  /** Zooms to this window (epoch ms) whenever it changes. */
  zoom?: [number, number];
}

/** Time-series chart. The core decimates: a low-res overview of the whole
 *  period, replaced by full detail inside the zoom window on every zoom.
 *  Flagged stretches of the plotted columns (or their parents) are shaded. */
export function Chart({ api, dataset, columns, onRange, zoom }: Props) {
  const element = useRef<HTMLDivElement>(null);
  const view = useColumnView(dataset);
  const byId = new Map(dataset.columns.map((column) => [column.id, column]));
  /** Series name and colour of each plotted column, by the dataset's display settings. */
  const looks = columns.map((id) => {
    const column = byId.get(id)!;
    return { name: withUnits(view.name(column), column), color: view.color(column) };
  });
  // A change of names or colours draws the chart again.
  const looksKey = JSON.stringify(looks);
  const chart = useRef<echarts.ECharts | null>(null);

  useEffect(() => {
    const instance = echarts.init(element.current!, undefined, { locale: CHART_LOCALE });
    chart.current = instance;
    const resizeObserver = new ResizeObserver(() => instance.resize());
    resizeObserver.observe(element.current!);
    return () => {
      resizeObserver.disconnect();
      instance.dispose();
    };
  }, []);

  // The data effect below outlives renders: it calls the latest callback.
  const reportRange = useRef(onRange);
  reportRange.current = onRange;

  const columnsKey = columns.join('\n');
  /** Shading of flagged stretches; drawn again whenever the overview is redrawn. */
  const flagAreas = useRef<unknown[]>([]);
  /** Whether the current dataset's overview is on the chart (so it has series to shade). */
  const overviewDrawn = useRef(false);
  const shading = () => ({ silent: true, data: flagAreas.current });

  useEffect(() => {
    const instance = chart.current!;
    const startMs = dataset.startMs;
    const endMs = dataset.startMs + dataset.stepCount * dataset.stepS * MS_PER_SECOND;
    const seriesOf = async (column: string, fromMs: number, toMs: number, buckets: number) =>
      toPoints(await api.series({ dataset: dataset.id, column, fromMs, toMs, buckets }));
    /** False once the effect is cleaned up: late answers are then dropped. */
    let current = true;
    overviewDrawn.current = false;
    // The window flag edits act on is the new full range from now on, not
    // only once the overview arrives from the worker.
    reportRange.current(startMs, endMs);
    let overview: Point[][] = [];
    /** Only the latest zoom's detail is drawn. */
    let latestZoom = 0;
    let pendingZoom: number | undefined;

    const drawOverview = async () => {
      overview = await Promise.all(columns.map((column) => seriesOf(column, startMs, endMs, OVERVIEW_BUCKETS)));
      if (!current) return;
      instance.setOption(
        {
          useUTC: true,
          animation: false,
          grid: { left: 56, right: 24, top: 40, bottom: 72 },
          tooltip: { trigger: 'axis' },
          legend: { top: 4, type: 'scroll' },
          xAxis: { type: 'time', min: startMs, max: endMs },
          yAxis: { type: 'value', scale: true },
          dataZoom: [
            { type: 'inside', throttle: 50 },
            { type: 'slider', height: 24, bottom: 16 },
          ],
          series: columns.map((_, position) => ({
            type: 'line',
            name: looks[position].name,
            ...(looks[position].color && { color: looks[position].color }),
            data: overview[position],
            showSymbol: false,
            connectNulls: false,
            lineStyle: { width: 1 },
            ...(position === 0 ? { markArea: shading() } : {}),
          })),
        },
        { notMerge: true },
      );
      overviewDrawn.current = true;
    };

    const drawDetail = async (zoom: number, fromMs: number, toMs: number) => {
      const buckets = Math.max(MIN_DETAIL_BUCKETS, element.current?.clientWidth ?? FALLBACK_WIDTH_PX);
      const details = await Promise.all(columns.map((column) => seriesOf(column, fromMs, toMs, buckets)));
      if (!current || zoom !== latestZoom) return;
      instance.setOption({
        series: columns.map((_, position) => {
          const whole = overview[position] ?? [];
          const before = whole.filter(([timeMs]) => timeMs < fromMs);
          const after = whole.filter(([timeMs]) => timeMs > toMs);
          return { data: [...before, ...details[position], ...after] };
        }),
      });
    };

    const onZoom = () => {
      window.clearTimeout(pendingZoom);
      pendingZoom = window.setTimeout(() => {
        const zoom = (instance.getOption().dataZoom as { start: number; end: number }[])[0];
        const fromMs = startMs + ((endMs - startMs) * zoom.start) / FULL_RANGE_PERCENT;
        const toMs = startMs + ((endMs - startMs) * zoom.end) / FULL_RANGE_PERCENT;
        reportRange.current(fromMs, toMs);
        drawDetail(++latestZoom, fromMs, toMs).catch(reportChartError);
      }, ZOOM_DEBOUNCE_MS);
    };

    drawOverview().catch(reportChartError);
    instance.on('datazoom', onZoom);
    return () => {
      current = false;
      window.clearTimeout(pendingZoom);
      // On unmount React runs the first effect's cleanup (dispose) before this one.
      if (!instance.isDisposed()) instance.off('datazoom', onZoom);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, dataset.id, dataset.startMs, dataset.stepCount, dataset.stepS, dataset.columns, columnsKey, looksKey]);

  // Separate from the effect above so flag edits keep the zoom.
  useEffect(() => {
    const timeOf = (step: number) => dataset.startMs + step * dataset.stepS * MS_PER_SECOND;
    const parentOf = new Map(dataset.columns.map((column) => [column.id, column.parent]));
    // A derived column inherits the flags of the measured columns it is computed from.
    const inputsOf = new Map(dataset.columns.map((column) => [column.id, [column.id, ...(column.derivedFrom ?? [])]]));
    const shown = new Set<string>();
    flagAreas.current = dataset.segments.flatMap((segment) => {
      const key = `${segment.flag}:${segment.startStep}:${segment.endStep}`;
      const flaggedBySegment = (input: string) => input === segment.column || parentOf.get(input) === segment.column;
      const covers = columns.some((column) => (inputsOf.get(column) ?? [column]).some(flaggedBySegment));
      if (!covers || shown.has(key)) return [];
      shown.add(key);
      const color = dataset.flags[segment.flag].color;
      return [[{ xAxis: timeOf(segment.startStep), itemStyle: { color, opacity: FLAG_SHADE_OPACITY } }, { xAxis: timeOf(segment.endStep) }]];
    });
    // Before the overview arrives there is nothing to shade; drawOverview picks the shading up then.
    if (columns.length && overviewDrawn.current) chart.current!.setOption({ series: [{ markArea: shading() }] });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dataset, columnsKey]);

  useEffect(() => {
    // Before the overview there is no zoom to move.
    if (zoom && overviewDrawn.current) chart.current!.dispatchAction({ type: 'dataZoom', startValue: zoom[0], endValue: zoom[1] });
  }, [zoom?.[0], zoom?.[1]]);

  return <div ref={element} className="chart" />;
}

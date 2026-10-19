/**
 * The summary graphs (ESPEC-INTERFACE §1.1, right pane): five small graphs
 * over every dataset checked as visible in the explorer, each dataset in its
 * own colour: data coverage of the primary columns, vertical shear profile,
 * direction frequency, monthly mean speed and mean diurnal profile.
 */
import { useMemo } from 'react';
import * as echarts from 'echarts/core';
import { CustomChart, LineChart, ScatterChart } from 'echarts/charts';
import { GridComponent, LegendComponent, PolarComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { WindApi } from '../api/client';
import type { CoverageDto } from '../api/generated/CoverageDto';
import type { DatasetDto } from '../api/generated/DatasetDto';
import type { DiurnalDto } from '../api/generated/DiurnalDto';
import type { MonthlyMeansDto } from '../api/generated/MonthlyMeansDto';
import type { ShearDto } from '../api/generated/ShearDto';
import type { WindRoseDto } from '../api/generated/WindRoseDto';
import { HOUR_LABELS, Plot, SAFE_TOOLTIP, useQuery } from '../Analyses';
import { formatNumber } from '../format';
import { MONTH_ABBREVIATIONS } from '../labels';
import { isDerived, isMeanSpeed, primaryColumns, type Entry } from '../workbook/model';

echarts.use([CustomChart, LineChart, ScatterChart, GridComponent, LegendComponent, PolarComponent, TooltipComponent, CanvasRenderer]);

const MS_PER_SECOND = 1000;
const PERCENT = 100;
const DEGREES_PER_TURN = 360;
/** Direction sectors of the summary rose (and of the windows' rose of every visible dataset). */
export const ROSE_SECTORS = 16;
/** Shear by sector is not drawn here; the request still needs a count. */
const SHEAR_SECTORS = 12;
/** Graph heights, px. */
const COVERAGE_ROW_PX = 22;
const COVERAGE_MIN_PX = 120;
const GRAPH_PX = 250;
/** Widest dataset name beside the coverage bands; longer ones are cut, px. */
const COVERAGE_LABEL_PX = 160;
/** Band height as a share of its row. */
const COVERAGE_BAND_SHARE = 0.6;
/** Opacity of a month with no data and of a complete month. */
const EMPTY_MONTH_OPACITY = 0.08;
const FULL_MONTH_OPACITY = 1;
/** Digits of the shear exponent in the legend. */
const ALPHA_DIGITS = 3;
/** Points of each drawn power-law curve, and its reach below and above the data. */
const CURVE_POINTS = 21;
const CURVE_BELOW = 0.6;
const CURVE_ABOVE = 1.3;
const COMPASS_8 = ['N', 'NE', 'L', 'SE', 'S', 'SO', 'O', 'NO'];
const COMPASS_STEP_DEG = DEGREES_PER_TURN / COMPASS_8.length;

/** What the summary asks the core for one dataset. */
interface DatasetSummary {
  dataset: DatasetDto;
  /** The dataset's name in the legends, unique among the graphed datasets. */
  label: string;
  color: string;
  coverage: CoverageDto;
  /** Indices of the primary columns in the dataset. */
  primary: number[];
  shear: ShearDto | null;
  rose: WindRoseDto | null;
  monthly: MonthlyMeansDto | null;
  diurnal: DiurnalDto | null;
}

/** What a graph needs of each dataset: its legend name, its colour and the answers it draws. */
export type Drawn<K extends keyof DatasetSummary = never> = Pick<DatasetSummary, 'label' | 'color' | K>;

/** A failure of one optional graph only leaves it out. */
const orNull = <T,>(pending: Promise<T> | null) => (pending ? pending.catch(() => null) : Promise.resolve(null));

/** Legend names: ECharts merges series of one name, so a repeated dataset name gets its count. */
export function legendLabels(names: string[]): string[] {
  const seen = new Map<string, number>();
  return names.map((name) => {
    const count = (seen.get(name) ?? 0) + 1;
    seen.set(name, count);
    return count > 1 ? `${name} (${count})` : name;
  });
}

async function summarize(api: WindApi, entry: Entry, label: string): Promise<DatasetSummary> {
  const { dataset } = entry.result;
  const primary = primaryColumns(dataset);
  const speeds = dataset.columns.filter((column) => isMeanSpeed(column) && !isDerived(column) && column.heightM != null);
  const heights = new Set(speeds.map((column) => column.heightM));
  const id = dataset.id;
  const [coverage, shear, rose, monthly, diurnal] = await Promise.all([
    api.coverage({ dataset: id }),
    orNull(heights.size >= 2 ? api.shear({ dataset: id, columns: speeds.map((column) => column.id), sectors: SHEAR_SECTORS }) : null),
    orNull(primary.direction ? api.windRose({ dataset: id, direction: primary.direction.id, speed: primary.speed?.id, sectors: ROSE_SECTORS }) : null),
    orNull(primary.speed ? api.monthlyMeans({ dataset: id, column: primary.speed.id }) : null),
    orNull(primary.speed ? api.diurnal({ dataset: id, column: primary.speed.id }) : null),
  ]);
  const primaryIndices = [primary.speed, primary.direction, primary.temperature].flatMap((column) => (column ? [dataset.columns.indexOf(column)] : []));
  return { dataset, label, color: entry.color, coverage, primary: primaryIndices, shear, rose, monthly, diurnal };
}

const common = { animation: false, tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' as const } };

function coverageOption(summaries: DatasetSummary[]) {
  return {
    ...common,
    tooltip: { ...SAFE_TOOLTIP, trigger: 'item' as const, valueFormatter: (value: number) => `${formatNumber(value * PERCENT)}%` },
    grid: { left: 8, right: 16, top: 8, bottom: 8, containLabel: true },
    xAxis: { type: 'time' },
    yAxis: {
      type: 'category',
      data: summaries.map((summary) => summary.label),
      inverse: true,
      axisLabel: { width: COVERAGE_LABEL_PX, overflow: 'truncate' as const },
    },
    series: summaries.map((summary, row) => {
      const { dataset, coverage } = summary;
      const at = (step: number) => dataset.startMs + step * dataset.stepS * MS_PER_SECOND;
      // A month counts as covered as far as all its primary columns are.
      const data = coverage.months.map((month, index) => {
        const shares = summary.primary.map((column) => coverage.recovery[column][index] ?? 0);
        return [at(month.startStep), at(month.endStep), row, shares.length ? Math.min(...shares) : 0];
      });
      return {
        type: 'custom',
        name: summary.label,
        encode: { x: [0, 1], y: 2, tooltip: 3 },
        data,
        renderItem: (_params: unknown, draw: { value: (index: number) => number; coord: (point: number[]) => number[]; size: (point: number[]) => number[] }) => {
          const start = draw.coord([draw.value(0), draw.value(2)]);
          const end = draw.coord([draw.value(1), draw.value(2)]);
          const height = draw.size([0, 1])[1] * COVERAGE_BAND_SHARE;
          const share = draw.value(3);
          return {
            type: 'rect',
            shape: { x: start[0], y: start[1] - height / 2, width: Math.max(end[0] - start[0], 1), height },
            style: { fill: summary.color, opacity: EMPTY_MONTH_OPACITY + (FULL_MONTH_OPACITY - EMPTY_MONTH_OPACITY) * share },
          };
        },
      };
    }),
  };
}

export function shearOption(summaries: Drawn<'shear'>[]) {
  const withShear = summaries.filter((summary) => summary.shear?.alpha != null);
  return {
    ...common,
    tooltip: { ...SAFE_TOOLTIP, trigger: 'item' as const, valueFormatter: (value: number) => formatNumber(value) },
    legend: { top: 0, type: 'scroll' as const },
    grid: { left: 56, right: 16, top: 36, bottom: 36 },
    xAxis: { type: 'value', scale: true, name: 'Velocidade média (m/s)', nameLocation: 'middle', nameGap: 22 },
    // Beside the axis, not above it, where a long legend would cover it.
    yAxis: { type: 'value', name: 'Altura (m)', nameLocation: 'middle', nameGap: 36, min: 0 },
    series: withShear.flatMap((summary) => {
      const shear = summary.shear!;
      const alpha = shear.alpha!;
      const logHeights = shear.heightsM.map(Math.log);
      const logSpeeds = shear.meanSpeeds.map(Math.log);
      const average = (values: number[]) => values.reduce((total, value) => total + value, 0) / values.length;
      const [meanLogHeight, meanLogSpeed] = [average(logHeights), average(logSpeeds)];
      const [lowest, highest] = [Math.min(...shear.heightsM), Math.max(...shear.heightsM)];
      const curve = Array.from({ length: CURVE_POINTS }, (_, point) => {
        const from = lowest * CURVE_BELOW;
        const height = from + ((highest * CURVE_ABOVE - from) * point) / (CURVE_POINTS - 1);
        return [Math.exp(meanLogSpeed + alpha * (Math.log(height) - meanLogHeight)), height];
      });
      const name = `${summary.label} (α = ${formatNumber(alpha, ALPHA_DIGITS)})`;
      return [
        { type: 'line', name, data: curve, showSymbol: false, color: summary.color },
        { type: 'scatter', name, data: shear.meanSpeeds.map((speed, index) => [speed, shear.heightsM[index]]), color: summary.color, symbolSize: 8 },
      ];
    }),
  };
}

export function roseOption(summaries: Drawn<'rose'>[]) {
  const sectorDeg = DEGREES_PER_TURN / ROSE_SECTORS;
  return {
    ...common,
    tooltip: { ...SAFE_TOOLTIP, trigger: 'item' as const, valueFormatter: (value: number) => formatNumber(value) },
    legend: { top: 0, type: 'scroll' as const },
    polar: { radius: '68%', center: ['50%', '56%'] },
    angleAxis: {
      type: 'value',
      min: 0,
      max: DEGREES_PER_TURN,
      interval: COMPASS_STEP_DEG,
      startAngle: 90,
      clockwise: true,
      axisLabel: { formatter: (degrees: number) => COMPASS_8[Math.round(degrees / COMPASS_STEP_DEG) % COMPASS_8.length] },
    },
    radiusAxis: { axisLine: { show: false }, axisTick: { show: false }, axisLabel: { formatter: (value: number) => `${value}%` } },
    series: summaries
      .filter((summary) => summary.rose)
      .map((summary) => {
        const frequency = summary.rose!.frequency.map((share) => (share ?? 0) * PERCENT);
        // Back to the first sector, so the line closes.
        const points = [...frequency, frequency[0]].map((value, sector) => [value, sector * sectorDeg]);
        return { type: 'line', coordinateSystem: 'polar', name: summary.label, data: points, color: summary.color, showSymbol: false };
      }),
  };
}

export function lineOption<T extends Drawn>(categories: string[], yName: string, summaries: T[], values: (summary: T) => (number | null)[] | null) {
  return {
    ...common,
    tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' as const, valueFormatter: (value: number) => formatNumber(value) },
    legend: { top: 0, type: 'scroll' as const },
    // Room at the left for the axis name, which Plot sets beside the axis (the legend runs above).
    grid: { left: 56, right: 16, top: 32, bottom: 28 },
    xAxis: { type: 'category', data: categories },
    yAxis: { type: 'value', scale: true, name: yName },
    series: summaries.flatMap((summary) => {
      const data = values(summary);
      return data ? [{ type: 'line', name: summary.label, data, color: summary.color, symbolSize: 5 }] : [];
    }),
  };
}

export function SummaryGraphs({ api, entries }: { api: WindApi; entries: Entry[] }) {
  const visible = entries.filter((entry) => entry.visible);
  const { data: summaries, error } = useQuery(() => {
    const labels = legendLabels(visible.map((entry) => entry.result.dataset.name));
    return Promise.all(visible.map((entry, position) => summarize(api, entry, labels[position])));
  }, [api, entries]);
  const options = useMemo(() => {
    if (!summaries?.length) return null;
    return {
      coverage: coverageOption(summaries),
      shear: shearOption(summaries),
      rose: roseOption(summaries),
      monthly: lineOption(MONTH_ABBREVIATIONS, 'm/s', summaries, (summary) => summary.monthly?.means ?? null),
      diurnal: lineOption(HOUR_LABELS, 'm/s', summaries, (summary) => summary.diurnal?.all ?? null),
    };
  }, [summaries]);
  if (!visible.length) return <div className="summary-empty">Os gráficos-resumo aparecem aqui para os datasets marcados como visíveis no explorador.</div>;
  return (
    <div className="summary">
      {error && <div className="note err">{error}</div>}
      {options && (
        <>
          <figure className="wide">
            <figcaption>Cobertura de dados nas colunas primárias</figcaption>
            <Plot option={options.coverage} height={Math.max(COVERAGE_MIN_PX, summaries!.length * COVERAGE_ROW_PX + 40)} />
          </figure>
          <figure>
            <figcaption>Perfil vertical de cisalhamento</figcaption>
            <Plot option={options.shear} height={GRAPH_PX} />
          </figure>
          <figure>
            <figcaption>Frequência de direção do vento</figcaption>
            <Plot option={options.rose} height={GRAPH_PX} />
          </figure>
          <figure>
            <figcaption>Velocidade média mensal</figcaption>
            <Plot option={options.monthly} height={GRAPH_PX} />
          </figure>
          <figure>
            <figcaption>Perfil diurno médio</figcaption>
            <Plot option={options.diurnal} height={GRAPH_PX} />
          </figure>
        </>
      )}
    </div>
  );
}

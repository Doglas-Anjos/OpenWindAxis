/**
 * The analysis windows (ESPEC-INTERFACE §3–§4): coverage, histogram, wind
 * rose, diurnal profile, shear, turbulence, extreme winds, scatter and tables. Every window has the same anatomy (§3.1):
 * a column of controls on the left (dataset first, then the format toggle
 * Gráfico | Tabela and the window's own controls, with "Filtrar por" at the
 * bottom) and the output on the right. A change in any control recomputes
 * at once.
 */
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import * as echarts from 'echarts/core';
import { BarChart, HeatmapChart, LineChart, ScatterChart } from 'echarts/charts';
import { GridComponent, LegendComponent, PolarComponent, TooltipComponent, VisualMapComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { WindApi } from './api/client';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { DistortionMeasureDto } from './api/generated/DistortionMeasureDto';
import type { FilterDto } from './api/generated/FilterDto';
import type { ImportResultDto } from './api/generated/ImportResultDto';
import type { ExtremePeriodDto } from './api/generated/ExtremePeriodDto';
import type { GumbelFitDto } from './api/generated/GumbelFitDto';
import type { IecEditionDto } from './api/generated/IecEditionDto';
import type { PeakMethodDto } from './api/generated/PeakMethodDto';
import type { HeightFitKindDto } from './api/generated/HeightFitKindDto';
import type { ShearAggregation } from './api/generated/ShearAggregation';
import type { TurbulenceBinDto } from './api/generated/TurbulenceBinDto';
import type { TurbulenceVersusDto } from './api/generated/TurbulenceVersusDto';
import type { WeibullMethod } from './api/generated/WeibullMethod';
import { MONTH_ABBREVIATIONS, SHEAR_AGGREGATION_LABEL, WEIBULL_METHOD_LABEL } from './labels';
import { errorMessage, formatCount, formatNumber, formatPercent, formatWithInterval, NO_VALUE } from './format';
import { formatDateTime, typedNumber } from './workbook/model';
import { CHART_LOCALE } from './chartLocale';
import { useColumnView } from './display';
import { SHEAR_DISPLAY_LABEL, ShearDisplays, SUBDIVIDED_DISPLAYS, SUBDIVISION_LABEL, type ShearDisplay, type Subdivision } from './ShearDisplays';
import { DISTORTION_DISPLAY_LABEL, DistortionDetailChart, type DistortionDisplay } from './DistortionDetail';

echarts.use([BarChart, HeatmapChart, LineChart, ScatterChart, GridComponent, LegendComponent, PolarComponent, TooltipComponent, VisualMapComponent, CanvasRenderer]);

export type AnalysisKind = 'coverage' | 'histogram' | 'distribution' | 'rose' | 'diurnal' | 'shear' | 'turbulence' | 'distortion' | 'extremes' | 'scatter' | 'tables';

interface Props {
  api: WindApi;
  /** Changes on every import or flag edit, so results recompute. */
  result: ImportResultDto;
  /** "Filtrar por" criteria; a new object only when they change. */
  filter?: FilterDto;
  /** Boom orientations by column id (Configurar dataset › Lança), where known. */
  booms?: Record<string, number>;
}

const DEGREES_PER_TURN = 360;
const HOURS_PER_DAY = 24;
const PERCENT = 100;
/** Default and offered sector counts. */
const ROSE_SECTOR_CHOICES = [4, 8, 12, 16, 36];
const DEFAULT_ROSE_SECTORS = 16;
const SHEAR_SECTOR_CHOICES = [4, 8, 12, 16];
const DEFAULT_SHEAR_SECTORS = 12;
/** Default histogram bin width, in the column's units. */
const DEFAULT_BIN_WIDTH = 1;
/** Calm threshold a rose starts with, m/s: 0 = no calms, every direction counts. */
const DEFAULT_CALM_THRESHOLD = 0;
/** Digits shown for the air density, kg/m³. */
const DENSITY_DIGITS = 3;
/** Recovery above which a coverage cell switches to light text. */
const DARK_CELL_SHARE = 0.6;
/** Digits shown for Weibull parameters, shear exponents and z₀. */
const PARAMETER_DIGITS = 3;
/** The original's third table: fits through every height, the extremes, and each neighbouring pair. */
const HEIGHT_FIT_LABEL: Record<HeightFitKindDto, string> = { all: 'Todas', extremes: 'Mais baixa e mais alta', neighbours: 'Vizinhas' };
const ROUGHNESS_DIGITS = 4;
/** Points on the drawn power-law curve, and how far it reaches past the data. */
const CURVE_POINTS = 21;
const CURVE_BELOW = 0.8;
const CURVE_ABOVE = 1.2;
/** Chart heights, px. */
const DEFAULT_CHART_HEIGHT = 380;
const ROSE_CHART_HEIGHT = 440;
const PROFILE_CHART_HEIGHT = 360;
const SMALL_CHART_HEIGHT = 170;
/** Tooltips as plain text: column names come from files, never as markup. */
export const SAFE_TOOLTIP = { renderMode: 'richText' } as const;

const COMPASS_16 = ['N', 'NNE', 'NE', 'ENE', 'L', 'ESE', 'SE', 'SSE', 'S', 'SSO', 'SO', 'OSO', 'O', 'ONO', 'NO', 'NNO'];
export const sectorLabels = (sectors: number) =>
  Array.from({ length: sectors }, (_, sector) =>
    COMPASS_16.length % sectors === 0
      ? COMPASS_16[(sector * COMPASS_16.length) / sectors]
      : `${Math.round((sector * DEGREES_PER_TURN) / sectors)}°`,
  );
export const HOUR_LABELS = Array.from({ length: HOURS_PER_DAY }, (_, hour) => `${hour}h`);
const hourRange = (hour: number) => `${String(hour).padStart(2, '0')}:00–${String(hour + 1).padStart(2, '0')}:00`;
export const asPercent = (shares: (number | null)[]) => shares.map((share) => (share == null ? null : share * PERCENT));

export const isMeanSpeed = (column: ColumnDto) => column.kind === 'wind_speed' && column.subtype === 'mean';
export const isMeanDirection = (column: ColumnDto) => column.kind === 'wind_direction' && column.subtype === 'mean';
export const columnLabel = (column: ColumnDto) => withUnits(column.id, column);
/** `name` with the column's units, when known. */
export const withUnits = (name: string, column: ColumnDto) => (column.units ? `${name} (${column.units})` : name);
const firstId = (dataset: DatasetDto, matches: (column: ColumnDto) => boolean) => dataset.columns.find(matches)?.id ?? null;

/** Mean wind speed column nearest in height to the direction column `directionId`. */
function nearestSpeed(dataset: DatasetDto, directionId: string): string | null {
  const height = dataset.columns.find((column) => column.id === directionId)?.heightM;
  const distance = (column: ColumnDto) => (height == null || column.heightM == null ? Infinity : Math.abs(column.heightM - height));
  let best: ColumnDto | null = null;
  for (const column of dataset.columns) {
    if (isMeanSpeed(column) && (best == null || distance(column) < distance(best))) best = column;
  }
  return best?.id ?? null;
}

/** Runs a core query whenever `dependencies` change; a failure becomes a
 *  message instead of a crash. `run` returns `undefined` when there is
 *  nothing to ask.
 *
 *  At most one call per view is in flight: requests made meanwhile replace
 *  each other and only the newest is sent when it ends, so typing into a field
 *  never queues stale work in the core's single worker. The last answer stays
 *  on screen until the new one arrives, so `run` should bundle the inputs the
 *  view needs to draw it (sectors, columns, units) with the core's result. */
export function useQuery<T>(run: () => Promise<T> | undefined, dependencies: unknown[]): { data?: T; error?: string } {
  const [state, setState] = useState<{ data?: T; error?: string }>({});
  const newest = useRef(run);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    newest.current = run;
    if (inFlight.current) return; // the running loop picks the newest request up
    inFlight.current = true;
    (async () => {
      while (mounted.current) {
        const job = newest.current;
        const pending = job();
        let outcome: { data?: T; error?: string } = {};
        if (pending !== undefined) {
          try {
            outcome = { data: await pending };
          } catch (thrown) {
            outcome = { error: errorMessage(thrown) };
          }
        }
        if (!mounted.current) break;
        if (newest.current === job) {
          setState(outcome);
          break;
        }
        // Superseded while it ran: loop to send the newest request.
      }
      inFlight.current = false;
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, dependencies);
  return state;
}

/** Room a rotated axis name takes at the chart's left edge, px: half its line and a margin. */
const SIDE_NAME_EDGE = 16;
/** Gap of a side axis name when the grid's left margin is not a pixel count. */
const SIDE_NAME_GAP = 40;

interface AxisOption {
  name?: string;
  nameLocation?: string;
  position?: string;
}

/**
 * ECharts sets a y-axis name above the axis, where a top legend runs over
 * it. With a legend, an unplaced name of the left axis goes beside it, as the
 * original's axis titles, at the outer edge of the grid's left margin (clear
 * of the tick labels inside it). A right-hand axis keeps its place.
 */
function withSideAxisName(option: echarts.EChartsCoreOption): echarts.EChartsCoreOption {
  const { legend, yAxis, grid } = option as { legend?: unknown; yAxis?: AxisOption | AxisOption[]; grid?: { left?: unknown } };
  if (!legend || !yAxis) return option;
  const nameGap = typeof grid?.left === 'number' ? grid.left - SIDE_NAME_EDGE : SIDE_NAME_GAP;
  const side = (axis: AxisOption, index: number) =>
    axis.name && axis.nameLocation === undefined && index === 0 && axis.position !== 'right' ? { ...axis, nameLocation: 'middle', nameGap } : axis;
  return { ...option, yAxis: Array.isArray(yAxis) ? yAxis.map(side) : side(yAxis, 0) };
}

/** One ECharts chart that resizes with its box. */
export function Plot({ option, height = DEFAULT_CHART_HEIGHT }: { option: echarts.EChartsCoreOption; height?: number }) {
  const element = useRef<HTMLDivElement>(null);
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
  useEffect(() => {
    chart.current!.setOption(withSideAxisName(option), { notMerge: true });
  }, [option]);
  return <div ref={element} className="chart" style={{ height }} />;
}

export function ColumnSelect(props: {
  label: string;
  dataset: DatasetDto;
  value: string | null;
  onChange: (columnId: string | null) => void;
  only?: (column: ColumnDto) => boolean;
  /** Label of an extra "no column" choice. */
  none?: string;
}) {
  const view = useColumnView(props.dataset);
  // The dataset's shown columns in display order, and the chosen one even when hidden.
  const listed = view.shown().concat(props.dataset.columns.filter((column) => column.id === props.value && !view.visible(column)));
  return (
    <label>
      {props.label}
      <select value={props.value ?? ''} onChange={(event) => props.onChange(event.target.value === '' ? null : event.target.value)}>
        {props.none && <option value="">{props.none}</option>}
        {listed.map((column) =>
          !props.only || props.only(column) ? (
            <option key={column.id} value={column.id}>
              {withUnits(view.name(column), column)}
            </option>
          ) : null,
        )}
      </select>
    </label>
  );
}

const ErrorNote = ({ message }: { message: string }) => <div className="note err">{message}</div>;

/** What the window around a view puts in its control column: the dataset
 *  selector (top) and the "Filtrar por" group (bottom). */
export const WindowSlots = createContext<{ dataset?: ReactNode; filter?: ReactNode; exportRecipe?: (recipe: RecipeRequest) => void }>({});

/** What "Exportar receita" needs of a view: the analysis, as the core's
 *  dispatch names it, and the request the view sent. */
export interface RecipeRequest {
  analysis: string;
  request: object;
}

type Format = 'graph' | 'table';

/** The common anatomy of an analysis window (ESPEC-INTERFACE §3.1). `graph`
 *  and `table` are the two formats; a missing one disables its button. */
export function AnalysisLayout(props: {
  controls: ReactNode;
  facts?: ReactNode;
  error?: string;
  graph?: ReactNode;
  table?: ReactNode;
  note?: ReactNode;
  /** The request behind the output, for "Exportar receita"; `null` while there is none. */
  recipe?: RecipeRequest | null;
}) {
  const slots = useContext(WindowSlots);
  // The graph first, as in the reference product; a view without one shows its table.
  const [format, setFormat] = useState<Format>('graph');
  const shown = format === 'graph' ? props.graph ?? props.table : props.table ?? props.graph;
  return (
    <div className="analysis">
      <aside className="analysis-controls">
        {slots.dataset}
        <div className="format" role="group" aria-label="Formato">
          <span>Formato</span>
          <button className={format === 'graph' ? 'on' : ''} disabled={props.graph === undefined} onClick={() => setFormat('graph')}>
            Gráfico
          </button>
          <button className={format === 'table' ? 'on' : ''} disabled={props.table === undefined} onClick={() => setFormat('table')}>
            Tabela
          </button>
        </div>
        <div className="controls">{props.controls}</div>
        {slots.filter}
        {slots.exportRecipe && (
          <button
            className="btn"
            disabled={!props.recipe || !!props.error}
            title="Salva um JSON com o hash do arquivo, a configuração, as flags e edições, o pedido, o resultado e a versão do núcleo: 'wind replay' refaz e confere número a número."
            onClick={() => props.recipe && slots.exportRecipe!(props.recipe)}
          >
            Exportar receita
          </button>
        )}
      </aside>
      <section className="analysis-output">
        {props.error && <ErrorNote message={props.error} />}
        {props.facts}
        {props.note}
        {shown}
      </section>
    </div>
  );
}

/** What the turbulence window shows the TI against. */
type TurbulenceAxis = TurbulenceVersusDto['kind'];
const TURBULENCE_AXIS_LABEL: Record<TurbulenceAxis, string> = {
  speed: 'Velocidade do vento',
  direction: 'Direção do vento',
  month: 'Mês',
  hour: 'Hora do dia',
  height: 'Altura',
};
const IEC_EDITION_LABEL: Record<IecEditionDto, string> = { ed4: '4ª edição (2019)', ed3: '3ª edição (2005)', ed2: '2ª edição (1999)' };
/** Choice of every sensor at once in the sensor list. */
const ALL_SENSORS = '';
/** Digits of a turbulence intensity. */
const TI_DIGITS = 3;
const DEFAULT_TURBULENCE_SECTORS = 12;
/** A NTM curve is drawn thinner and dashed than the site's lines. */
const NTM_LINE = { width: 1, type: 'dashed' } as const;

/** Whether the dataset has a TI column computed from speed column `speed`. */
const hasTurbulence = (dataset: DatasetDto, speed: ColumnDto) =>
  dataset.columns.some((column) => column.kind === 'turbulence_intensity' && column.derivedFrom?.includes(speed.id));

function TurbulenceView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const sensors = dataset.columns.filter((column) => isMeanSpeed(column) && hasTurbulence(dataset, column));
  const [axis, setAxis] = useState<TurbulenceAxis>('speed');
  const [sensorId, setSensorId] = useState<string>(() => sensors[0]?.id ?? ALL_SENSORS);
  const [binWidth, setBinWidth] = useState(DEFAULT_BIN_WIDTH);
  const [directionId, setDirectionId] = useState<string | null>(() => firstId(dataset, isMeanDirection));
  const [sectors, setSectors] = useState(DEFAULT_TURBULENCE_SECTORS);
  const [edition, setEdition] = useState<IecEditionDto>('ed4');
  const speeds = sensorId === ALL_SENSORS || axis === 'height' ? sensors.map((column) => column.id) : [sensorId];
  const versus: TurbulenceVersusDto | null =
    axis === 'speed'
      ? { kind: 'speed', binWidth }
      : axis === 'direction'
        ? directionId == null
          ? null
          : { kind: 'direction', direction: directionId, sectors }
        : { kind: axis };
  const request = versus == null || !speeds.length ? null : { dataset: dataset.id, speeds, versus, edition, filter };
  const { data: answer, error } = useQuery(
    () => (request == null ? undefined : api.turbulence(request)),
    [api, result, speeds.join('\n'), axis, binWidth, directionId, sectors, edition, filter],
  );
  const figureName = (answer?.edition ?? edition) === 'ed2' ? 'TI característica' : 'TI representativa';
  const binLabel = (center: number | null) => {
    if (center == null) return NO_VALUE;
    switch (axis) {
      case 'speed':
        return `${formatNumber(center - binWidth / 2)}–${formatNumber(center + binWidth / 2)} m/s`;
      case 'direction':
        return sectorLabels(sectors)[Math.round((center * sectors) / DEGREES_PER_TURN)];
      case 'month':
        return MONTH_ABBREVIATIONS[center - 1];
      case 'hour':
        return hourRange(center);
      case 'height':
        return `${formatNumber(center)} m`;
    }
  };
  const option = useMemo(() => {
    if (!answer) return null;
    const bySensor = speeds.map((sensor) => ({ sensor, bins: answer.bins.filter((bin) => bin.sensor === sensor) }));
    const tooltip = { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value, TI_DIGITS) };
    if (axis === 'height') {
      const points = (pick: (bin: TurbulenceBinDto) => number | null) => answer.bins.map((bin) => [pick(bin), bin.center]);
      return {
        animation: false,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
        legend: { top: 4 },
        grid: { left: 56, right: 24, top: 40, bottom: 40 },
        xAxis: { type: 'value', scale: true, name: 'TI', nameLocation: 'middle', nameGap: 24 },
        yAxis: { type: 'value', name: 'Altura (m)', min: 0 },
        series: [
          { type: 'line', name: 'TI média', data: points((bin) => bin.mean), symbolSize: 8 },
          { type: 'line', name: figureName, data: points((bin) => bin.figure), symbolSize: 8, lineStyle: { type: 'dashed' } },
        ],
      };
    }
    const siteSeries = bySensor.flatMap(({ sensor, bins }) => {
      const label = speeds.length > 1 ? ` · ${sensor}` : '';
      const data = (pick: (bin: TurbulenceBinDto) => number | null) =>
        axis === 'speed' ? bins.map((bin) => [bin.center, pick(bin)]) : bins.map((bin) => pick(bin));
      return [
        { type: 'line', name: `TI média${label}`, data: data((bin) => bin.mean), connectNulls: false },
        { type: 'line', name: `${figureName}${label}`, data: data((bin) => bin.figure), connectNulls: false, lineStyle: { width: 3 } },
      ];
    });
    const ntmSeries = answer.ntm.map((curve) => ({
      type: 'line',
      name: `IEC ${curve.category}`,
      data: answer.ntmSpeeds.map((speed, index) => [speed, curve.ti[index]]),
      showSymbol: false,
      lineStyle: NTM_LINE,
    }));
    const categories =
      axis === 'direction' ? sectorLabels(sectors) : axis === 'month' ? MONTH_ABBREVIATIONS : axis === 'hour' ? HOUR_LABELS : undefined;
    return {
      animation: false,
      tooltip,
      legend: { top: 4, type: 'scroll' },
      grid: { left: 56, right: 24, top: 40, bottom: 40 },
      xAxis: categories ? { type: 'category', data: categories } : { type: 'value', name: 'm/s', nameLocation: 'middle', nameGap: 24, min: 0 },
      yAxis: { type: 'value', name: 'TI', min: 0 },
      series: [...siteSeries, ...ntmSeries],
    };
  }, [answer, axis, sectors, figureName, speeds.join('\n')]);
  const facts = answer && answer.at15.length > 0 && (
    <div className="facts">
      {answer.at15.map((sensor) => (
        <span key={sensor.sensor}>
          {sensor.sensor} a 15 m/s: <b>{formatNumber(sensor.mean, TI_DIGITS)}</b> média · {figureName} <b>{formatNumber(sensor.figure, TI_DIGITS)}</b> · categoria IEC{' '}
          <b>{sensor.category ?? NO_VALUE}</b> ({formatCount(sensor.count)} passos)
        </span>
      ))}
    </div>
  );
  const table = answer && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>{TURBULENCE_AXIS_LABEL[axis]}</th>
            {speeds.length > 1 && axis !== 'height' && <th>Sensor</th>}
            {axis === 'height' && <th>Sensor</th>}
            <th className="r">Passos</th>
            <th className="r">TI média</th>
            <th className="r">DP da TI</th>
            <th className="r">TI de pico</th>
            <th className="r">{figureName}</th>
          </tr>
        </thead>
        <tbody>
          {answer.bins.map((bin, index) => (
            <tr key={index}>
              <td>{binLabel(bin.center)}</td>
              {(speeds.length > 1 || axis === 'height') && <td>{bin.sensor}</td>}
              <td className="r">{formatCount(bin.count)}</td>
              <td className="r">{formatNumber(bin.mean, TI_DIGITS)}</td>
              <td className="r">{formatNumber(bin.deviation, TI_DIGITS)}</td>
              <td className="r">{formatNumber(bin.peak, TI_DIGITS)}</td>
              <td className="r">{formatNumber(bin.figure, TI_DIGITS)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'turbulence', request: request }}
      controls={
        <>
          <label>
            Exibir a turbulência versus
            <select value={axis} onChange={(event) => setAxis(event.target.value as TurbulenceAxis)}>
              {Object.entries(TURBULENCE_AXIS_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {axis !== 'height' && (
            <label>
              Sensor de velocidade
              <select value={sensorId} onChange={(event) => setSensorId(event.target.value)}>
                {sensors.map((column) => (
                  <option key={column.id} value={column.id}>
                    {columnLabel(column)}
                  </option>
                ))}
                {sensors.length > 1 && <option value={ALL_SENSORS}>Todos os sensores</option>}
              </select>
            </label>
          )}
          {axis === 'speed' && (
            <label>
              Largura da classe (m/s)
              <input type="number" min={0.1} step={0.5} value={binWidth} onChange={(event) => setBinWidth(Number(event.target.value) || DEFAULT_BIN_WIDTH)} />
            </label>
          )}
          {axis === 'direction' && (
            <>
              <ColumnSelect label="Direção" dataset={dataset} value={directionId} onChange={setDirectionId} only={isMeanDirection} />
              <label>
                Setores
                <select value={sectors} onChange={(event) => setSectors(Number(event.target.value))}>
                  {SHEAR_SECTOR_CHOICES.map((choice) => (
                    <option key={choice}>{choice}</option>
                  ))}
                </select>
              </label>
            </>
          )}
          <label>
            IEC 61400-1
            <select value={edition} onChange={(event) => setEdition(event.target.value as IecEditionDto)}>
              {Object.entries(IEC_EDITION_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <p className="hint">
            TI = DP ÷ média de cada passo, a partir de 0,01 m/s como no original (ou de 4 m/s, o corte da IEC, quando Configurar dataset › Propriedades
            básicas o pede). {figureName} = média{' '}
            {edition === 'ed2' ? '+ DP' : '+ 1,28 × DP (quantil de 90 %)'} da classe. A categoria compara a classe de 14,5–15,5 m/s com o modelo normal de
            turbulência (NTM) de cada categoria; acima de todas, S.
          </p>
        </>
      }
      facts={facts || undefined}
      error={!sensors.length ? 'Nenhuma velocidade média com coluna de desvio padrão associada: a turbulência precisa dela.' : error}
      graph={option ? <Plot option={option} /> : undefined}
      table={table || undefined}
    />
  );
}

type PeakKind = PeakMethodDto['kind'];
const PEAK_METHOD_LABEL: Record<PeakKind, string> = { periodic_maxima: 'Máximos periódicos', independent_storms: 'Tempestades independentes' };
const EXTREME_PERIOD_LABEL: Record<ExtremePeriodDto, string> = { month: 'Mês', year: 'Ano' };
const GUMBEL_FIT_LABEL: Record<GumbelFitDto, string> = { max_likelihood: 'Máxima verossimilhança', gringorten: 'Mínimos quadrados (Gringorten)' };
/** Defaults of the reference product: periods at least half complete, storms 48 h apart. */
const DEFAULT_MIN_RECOVERY_PERCENT = 50;
const DEFAULT_INDEPENDENCE_HOURS = 48;
/** Digits of a Gumbel parameter. */
const GUMBEL_DIGITS = 3;
const EXTREME_PEAKS_HEIGHT = 200;
/** Speed columns an extreme analysis reads: the means, and their maxima and gusts. */
const isExtremeCandidate = (column: ColumnDto) => column.kind === 'wind_speed' && ['mean', 'max', 'gust'].includes(column.subtype);

function ExtremesView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [columnId, setColumnId] = useState<string | null>(() => firstId(dataset, isMeanSpeed));
  const [kind, setKind] = useState<PeakKind>('periodic_maxima');
  const [period, setPeriod] = useState<ExtremePeriodDto>('month');
  const [minRecovery, setMinRecovery] = useState(DEFAULT_MIN_RECOVERY_PERCENT);
  const [threshold, setThreshold] = useState('');
  const [independenceHours, setIndependenceHours] = useState(DEFAULT_INDEPENDENCE_HOURS);
  const [fit, setFit] = useState<GumbelFitDto>('max_likelihood');
  const [confidence, setConfidence] = useState(false);
  const typedThreshold = typedNumber(threshold);
  const method: PeakMethodDto =
    kind === 'periodic_maxima'
      ? { kind, period, minRecovery: minRecovery / PERCENT }
      : { kind, threshold: typedThreshold !== undefined && Number.isFinite(typedThreshold) ? typedThreshold : undefined, independenceHours };
  const request = columnId == null ? null : { dataset: dataset.id, column: columnId, method, fit, confidence, filter };
  const { data: answer, error } = useQuery(
    () => (request == null ? undefined : api.extremes(request)),
    [api, result, columnId, kind, period, minRecovery, typedThreshold, independenceHours, fit, confidence, filter],
  );
  const units = dataset.columns.find((column) => column.id === columnId)?.units ?? '';
  const option = useMemo(() => {
    if (!answer) return null;
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
      legend: { top: 4 },
      grid: { left: 56, right: 24, top: 40, bottom: 44 },
      xAxis: { type: 'log', name: 'Período de retorno (anos)', nameLocation: 'middle', nameGap: 26, min: 1, max: 200 },
      yAxis: { type: 'value', scale: true, name: units },
      series: [
        { type: 'line', name: 'Gumbel ajustado', data: answer.curve.map((level) => [level.years, level.value]), showSymbol: false },
        { type: 'scatter', name: 'Picos', data: answer.empirical.map((level) => [level.years, level.value]), symbolSize: 7 },
      ],
    };
  }, [answer, units]);
  const peaksOption = useMemo(() => {
    if (!answer) return null;
    return {
      animation: false,
      useUTC: true,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
      grid: { left: 56, right: 24, top: 16, bottom: 28 },
      xAxis: { type: 'time' },
      yAxis: { type: 'value', scale: true, name: units },
      series: [{ type: 'scatter', name: 'Picos', data: answer.peaks.map((peak) => [peak.timeMs, peak.value]), symbolSize: 6 }],
    };
  }, [answer, units]);
  const facts = answer && (
    <div className="facts">
      <span>
        Vento de {answer.designYears} anos <b>{formatWithInterval(answer.design, answer.interval)}</b> {units}
      </span>
      <span>
        Gumbel anual μ {formatNumber(answer.annual.mode, GUMBEL_DIGITS)} · β {formatNumber(answer.annual.scale, GUMBEL_DIGITS)}
      </span>
      <span>
        {formatCount(answer.peaks.length)} picos · {formatNumber(answer.perYear)} por ano
        {answer.threshold != null && ` · limiar ${formatNumber(answer.threshold)} ${units}`}
      </span>
    </div>
  );
  const table = answer && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th className="r">Período de retorno (anos)</th>
            <th className="r">Vento ({units})</th>
          </tr>
        </thead>
        <tbody>
          {answer.returnLevels.map((level) => (
            <tr key={level.years}>
              <td className="r">{formatNumber(level.years)}</td>
              <td className="r">{formatNumber(level.value)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <table className="grid">
        <thead>
          <tr>
            <th>Pico em</th>
            <th className="r">Valor ({units})</th>
            <th>{kind === 'periodic_maxima' ? 'Período' : 'Tempestade'}</th>
            {kind === 'periodic_maxima' && <th className="r">Recuperação</th>}
          </tr>
        </thead>
        <tbody>
          {answer.peaks.map((peak) => (
            <tr key={peak.timeMs}>
              <td>{formatDateTime(peak.timeMs)}</td>
              <td className="r">{formatNumber(peak.value)}</td>
              <td>
                {formatDateTime(peak.fromMs)} – {formatDateTime(peak.toMs)}
              </td>
              {kind === 'periodic_maxima' && <td className="r">{formatPercent(peak.recovery)}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'extremes', request: request }}
      controls={
        <>
          <ColumnSelect label="Coluna de velocidade" dataset={dataset} value={columnId} onChange={setColumnId} only={isExtremeCandidate} />
          <label>
            Método
            <select value={kind} onChange={(event) => setKind(event.target.value as PeakKind)}>
              {Object.entries(PEAK_METHOD_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {kind === 'periodic_maxima' ? (
            <>
              <label>
                Período
                <select value={period} onChange={(event) => setPeriod(event.target.value as ExtremePeriodDto)}>
                  {Object.entries(EXTREME_PERIOD_LABEL).map(([value, label]) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Recuperação mínima do período (%)
                <input type="number" min={0} max={PERCENT} step={5} value={minRecovery} onChange={(event) => setMinRecovery(Number(event.target.value))} />
              </label>
            </>
          ) : (
            <>
              <label>
                Limiar da tempestade ({units})
                <input inputMode="decimal" value={threshold} placeholder="Recomendado (~20 tempestades/ano)" onChange={(event) => setThreshold(event.target.value)} />
              </label>
              <label>
                Independência (h)
                <input type="number" min={0} step={1} value={independenceHours} onChange={(event) => setIndependenceHours(Number(event.target.value))} />
              </label>
            </>
          )}
          <label>
            Ajuste de Gumbel
            <select value={fit} onChange={(event) => setFit(event.target.value as GumbelFitDto)}>
              {Object.entries(GUMBEL_FIT_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="check">
            <input type="checkbox" checked={confidence} onChange={(event) => setConfidence(event.target.checked)} />
            Intervalo de confiança (95 %)
          </label>
          <p className="hint">
            Gumbel ajustado aos picos e anualizado (a moda anual soma β·ln r, com r picos por ano). Para a rajada de 50 anos, escolha a coluna de rajada ou
            de máximo. O intervalo reamostra os picos (bootstrap); com poucos picos ele não sai.
          </p>
        </>
      }
      facts={facts || undefined}
      error={error}
      graph={
        option && peaksOption ? (
          <>
            <Plot option={option} />
            <Plot option={peaksOption} height={EXTREME_PEAKS_HEIGHT} />
          </>
        ) : undefined
      }
      table={table || undefined}
    />
  );
}

/** Digits of a regression coefficient and of R². */
const REGRESSION_DIGITS = 4;
/** Points above which ECharts draws the scatter in its large-data mode. */
const LARGE_SCATTER = 2_000;

function ScatterView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const speeds = dataset.columns.filter(isMeanSpeed);
  const [xId, setXId] = useState<string | null>(() => speeds[1]?.id ?? dataset.columns[0]?.id ?? null);
  const [yId, setYId] = useState<string | null>(() => speeds[0]?.id ?? dataset.columns[1]?.id ?? null);
  const request = xId == null || yId == null ? null : { dataset: dataset.id, x: xId, y: yId, filter };
  const { data: answer, error } = useQuery(
    () => (request == null ? undefined : api.scatter(request)),
    [api, result, xId, yId, filter],
  );
  const label = (id: string | null) => {
    const column = dataset.columns.find((candidate) => candidate.id === id);
    return column ? columnLabel(column) : '';
  };
  const option = useMemo(() => {
    if (!answer) return null;
    const lowest = Math.min(...answer.xs);
    const highest = Math.max(...answer.xs);
    const line = answer.slope != null && answer.intercept != null ? [lowest, highest].map((x) => [x, answer.slope! * x + answer.intercept!]) : [];
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
      legend: { top: 4 },
      grid: { left: 64, right: 24, top: 40, bottom: 48 },
      xAxis: { type: 'value', scale: true, name: label(xId), nameLocation: 'middle', nameGap: 28 },
      // Beside the axis, not above it, where the legend runs.
      yAxis: { type: 'value', scale: true, name: label(yId), nameLocation: 'middle', nameGap: 44 },
      series: [
        { type: 'scatter', name: 'Passos', data: answer.xs.map((x, index) => [x, answer.ys[index]]), symbolSize: 3, large: answer.xs.length > LARGE_SCATTER },
        { type: 'line', name: 'Reta de mínimos quadrados', data: line, showSymbol: false },
      ],
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [answer]);
  const facts = answer && (
    <div className="facts">
      <span>
        {formatCount(answer.count)} passos{answer.stride > 1 && ` (desenhado 1 a cada ${formatCount(answer.stride)})`}
      </span>
      <span>
        y = <b>{formatNumber(answer.slope, REGRESSION_DIGITS)}</b> · x + <b>{formatNumber(answer.intercept, REGRESSION_DIGITS)}</b>
      </span>
      <span>
        R² <b>{formatNumber(answer.rSquared, REGRESSION_DIGITS)}</b>
      </span>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'scatter', request: request }}
      controls={
        <>
          <ColumnSelect label="Eixo x" dataset={dataset} value={xId} onChange={setXId} />
          <ColumnSelect label="Eixo y" dataset={dataset} value={yId} onChange={setYId} />
          <p className="hint">Passos com as duas colunas válidas. A reta e o R² usam todos eles; o gráfico desenha uma amostra regular no tempo.</p>
        </>
      }
      facts={facts || undefined}
      error={error}
      graph={option ? <Plot option={option} /> : undefined}
    />
  );
}

/** The tables of Analisar › Tabelas; only the monthly means for now. */
const TABLE_KINDS = { monthly: 'Médias mensais' } as const;

function TablesView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [columnId, setColumnId] = useState<string | null>(() => firstId(dataset, isMeanSpeed) ?? dataset.columns[0]?.id ?? null);
  const request = columnId == null ? null : { dataset: dataset.id, column: columnId, filter };
  const { data: answer, error } = useQuery(
    () => (request == null ? undefined : api.monthlyTable(request)),
    [api, result, columnId, filter],
  );
  const units = dataset.columns.find((column) => column.id === columnId)?.units ?? '';
  const option = useMemo(
    () =>
      answer && {
        animation: false,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value) },
        legend: { top: 4, type: 'scroll' },
        grid: { left: 56, right: 24, top: 40, bottom: 32 },
        xAxis: { type: 'category', data: MONTH_ABBREVIATIONS },
        yAxis: { type: 'value', scale: true, name: units },
        series: [
          ...answer.years.map((year, row) => ({ type: 'line', name: String(year), data: answer.means[row] })),
          { type: 'line', name: 'Média dos anos', data: answer.meanOfYears, lineStyle: { width: 3 } },
        ],
      },
    [answer, units],
  );
  const facts = answer && (
    <div className="facts">
      <span>
        Média das médias mensais (MoMM) <b>{formatNumber(answer.momm)}</b> {units}
      </span>
      <span>
        Variabilidade interanual <b>{answer.interannualVariability == null ? NO_VALUE : formatPercent(answer.interannualVariability)}</b>
      </span>
    </div>
  );
  const table = answer && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>Ano</th>
            {MONTH_ABBREVIATIONS.map((name) => (
              <th key={name} className="r">
                {name}
              </th>
            ))}
            <th className="r">Anual</th>
          </tr>
        </thead>
        <tbody>
          {answer.years.map((year, row) => (
            <tr key={year}>
              <td>{year}</td>
              {answer.means[row].map((mean, month) => (
                <td key={month} className="r" title={`Recuperação ${formatPercent(answer.recovery[row][month])}`}>
                  {formatNumber(mean)}
                </td>
              ))}
              <td className="r">
                <b>{formatNumber(answer.annual[row])}</b>
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <td>Média</td>
            {answer.meanOfYears.map((mean, month) => (
              <td key={month} className="r">
                <b>{formatNumber(mean)}</b>
              </td>
            ))}
            <td className="r">
              <b>{formatNumber(answer.momm)}</b>
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'table', request: request }}
      controls={
        <>
          <label>
            Tabela
            <select value="monthly" disabled title="As outras tabelas do original ainda não estão disponíveis">
              {Object.entries(TABLE_KINDS).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <ColumnSelect label="Coluna" dataset={dataset} value={columnId} onChange={setColumnId} />
          <p className="hint">
            Média de cada mês de cada ano; passe o mouse para ver a recuperação. A anual e a MoMM pesam os meses pelos dias e pedem os doze meses; a
            variabilidade interanual é o desvio padrão das médias anuais completas sobre a média delas.
          </p>
        </>
      }
      facts={facts || undefined}
      error={error}
      graph={option ? <Plot option={option} /> : undefined}
      table={table || undefined}
    />
  );
}

/** Digits of an R². */
const R_SQUARED_DIGITS = 4;
/** Colours of the four fits' curves, in the original's legend order. */
const FIT_COLORS: Record<WeibullMethod, string> = { max_likelihood: '#d64545', least_squares: '#7a7a7a', wasp: '#2a5cc0', openwind: '#e0a020' };

/** Analisar › Distribuição de velocidade: the four Weibull fits of one speed
 *  column side by side, over its histogram (crates/wind-core/src/analysis/distribution.rs). */
function DistributionView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [columnId, setColumnId] = useState<string | null>(() => firstId(dataset, isMeanSpeed) ?? null);
  /** Bin width as typed; empty = the width the fits' R² is measured on. */
  const [widthText, setWidthText] = useState('');
  const request = columnId == null ? null : { dataset: dataset.id, column: columnId, filter };
  const { data: answer, error } = useQuery(() => (request ? api.speedDistribution(request) : undefined), [api, result, columnId, filter]);
  const typedWidth = Number(widthText.replace(',', '.'));
  const width = widthText.trim() !== '' && typedWidth > 0 ? typedWidth : answer?.binWidth;
  const { data: histogram } = useQuery(
    () => (columnId == null || width == null ? undefined : api.histogram({ dataset: dataset.id, column: columnId, binWidth: width, filter })),
    [api, result, columnId, width, filter],
  );
  const option = useMemo(() => {
    if (!answer || !histogram) return null;
    const edge = (bin: number) => histogram.binStart + bin * histogram.binWidth;
    const cdf = (speed: number, k: number, a: number) => (speed <= 0 ? 0 : 1 - Math.exp(-((speed / a) ** k)));
    const curves = answer.fits
      .filter((fit) => fit.k != null && fit.a != null)
      .map((fit) => ({
        type: 'line',
        name: WEIBULL_METHOD_LABEL[fit.method],
        color: FIT_COLORS[fit.method],
        data: histogram.frequency.map((_, bin) => (cdf(edge(bin + 1), fit.k!, fit.a!) - cdf(edge(bin), fit.k!, fit.a!)) * PERCENT),
        smooth: true,
        showSymbol: false,
      }));
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => `${formatNumber(value)}%` },
      // One scrolling line: the five entries wrap in a narrow window and cover the axis name.
      legend: { top: 4, type: 'scroll' },
      grid: { left: 56, right: 24, top: 40, bottom: 40 },
      xAxis: { type: 'category', data: histogram.frequency.map((_, bin) => formatNumber(edge(bin))), name: 'm/s', nameLocation: 'middle', nameGap: 26 },
      yAxis: { type: 'value', name: '%' },
      series: [{ type: 'bar', name: 'Dados medidos', data: asPercent(histogram.frequency), barCategoryGap: '8%', color: '#9cc98a' }, ...curves],
    };
  }, [answer, histogram]);
  const table = answer && (
    <div className="tablewrap">
      <table className="grid distribution">
        <thead>
          <tr>
            <th>Algoritmo</th>
            <th className="r">k</th>
            <th className="r">A (m/s)</th>
            <th className="r">Média (m/s)</th>
            <th className="r" title={`Fração acima da média medida, ${formatNumber(answer.mean, PARAMETER_DIGITS)} m/s`}>
              Acima da média
            </th>
            <th className="r" title="Densidade de potência a 1,225 kg/m³">
              Potência (W/m²)
            </th>
            <th className="r" title="Ajuste ao histograma de frequências: perto de 1, bom ajuste">R²</th>
          </tr>
        </thead>
        <tbody>
          {answer.fits.map((fit) => (
            <tr key={fit.method} title={fit.error?.message}>
              <td>{WEIBULL_METHOD_LABEL[fit.method]}</td>
              <td className="r">{formatNumber(fit.k, PARAMETER_DIGITS)}</td>
              <td className="r">{formatNumber(fit.a, PARAMETER_DIGITS)}</td>
              <td className="r">{formatNumber(fit.mean, PARAMETER_DIGITS)}</td>
              <td className="r">{formatNumber(fit.aboveMean, PARAMETER_DIGITS)}</td>
              <td className="r">{formatNumber(fit.powerDensityWM2, 1)}</td>
              <td className="r">{formatNumber(fit.rSquared, R_SQUARED_DIGITS)}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <td>Dados medidos</td>
            <td colSpan={2}>({formatCount(answer.samples)} passos)</td>
            <td className="r">{formatNumber(answer.mean, PARAMETER_DIGITS)}</td>
            <td className="r">{formatNumber(answer.aboveMean, PARAMETER_DIGITS)}</td>
            <td className="r">{formatNumber(answer.powerDensityWM2, 1)}</td>
            <td />
          </tr>
        </tfoot>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'distribution', request }}
      controls={
        <>
          <ColumnSelect label="Sensor de velocidade" dataset={dataset} value={columnId} onChange={setColumnId} only={isMeanSpeed} />
          <label title="Só muda o histograma desenhado; os ajustes usam as velocidades, e o R² os seus próprios intervalos.">
            Largura do intervalo (m/s)
            <input type="number" min={0} step="any" placeholder={answer ? formatNumber(answer.binWidth) : ''} value={widthText} onChange={(event) => setWidthText(event.target.value)} />
          </label>
          <p className="hint">
            Os quatro algoritmos de Weibull lado a lado. A densidade de potência é a 1,225 kg/m³ e o R² compara cada curva com o histograma de
            frequências.
          </p>
        </>
      }
      facts={table || undefined}
      error={error}
      graph={option ? <Plot option={option} /> : undefined}
    />
  );
}

/** What the distortion window compares, in pt-BR. */
const DISTORTION_MEASURE_LABEL: Record<DistortionMeasureDto, string> = { ratio: 'Razão entre as velocidades', difference: 'Diferença entre as velocidades' };
/** All pairs at once, in the pair select. */
const ALL_PAIRS = -1;
/** Default distance within which two anemometers are a pair, m (the original's option). */
const DEFAULT_COLOCATION_M = 1;
/** Ticks of the direction axis, degrees. */
const DIRECTION_TICK_DEG = 30;
const FULL_TURN_DEG = 360;
/** Digits of a distortion or scatter factor. */
const FACTOR_DIGITS = 3;

/** Analisar › Distorção da torre: the ratio or difference of each pair of
 *  colocated anemometers by direction sector, with the tower distortion and
 *  scatter factors (crates/wind-core/src/analysis/distortion.rs). */
function DistortionView({ api, result, filter, booms = {} }: Props) {
  const dataset = result.dataset;
  const [measure, setMeasure] = useState<DistortionMeasureDto>('ratio');
  const [colocationText, setColocationText] = useState(String(DEFAULT_COLOCATION_M));
  const [pairIndex, setPairIndex] = useState(ALL_PAIRS);
  const [polar, setPolar] = useState(true);
  const [display, setDisplay] = useState<DistortionDisplay>('sectors');
  const colocation = Number(colocationText.replace(',', '.'));
  const request = { dataset: dataset.id, measure, colocationM: Number.isFinite(colocation) && colocation >= 0 ? colocation : DEFAULT_COLOCATION_M, filter };
  const { data: answer, error } = useQuery(() => api.towerDistortion(request), [api, result, measure, colocationText, filter]);
  const pairName = (pair: { a: string; b: string }) => `${pair.a} × ${pair.b}`;
  const shown = answer ? (pairIndex === ALL_PAIRS ? answer.pairs : answer.pairs.slice(pairIndex, pairIndex + 1)) : [];
  const option = useMemo(() => {
    if (!answer || !shown.length) return null;
    const width = FULL_TURN_DEG / answer.sectors;
    const lines: { name: string; values: (number | null)[]; dashed?: boolean }[] =
      pairIndex === ALL_PAIRS
        ? shown.map((pair) => ({ name: pairName(pair), values: pair.sectors.map((sector) => sector.median) }))
        : [
            { name: 'Mediana', values: shown[0].sectors.map((sector) => sector.median) },
            { name: 'Percentil 25', values: shown[0].sectors.map((sector) => sector.lowerQuartile), dashed: true },
            { name: 'Percentil 75', values: shown[0].sectors.map((sector) => sector.upperQuartile), dashed: true },
          ];
    const series: object[] = lines.map(({ name, values, dashed }) => {
      const points = values.map((value, sector) => (polar ? [value, sector * width] : [sector * width, value]));
      return {
        type: 'line',
        name,
        coordinateSystem: polar ? 'polar' : 'cartesian2d',
        // The polar line closes on north.
        data: polar ? [...points, points[0]] : points,
        showSymbol: false,
        connectNulls: true,
        lineStyle: dashed ? { type: 'dashed' } : undefined,
      };
    });
    // Where each sensor of a single pair should be shaded: 180° from its boom, when known.
    if (pairIndex !== ALL_PAIRS) {
      const shaded = [shown[0].a, shown[0].b].flatMap((id) => (booms[id] == null ? [] : [{ id, at: (booms[id] + FULL_TURN_DEG / 2) % FULL_TURN_DEG }]));
      const values = lines.flatMap((line) => line.values).filter((value): value is number => value != null);
      const [low, high] = [Math.min(...values), Math.max(...values)];
      for (const { id, at } of shaded) {
        series.push({
          type: 'line',
          name: `Sombra esperada de ${id}`,
          coordinateSystem: polar ? 'polar' : 'cartesian2d',
          data: polar ? [[low, at], [high, at]] : [[at, low], [at, high]],
          showSymbol: false,
          lineStyle: { type: 'dotted', width: 2 },
        });
      }
    }
    const unit = measure === 'ratio' ? '' : ' (m/s)';
    const axes = polar
      ? {
          polar: { radius: '72%' },
          angleAxis: { type: 'value', min: 0, max: FULL_TURN_DEG, interval: DIRECTION_TICK_DEG, startAngle: 90, clockwise: true },
          // No axis name: on a polar chart it sits on north; the window's "Calcular" says what is drawn.
          radiusAxis: { type: 'value', scale: true },
        }
      : {
          grid: { left: 64, right: 24, top: 40, bottom: 44 },
          xAxis: { type: 'value', min: 0, max: FULL_TURN_DEG, interval: DIRECTION_TICK_DEG, name: 'Direção (°)', nameLocation: 'middle', nameGap: 26 },
          yAxis: { type: 'value', scale: true, name: `${DISTORTION_MEASURE_LABEL[measure]}${unit}` },
        };
    return { animation: false, tooltip: { ...SAFE_TOOLTIP, trigger: 'item' }, legend: { top: 4, type: 'scroll' }, ...axes, series };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [answer, pairIndex, polar, JSON.stringify(booms)]);
  const factors = answer && (
    <div className="tablewrap">
      <table className="grid distortion">
        <thead>
          <tr>
            <th>Par de anemômetros</th>
            <th>Direção</th>
            <th className="r">Ocorrências</th>
            <th className="r" title="Média ponderada de |mediana da razão − 1| nos 72 setores">
              Fator de distorção
            </th>
            <th className="r" title="Média ponderada do desvio padrão da razão dentro de cada setor">
              Fator de dispersão
            </th>
          </tr>
        </thead>
        <tbody>
          {answer.pairs.map((pair) => (
            <tr key={pairName(pair)}>
              <td className="id">{pairName(pair)}</td>
              <td className="id">{pair.direction}</td>
              <td className="r">{formatCount(pair.occurrences)}</td>
              <td className="r">{formatNumber(pair.distortionFactor, FACTOR_DIGITS)}</td>
              <td className="r">{formatNumber(pair.scatterFactor, FACTOR_DIGITS)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  const tablePair = answer?.pairs[pairIndex === ALL_PAIRS ? 0 : pairIndex];
  const table = answer && tablePair && (
    <div className="tablewrap">
      <p className="hint">Estatística por setor de {pairName(tablePair)}</p>
      <table className="grid">
        <thead>
          <tr>
            <th className="r">Setor (°)</th>
            <th className="r">Passos</th>
            <th className="r">Média</th>
            <th className="r">Mín.</th>
            <th className="r">Máx.</th>
            <th className="r">DP</th>
            <th className="r">Mediana</th>
          </tr>
        </thead>
        <tbody>
          {tablePair.sectors.map((sector, index) => (
            <tr key={index}>
              <td className="r">{formatNumber((index * FULL_TURN_DEG) / answer.sectors)}</td>
              <td className="r">{formatCount(sector.count)}</td>
              <td className="r">{formatNumber(sector.mean, FACTOR_DIGITS)}</td>
              <td className="r">{formatNumber(sector.min, FACTOR_DIGITS)}</td>
              <td className="r">{formatNumber(sector.max, FACTOR_DIGITS)}</td>
              <td className="r">{formatNumber(sector.deviation, FACTOR_DIGITS)}</td>
              <td className="r">{formatNumber(sector.median, FACTOR_DIGITS)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={{ analysis: 'distortion', request }}
      controls={
        <>
          <label title="Anemômetros a até esta diferença de altura formam um par.">
            Analisar sensores a até (m)
            <input type="number" min={0} step="any" value={colocationText} onChange={(event) => setColocationText(event.target.value)} />
          </label>
          <label>
            Calcular
            <select value={measure} onChange={(event) => setMeasure(event.target.value as DistortionMeasureDto)}>
              {Object.entries(DISTORTION_MEASURE_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Par de sensores
            <select value={pairIndex} onChange={(event) => setPairIndex(Number(event.target.value))}>
              <option value={ALL_PAIRS}>Todos</option>
              {answer?.pairs.map((pair, index) => (
                <option key={index} value={index}>
                  {pairName(pair)}
                </option>
              ))}
            </select>
          </label>
          {pairIndex !== ALL_PAIRS && (
            <label title="As exibições do original para um par; com a lança de cada sensor informada, a estatística por setor marca onde ele deveria estar à sombra.">
              Exibir
              <select value={display} onChange={(event) => setDisplay(event.target.value as DistortionDisplay)}>
                {Object.entries(DISTORTION_DISPLAY_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="check">
            <input type="checkbox" checked={polar} onChange={(event) => setPolar(event.target.checked)} />
            Gráfico polar
          </label>
          <p className="hint">
            A razão ou diferença de cada par de anemômetros na mesma altura em 72 setores de direção, com a direção mais próxima em altura. Os
            fatores usam sempre a razão.
          </p>
        </>
      }
      facts={answer && !answer.pairs.length ? <div className="note">Nenhum par de anemômetros na mesma altura com um sensor de direção.</div> : factors || undefined}
      error={error}
      graph={
        pairIndex !== ALL_PAIRS && display !== 'sectors' && answer?.pairs[pairIndex] ? (
          <DistortionDetailChart
            api={api}
            dataset={dataset.id}
            pair={answer.pairs[pairIndex]}
            measure={measure}
            filter={filter}
            display={display}
            polar={polar}
            axisName={DISTORTION_MEASURE_LABEL[measure]}
          />
        ) : option ? (
          <Plot option={option} />
        ) : undefined
      }
      table={table || undefined}
    />
  );
}

export function Analysis({ kind, ...props }: Props & { kind: AnalysisKind }) {
  switch (kind) {
    case 'coverage':
      return <CoverageView {...props} />;
    case 'histogram':
      return <HistogramView {...props} />;
    case 'distribution':
      return <DistributionView {...props} />;
    case 'rose':
      return <RoseView {...props} />;
    case 'diurnal':
      return <DiurnalView {...props} />;
    case 'shear':
      return <ShearView {...props} />;
    case 'turbulence':
      return <TurbulenceView {...props} />;
    case 'distortion':
      return <DistortionView {...props} />;
    case 'extremes':
      return <ExtremesView {...props} />;
    case 'scatter':
      return <ScatterView {...props} />;
    case 'tables':
      return <TablesView {...props} />;
  }
}

function CoverageView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const request = { dataset: dataset.id, filter };
  const { data: answer, error } = useQuery(
    async () => ({ coverage: await api.coverage(request), columns: dataset.columns }),
    [api, result, filter],
  );
  const option = useMemo(() => {
    if (!answer) return null;
    const { coverage, columns } = answer;
    const months = coverage.months.map((month) => `${MONTH_ABBREVIATIONS[month.month - 1]}/${String(month.year).slice(2)}`);
    const cells = columns.flatMap((_, row) => coverage.recovery[row].map((share, month) => [month, row, share == null ? null : share * PERCENT]));
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, valueFormatter: (value: number) => `${formatNumber(value)}%` },
      grid: { left: 140, right: 70, top: 16, bottom: 40 },
      xAxis: { type: 'category', data: months, splitArea: { show: true } },
      yAxis: { type: 'category', data: columns.map((column) => column.id), inverse: true },
      visualMap: { min: 0, max: PERCENT, calculable: false, orient: 'vertical', right: 0, top: 'center', text: ['100 %', '0 %'], inRange: { color: ['#fde2e1', '#f6c453', '#2f8f5b'] } },
      series: [{ type: 'heatmap', data: cells, label: { show: false } }],
    };
  }, [answer]);
  const cellStyle = (share: number | null) => ({
    background: share == null ? undefined : `color-mix(in srgb, var(--accent) ${Math.round(share * PERCENT)}%, transparent)`,
    color: share != null && share > DARK_CELL_SHARE ? 'var(--accent-text)' : undefined,
  });
  const table = answer && (
    <div className="tablewrap">
      <table className="cov grid">
        <thead>
          <tr>
            <th>Coluna</th>
            {answer.coverage.months.map((month) => (
              <th key={`${month.year}-${month.month}`} className="r">
                {MONTH_ABBREVIATIONS[month.month - 1]}/{String(month.year).slice(2)}
              </th>
            ))}
            <th className="r">Total</th>
          </tr>
        </thead>
        <tbody>
          {answer.columns.map((column, position) => (
            <tr key={column.id}>
              <td className="id">{column.id}</td>
              {answer.coverage.recovery[position].map((share, month) => (
                <td key={month} className="r" style={cellStyle(share)}>
                  {share == null ? '' : Math.round(share * PERCENT)}
                </td>
              ))}
              <td className="r">
                <b>{formatPercent(answer.coverage.total[position])}</b>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'coverage', request: request }}
      controls={<p className="hint">Parcela dos passos de cada mês com dado válido, já descontados os trechos marcados e os critérios de "Filtrar por".</p>}
      error={error}
      graph={option ? <Plot option={option} height={Math.max(DEFAULT_CHART_HEIGHT, answer!.columns.length * 26 + 60)} /> : undefined}
      table={table || undefined}
    />
  );
}

function HistogramView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [columnId, setColumnId] = useState<string | null>(() => firstId(dataset, isMeanSpeed) ?? dataset.columns[0]?.id ?? null);
  const [binWidth, setBinWidth] = useState(DEFAULT_BIN_WIDTH);
  const [method, setMethod] = useState<WeibullMethod>('max_likelihood');
  /** The k and A intervals cost a fraction of a second on a long record: only when asked. */
  const [confidence, setConfidence] = useState(false);
  const column = dataset.columns.find((candidate) => candidate.id === columnId);
  const isSpeed = column?.kind === 'wind_speed';
  const request = columnId == null ? null : { dataset: dataset.id, column: columnId, binWidth, weibull: isSpeed ? method : undefined, confidence, filter };
  const { data: answer, error } = useQuery(
    () =>
      request == null
        ? undefined
        : api
            .histogram(request)
            .then((histogram) => ({ histogram, units: column?.units, method })),
    [api, result, columnId, binWidth, method, isSpeed, confidence, filter],
  );
  const data = answer?.histogram;
  const option = useMemo(() => {
    if (!answer || !data) return null;
    const binEdges = data.frequency.map((_, bin) => formatNumber(data.binStart + bin * data.binWidth));
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => `${formatNumber(value)}%` },
      legend: { top: 4 },
      grid: { left: 56, right: 24, top: 40, bottom: 40 },
      xAxis: { type: 'category', data: binEdges, name: answer.units, nameLocation: 'middle', nameGap: 26 },
      yAxis: { type: 'value', name: '%' },
      series: [
        { type: 'bar', name: 'Medido', data: asPercent(data.frequency), barCategoryGap: '8%' },
        ...(data.weibull ? [{ type: 'line', name: 'Weibull', data: asPercent(data.weibull.frequency), smooth: true, showSymbol: false }] : []),
      ],
    };
  }, [answer, data]);
  const facts = data && (
    <div className="facts">
      <span>
        <b>N</b> {formatCount(data.samples)}
      </span>
      <span>
        <b>Média</b> {formatNumber(data.mean)}
      </span>
      {data.weibull && (
        <>
          <span>
            <b>k</b> {formatWithInterval(data.weibull.k, data.weibull.kInterval, PARAMETER_DIGITS)}
          </span>
          <span>
            <b>A</b> {formatWithInterval(data.weibull.a, data.weibull.aInterval, PARAMETER_DIGITS)}
          </span>
          <span>
            <b>Média Weibull</b> {formatNumber(data.weibull.mean)}
          </span>
          <span
            title={
              data.densityMeasured
                ? 'Com a densidade do ar medida a cada passo (1,225 kg/m³ onde falta)'
                : 'Com a densidade do ar padrão de 1,225 kg/m³: o dataset não tem temperatura'
            }
          >
            <b>Densidade de potência</b> {formatNumber(data.powerDensityWM2, 0)} W/m² · ρ {formatNumber(data.airDensityKgM3, DENSITY_DIGITS)} kg/m³{' '}
            {data.densityMeasured ? '(medida)' : '(padrão)'}
          </span>
          <span title="Fator de padrão de energia: média(u³)/média(u)³">
            <b>EPF</b> {formatNumber(data.energyPatternFactor, PARAMETER_DIGITS)}
          </span>
        </>
      )}
    </div>
  );
  const table = data && answer && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th className="r">#</th>
            <th className="r">De ({answer.units})</th>
            <th className="r">Até</th>
            <th className="r">Ocorrências</th>
            <th className="r">Frequência</th>
            {data.weibull && <th className="r">Weibull</th>}
          </tr>
        </thead>
        <tbody>
          {data.frequency.map((share, bin) => (
            <tr key={bin}>
              <td className="r">{bin + 1}</td>
              <td className="r">{formatNumber(data.binStart + bin * data.binWidth)}</td>
              <td className="r">{formatNumber(data.binStart + (bin + 1) * data.binWidth)}</td>
              <td className="r">{formatCount(Math.round(share * data.samples))}</td>
              <td className="r">{formatPercent(share)}</td>
              {data.weibull && <td className="r">{formatPercent(data.weibull.frequency[bin])}</td>}
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <td colSpan={3}>Total</td>
            <td className="r">{formatCount(data.samples)}</td>
            <td className="r">100%</td>
            {data.weibull && <td className="r">{WEIBULL_METHOD_LABEL[answer.method]}</td>}
          </tr>
        </tfoot>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'histogram', request: request }}
      controls={
        <>
          <ColumnSelect label="Coluna" dataset={dataset} value={columnId} onChange={setColumnId} />
          <label>
            Largura da classe
            <input type="number" step="any" min="0" value={binWidth} onChange={(event) => setBinWidth(Number(event.target.value))} />
          </label>
          <label>
            Ajuste de Weibull
            <select value={method} disabled={!isSpeed} onChange={(event) => setMethod(event.target.value as WeibullMethod)}>
              {Object.entries(WEIBULL_METHOD_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label
            className="check"
            title="Intervalo de 95 % de k e A por bootstrap de dias inteiros: respeita a autocorrelação dos dados de 10 minutos. Leva frações de segundo em registros longos."
          >
            <input type="checkbox" checked={confidence} disabled={!isSpeed} onChange={(event) => setConfidence(event.target.checked)} />
            Intervalo de confiança (95 %)
          </label>
        </>
      }
      error={error}
      facts={facts}
      graph={option ? <Plot option={option} /> : undefined}
      table={table || undefined}
    />
  );
}

type RoseMetric = 'frequency' | 'energy' | 'meanSpeed';
const ROSE_METRIC_LABEL: Record<RoseMetric, string> = {
  frequency: 'Frequência %',
  energy: 'Energia %',
  meanSpeed: 'Velocidade média',
};

function RoseView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [directionId, setDirectionId] = useState<string | null>(() => firstId(dataset, isMeanDirection));
  const [speedId, setSpeedId] = useState<string | null>(() => (directionId == null ? null : nearestSpeed(dataset, directionId)));
  const [sectors, setSectors] = useState(DEFAULT_ROSE_SECTORS);
  const [metric, setMetric] = useState<RoseMetric>('frequency');
  /** As typed; an empty or unreadable value means no calms. */
  const [calmThreshold, setCalmThreshold] = useState(String(DEFAULT_CALM_THRESHOLD));
  const calm = Number(calmThreshold.replace(',', '.')) || DEFAULT_CALM_THRESHOLD;
  const request = directionId == null ? null : { dataset: dataset.id, direction: directionId, speed: speedId ?? undefined, sectors, calmThreshold: calm, filter };
  const { data: answer, error } = useQuery(
    () =>
      request == null
        ? undefined
        : api
            .windRose(request)
            .then((rose) => ({ rose, sectors, hasSpeed: speedId != null })),
    [api, result, directionId, speedId, sectors, calm, filter],
  );
  const data = answer?.rose;
  const answeredSectors = answer?.sectors ?? sectors;
  const hasSpeed = answer?.hasSpeed ?? false;
  const labels = useMemo(() => sectorLabels(answeredSectors), [answeredSectors]);
  const option = useMemo(() => {
    if (!data) return null;
    const sectorWidth = DEGREES_PER_TURN / answeredSectors;
    const edges = data.speedEdges;
    const classNames = edges.map((edge, index) => `${index ? edges[index - 1] : 0}–${edge}`);
    classNames.push(`≥${edges[edges.length - 1]}`);
    const series =
      metric === 'frequency' && data.frequencyBySpeed.length
        ? data.frequencyBySpeed.map((shares, speedClass) => ({
            type: 'bar',
            coordinateSystem: 'polar',
            stack: 'speed classes',
            name: `${classNames[speedClass]} m/s`,
            data: asPercent(shares),
          }))
        : [{ type: 'bar', coordinateSystem: 'polar', name: ROSE_METRIC_LABEL[metric], data: metric === 'meanSpeed' ? data.meanSpeed : asPercent(data[metric]) }];
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item', valueFormatter: (value: number) => formatNumber(value) },
      legend: { top: 4, type: 'scroll' },
      polar: { radius: '70%', center: ['50%', '55%'] },
      // North at the top, sector 0 centred on it.
      angleAxis: { type: 'category', data: labels, startAngle: 90 + sectorWidth / 2, z: 10 },
      // The axis line would be drawn at the first sector's edge, not north: labels only.
      radiusAxis: { axisLine: { show: false }, axisTick: { show: false } },
      series,
    };
  }, [data, metric, answeredSectors, labels]);
  const table = data && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>Setor</th>
            <th className="r">Freq.</th>
            {hasSpeed && (
              <>
                <th className="r">Vel. média</th>
                <th className="r">Energia</th>
              </>
            )}
          </tr>
        </thead>
        <tbody>
          {labels.map((label, sector) => (
            <tr key={sector}>
              <td>{label}</td>
              <td className="r">{formatPercent(data.frequency[sector])}</td>
              {hasSpeed && (
                <>
                  <td className="r">{formatNumber(data.meanSpeed[sector])}</td>
                  <td className="r">{formatPercent(data.energy[sector])}</td>
                </>
              )}
            </tr>
          ))}
          {data.calm != null && data.calm > 0 && (
            <tr>
              <td>Calmaria</td>
              <td className="r">{formatPercent(data.calm)}</td>
              {hasSpeed && <td colSpan={2} />}
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
  if (directionId == null) return <AnalysisLayout
      recipe={request && { analysis: 'rose', request: request }} controls={null} error="Nenhuma coluna de direção do vento neste dataset." />;
  return (
    <AnalysisLayout
      controls={
        <>
          <ColumnSelect label="Sensor de direção" dataset={dataset} value={directionId} onChange={setDirectionId} only={(column) => column.kind === 'wind_direction'} />
          <ColumnSelect label="Velocidade" dataset={dataset} value={speedId} onChange={setSpeedId} only={(column) => column.kind === 'wind_speed'} none="Nenhuma" />
          <label>
            Setores
            <select value={sectors} onChange={(event) => setSectors(Number(event.target.value))}>
              {ROSE_SECTOR_CHOICES.map((choice) => (
                <option key={choice}>{choice}</option>
              ))}
            </select>
          </label>
          <label title="Abaixo deste valor o vento é calmaria e não entra em nenhum setor; a % de calmaria aparece na tabela.">
            Calmaria abaixo de (m/s)
            <input inputMode="decimal" value={calmThreshold} disabled={speedId == null} onChange={(event) => setCalmThreshold(event.target.value)} />
          </label>
          <label>
            Exibir
            <select value={metric} onChange={(event) => setMetric(event.target.value as RoseMetric)}>
              <option value="frequency">Frequência</option>
              <option value="energy" disabled={speedId == null}>
                Energia
              </option>
              <option value="meanSpeed" disabled={speedId == null}>
                Velocidade média
              </option>
            </select>
          </label>
        </>
      }
      error={error}
      note={
        data && (
          <p className="hint">
            {formatCount(data.samples)} passos com dado válido
            {data.calm != null && data.calm > 0 && ` · calmaria ${formatPercent(data.calm)} (com os setores, soma 100 %)`}.
          </p>
        )
      }
      graph={option ? <Plot option={option} height={ROSE_CHART_HEIGHT} /> : undefined}
      table={table || undefined}
    />
  );
}

function DiurnalView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const [columnId, setColumnId] = useState<string | null>(() => firstId(dataset, isMeanSpeed) ?? dataset.columns[0]?.id ?? null);
  const column = dataset.columns.find((candidate) => candidate.id === columnId);
  const request = columnId == null ? null : { dataset: dataset.id, column: columnId, filter };
  const { data: answer, error } = useQuery(
    () => (request == null ? undefined : api.diurnal(request).then((diurnal) => ({ diurnal, units: column?.units }))),
    [api, result, columnId, filter],
  );
  const data = answer?.diurnal;
  const option = useMemo(() => {
    if (!answer || !data) return null;
    const hasData = (month: number) => data.byMonth[month].some((mean) => mean != null);
    const monthsWithData = MONTH_ABBREVIATIONS.filter((_, month) => hasData(month));
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value) },
      // Months start hidden; the legend toggles them.
      legend: { top: 4, type: 'scroll', selected: Object.fromEntries(monthsWithData.map((name) => [name, false])) },
      grid: { left: 56, right: 24, top: 40, bottom: 32 },
      xAxis: { type: 'category', data: HOUR_LABELS },
      yAxis: { type: 'value', scale: true, name: answer.units },
      series: [
        { type: 'line', name: 'Todos os meses', data: data.all, lineStyle: { width: 3 } },
        ...MONTH_ABBREVIATIONS.flatMap((name, month) => (hasData(month) ? [{ type: 'line', name, data: data.byMonth[month], showSymbol: false }] : [])),
      ],
    };
  }, [answer, data]);
  const table = data && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>Hora</th>
            <th className="r">Todos os meses</th>
            {MONTH_ABBREVIATIONS.map((name) => (
              <th key={name} className="r">
                {name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {HOUR_LABELS.map((_, hour) => (
            <tr key={hour}>
              <td>{hourRange(hour)}</td>
              <td className="r">
                <b>{formatNumber(data.all[hour])}</b>
              </td>
              {MONTH_ABBREVIATIONS.map((name, month) => (
                <td key={name} className="r">
                  {formatNumber(data.byMonth[month][hour])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      recipe={request && { analysis: 'diurnal', request: request }}
      controls={
        <>
          <ColumnSelect label="Coluna" dataset={dataset} value={columnId} onChange={setColumnId} />
          <p className="hint">Média por hora do dia no relógio do arquivo; o intervalo h junta os passos que começam em [h, h+1). Clique nos meses da legenda para compará-los.</p>
        </>
      }
      error={error}
      graph={option ? <Plot option={option} /> : undefined}
      table={table || undefined}
    />
  );
}

function ShearView({ api, result, filter }: Props) {
  const dataset = result.dataset;
  const candidates = dataset.columns.filter((column) => isMeanSpeed(column) && column.heightM != null);
  const [selectedIds, setSelectedIds] = useState<string[]>(() => candidates.map((column) => column.id));
  const [directionId, setDirectionId] = useState<string | null>(() => firstId(dataset, isMeanDirection));
  const [sectors, setSectors] = useState(DEFAULT_SHEAR_SECTORS);
  const [aggregation, setAggregation] = useState<ShearAggregation>('ap_momm');
  const [display, setDisplay] = useState<ShearDisplay>('speed');
  const [subdivision, setSubdivision] = useState<Subdivision>('none');
  const profile = display === 'speed' && subdivision === 'none';
  const request = candidates.length < 2 || !profile ? null : { dataset: dataset.id, columns: selectedIds, direction: directionId ?? undefined, sectors, aggregation, filter };
  const { data: answer, error } = useQuery(
    () =>
      request == null ? undefined : api.shear(request).then((shear) => ({ shear, sectors, columns: selectedIds })),
    [api, result, selectedIds.join('\n'), directionId, sectors, aggregation, filter, profile],
  );
  // The fits through each pair of heights, by an aggregate-profile method (the original's third table).
  const aggregate = aggregation === 'ap_mean' || aggregation === 'ap_momm';
  const fitsRequest = request && aggregate ? { dataset: dataset.id, quantity: 'speed' as const, columns: selectedIds, aggregation, filter } : null;
  const { data: heightFits } = useQuery(() => (fitsRequest ? api.shearView(fitsRequest).then((view) => view.fits) : undefined), [api, result, JSON.stringify(fitsRequest)]);
  const data = answer?.shear;
  const charts = useMemo(() => {
    if (!answer || !data) return null;
    // Fitted power law through the centroid of (ln z, ln U), as the least-squares line is.
    const logHeights = data.heightsM.map(Math.log);
    const logSpeeds = data.meanSpeeds.map(Math.log);
    const average = (values: number[]) => values.reduce((total, value) => total + value, 0) / values.length;
    const [meanLogHeight, meanLogSpeed] = [average(logHeights), average(logSpeeds)];
    const [lowest, highest] = [Math.min(...data.heightsM), Math.max(...data.heightsM)];
    const alpha = data.alpha;
    const curve =
      alpha == null
        ? []
        : Array.from({ length: CURVE_POINTS }, (_, point) => {
            const from = lowest * CURVE_BELOW;
            const height = from + ((highest * CURVE_ABOVE - from) * point) / (CURVE_POINTS - 1);
            return [Math.exp(meanLogSpeed + alpha * (Math.log(height) - meanLogHeight)), height];
          });
    const common = { animation: false, grid: { left: 56, right: 24, top: 36, bottom: 40 } };
    return {
      profile: {
        ...common,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'item', valueFormatter: (value: number) => formatNumber(value) },
        legend: { top: 4 },
        xAxis: { type: 'value', scale: true, name: 'm/s', nameLocation: 'middle', nameGap: 26 },
        yAxis: { type: 'value', name: 'Altura (m)', min: 0 },
        series: [
          { type: 'scatter', name: 'Média medida', data: data.meanSpeeds.map((speed, index) => [speed, data.heightsM[index]]), symbolSize: 10 },
          { type: 'line', name: `Lei de potência α = ${formatNumber(alpha, PARAMETER_DIGITS)}`, data: curve, showSymbol: false },
        ],
      },
      byHour: {
        ...common,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value, PARAMETER_DIGITS) },
        xAxis: { type: 'category', data: HOUR_LABELS },
        yAxis: { type: 'value', name: 'α', scale: true },
        series: [{ type: 'line', name: 'α', data: data.alphaByHour }],
      },
      bySector: data.alphaBySector.length
        ? {
            ...common,
            tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value, PARAMETER_DIGITS) },
            xAxis: { type: 'category', data: sectorLabels(answer.sectors) },
            yAxis: { type: 'value', name: 'α', scale: true },
            series: [{ type: 'bar', name: 'α', data: data.alphaBySector }],
          }
        : null,
    };
  }, [answer, data]);
  const order = new Map(candidates.map((column, position) => [column.id, position]));
  const toggle = (columnId: string) =>
    setSelectedIds((current) =>
      current.includes(columnId) ? current.filter((id) => id !== columnId) : [...current, columnId].sort((first, second) => order.get(first)! - order.get(second)!),
    );
  const bySpeed = !['direction', 'hodograph', 'freq_direction'].includes(display);
  const sharedControls = (
    <>
      <label title="O que a janela mostra, como no Display do original.">
        Exibir
        <select value={display} onChange={(event) => setDisplay(event.target.value as ShearDisplay)}>
          {Object.entries(SHEAR_DISPLAY_LABEL).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {SUBDIVIDED_DISPLAYS.includes(display) && (
        <label>
          Subdividir por
          <select value={subdivision} onChange={(event) => setSubdivision(event.target.value as Subdivision)}>
            {Object.entries(SUBDIVISION_LABEL).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
      )}
      {!display.startsWith('freq_') && (
        <label title="AP-MoMM corrige o viés sazonal de um registro com mais de uma estação do que de outra. Os de série temporal ajustam cada passo e tiram a média ou a mediana.">
          Método de agregação
          <select value={aggregation} onChange={(event) => setAggregation(event.target.value as ShearAggregation)}>
            {Object.entries(SHEAR_AGGREGATION_LABEL).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
      )}
    </>
  );
  const speedPicker = bySpeed && (
    <fieldset className="columns">
      <legend>Colunas de velocidade</legend>
      <label className="check">
        <input
          type="checkbox"
          checked={selectedIds.length === candidates.length}
          onChange={(event) => setSelectedIds(event.target.checked ? candidates.map((column) => column.id) : [])}
        />
        Selecionar todas
      </label>
      {candidates.map((column) => (
        <label key={column.id} className="check">
          <input type="checkbox" checked={selectedIds.includes(column.id)} onChange={() => toggle(column.id)} />
          {column.id} · {formatNumber(column.heightM)} m
        </label>
      ))}
    </fieldset>
  );
  if (!profile) {
    return (
      <ShearDisplays
        api={api}
        dataset={dataset}
        filter={filter}
        display={display}
        subdivision={SUBDIVIDED_DISPLAYS.includes(display) ? subdivision : 'none'}
        aggregation={aggregation}
        speedIds={selectedIds}
        controls={
          <>
            {sharedControls}
            {speedPicker}
          </>
        }
      />
    );
  }
  if (candidates.length < 2) return <AnalysisLayout
      recipe={request && { analysis: 'shear', request: request }} controls={sharedControls} error="O cisalhamento precisa de pelo menos duas colunas de velocidade média com altura informada." />;
  const facts = data && (
    <div className="facts">
      <span>
        <b title="Entre parênteses, o intervalo de 95 % por bootstrap de dias inteiros">Expoente α (lei de potência)</b> {formatWithInterval(data.alpha, data.alphaInterval, PARAMETER_DIGITS)}
      </span>
      <span>
        <b>Rugosidade z₀ (lei log)</b> {data.roughnessLengthM == null ? NO_VALUE : `${formatNumber(data.roughnessLengthM, ROUGHNESS_DIGITS)} m`}
      </span>
      <span>
        <b>Passos simultâneos</b> {formatCount(data.samples)}
      </span>
      {data.displacementM > 0 && (
        <span title="As leis usam a altura de cada sensor menos a de deslocamento (Configurar dataset)">
          <b>Altura de deslocamento</b> {formatNumber(data.displacementM)} m
        </span>
      )}
    </div>
  );
  const graph = data && charts && (
    <div className="split">
      <Plot option={charts.profile} height={PROFILE_CHART_HEIGHT} />
      <div>
        <h3>α por hora do dia</h3>
        <Plot option={charts.byHour} height={SMALL_CHART_HEIGHT} />
        {charts.bySector && (
          <>
            <h3>α por setor de direção</h3>
            <Plot option={charts.bySector} height={SMALL_CHART_HEIGHT} />
          </>
        )}
      </div>
    </div>
  );
  const table = data && answer && (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>Coluna</th>
            <th className="r">Altura (m)</th>
            <th className="r">Velocidade (m/s)</th>
          </tr>
        </thead>
        <tbody>
          {data.heightsM.map((height, index) => (
            <tr key={index}>
              <td>{answer.columns[index]}</td>
              <td className="r">{formatNumber(height)}</td>
              <td className="r">{formatNumber(data.meanSpeeds[index], PARAMETER_DIGITS)}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <td colSpan={3}>
              {SHEAR_AGGREGATION_LABEL[data.aggregation]} · α {formatWithInterval(data.alpha, data.alphaInterval, PARAMETER_DIGITS)}
            </td>
          </tr>
        </tfoot>
      </table>
      {data.alphaBySector.length > 0 && (
        <table className="grid">
          <thead>
            <tr>
              <th>Setor</th>
              <th className="r">α</th>
            </tr>
          </thead>
          <tbody>
            {sectorLabels(answer.sectors).map((label, sector) => (
              <tr key={sector}>
                <td>{label}</td>
                <td className="r">{formatNumber(data.alphaBySector[sector], PARAMETER_DIGITS)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {aggregate && heightFits && heightFits.length > 0 && (
        <table className="grid heightfits">
          <thead>
            <tr>
              <th>Alturas do ajuste</th>
              <th className="r">α</th>
              <th className="r">z₀ (m)</th>
            </tr>
          </thead>
          <tbody>
            {heightFits.map((fit, position) => (
              <tr key={position}>
                <td>{HEIGHT_FIT_LABEL[fit.kind]}: {fit.columns.join(', ')}</td>
                <td className="r">{formatNumber(fit.alpha, PARAMETER_DIGITS)}</td>
                <td className="r">{formatNumber(fit.roughnessM, ROUGHNESS_DIGITS)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
  return (
    <AnalysisLayout
      controls={
        <>
          {sharedControls}
          <ColumnSelect label="Sensor de direção" dataset={dataset} value={directionId} onChange={setDirectionId} only={(column) => column.kind === 'wind_direction'} none="Nenhum" />
          <label>
            Setores
            <select value={sectors} disabled={directionId == null} onChange={(event) => setSectors(Number(event.target.value))}>
              {SHEAR_SECTOR_CHOICES.map((choice) => (
                <option key={choice}>{choice}</option>
              ))}
            </select>
          </label>
          {speedPicker}
        </>
      }
      error={error}
      facts={facts}
      note={
        data && (
          <p className="hint">
            Sobre os passos em que todas as alturas têm dado.{' '}
            {data.displacementM > 0
              ? `As leis usam a altura de cada sensor menos ${formatNumber(data.displacementM)} m de deslocamento; o gráfico mostra as alturas nominais.`
              : 'Alturas nominais: o dataset não tem altura de deslocamento (Configurar dataset).'}
          </p>
        )
      }
      graph={graph || undefined}
      table={table || undefined}
    />
  );
}

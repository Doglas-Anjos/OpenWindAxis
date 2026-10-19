/**
 * Flag › Sombra da torre (manual dlg-flag-flag-tower-shading.htm), laid out
 * as the original's window: the
 * dataset; "Sinalizar com base em" the boom orientation or the detected
 * shading pattern; the sensors (or the pairs of colocated anemometers) and
 * the details of the one chosen; its round plots; and the flag rules
 * generated, run as one undoable edit.
 *
 * - **Orientação da lança:** each speed sensor is shaded with the wind from
 *   opposite its boom (Configurar dataset › Lança, also editable here), in a
 *   sector of the chosen width, by the direction sensor chosen (the nearest
 *   in height by default). The plot is a compass with the boom and the
 *   shaded sector.
 * - **Padrão detectado:** for each pair, the median ratio A/B in 72 sectors
 *   (crates/wind-core/src/shading.rs): the lowest median centres A's
 *   shaded sector, the highest B's; centres and widths can be changed. The
 *   plots are the median ratio and every step's ratio against the direction,
 *   with both sectors drawn.
 */
import { useMemo, useState } from 'react';
import * as echarts from 'echarts/core';
import { CustomChart } from 'echarts/charts';
import { MarkAreaComponent, TitleComponent } from 'echarts/components';
import type { WindApi } from './api/client';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { DatasetId } from './api/generated/DatasetId';
import type { FlagRuleDto } from './api/generated/FlagRuleDto';
import type { ShadingPairDto } from './api/generated/ShadingPairDto';
import { Plot, SAFE_TOOLTIP, useQuery } from './Analyses';
import { errorMessage, formatCount, formatNumber } from './format';
import { boomsOf, isDerived, type Entry } from './workbook/model';
import type { Workbook } from './workbook/useWorkbook';

echarts.use([CustomChart, MarkAreaComponent, TitleComponent]);

const DEGREES_PER_TURN = 360;
const HALF_TURN = DEGREES_PER_TURN / 2;
/** The flag a new dataset reserves for tower shading (wind-core flags::TOWER_SHADOW_FLAG). */
const TOWER_SHADOW_FLAG_NAME = 'Sombra da torre';
/** The original's shaded sector width while the detection loads, degrees. */
const FALLBACK_WIDTH_DEG = 30;
/** A generated rule flags from the first step in the sector. */
const ONE_STEP = 1;
const CHART_HEIGHT = 300;
/** Fill of a shaded sector, per sensor of a pair (the original's yellow and green). */
const SHADE_A = 'rgba(240, 210, 60, 0.35)';
const SHADE_B = 'rgba(120, 200, 110, 0.35)';
const MEDIAN_COLOR = '#c62828';
const SCATTER_COLOR = 'rgba(198, 40, 40, 0.35)';
const BOOM_COLOR = '#333';
const SCATTER_POINT_SIZE = 3;
/** The ratio plot spans these quantiles: a near-zero B makes ratios in the hundreds. */
const LOW_QUANTILE = 0.01;
const HIGH_QUANTILE = 0.99;
/** Both ratio plots' radius runs between their extremes rounded out to this step. */
const RATIO_AXIS_STEP = 0.05;
/** Decimals the rounded bounds keep: 0.05 · 3 is 0.15000000000000002 otherwise. */
const RATIO_AXIS_DECIMALS = 2;

/** `low` to `high` widened to whole {@link RATIO_AXIS_STEP}s, so the axis labels read 0.9 or 1.15. */
const ratioBounds = (low: number, high: number) => ({
  min: Number((Math.floor(low / RATIO_AXIS_STEP) * RATIO_AXIS_STEP).toFixed(RATIO_AXIS_DECIMALS)),
  max: Number((Math.ceil(high / RATIO_AXIS_STEP) * RATIO_AXIS_STEP).toFixed(RATIO_AXIS_DECIMALS)),
});
/** Rows shown by the sensor and pair lists. */
const LIST_ROWS = 6;
const COMPASS = ['N', 'NE', 'L', 'SE', 'S', 'SO', 'O', 'NO'];
const COMPASS_STEP_DEG = 45;

/** `degrees` in [0, 360). */
const wrapped = (degrees: number) => ((degrees % DEGREES_PER_TURN) + DEGREES_PER_TURN) % DEGREES_PER_TURN;
/** The sector `center ± width/2` as text, e.g. "290° a 320°". */
const sectorText = (center: number, width: number) => `${formatNumber(wrapped(center - width / 2))}° a ${formatNumber(wrapped(center + width / 2))}°`;

const isMeanSpeed = (column: ColumnDto) => column.kind === 'wind_speed' && column.subtype === 'mean' && !isDerived(column);
const isMeanDirection = (column: ColumnDto) => column.kind === 'wind_direction' && column.subtype === 'mean';

/** The direction column nearest `column` in height. */
function nearestVane(dataset: DatasetDto, column: ColumnDto): string | undefined {
  const vanes = dataset.columns.filter(isMeanDirection);
  const distance = (vane: ColumnDto) => Math.abs((vane.heightM ?? 0) - (column.heightM ?? 0));
  return vanes.reduce<ColumnDto | undefined>((best, vane) => (best && distance(best) <= distance(vane) ? best : vane), undefined)?.id;
}

/** A shaded sector on a polar plot: a custom series drawing a wedge over the whole radius. */
function wedge(name: string, center: number, width: number, color: string) {
  // Canvas angles start east and turn clockwise; directions start north.
  const toCanvas = (direction: number) => ((direction - 90) * Math.PI) / HALF_TURN;
  return {
    type: 'custom',
    name,
    coordinateSystem: 'polar',
    silent: true,
    data: [[0, center]],
    z: 0,
    renderItem: (params: { coordSys: { cx: number; cy: number; r: number; r0: number } }) => ({
      type: 'sector',
      shape: { cx: params.coordSys.cx, cy: params.coordSys.cy, r0: params.coordSys.r0, r: params.coordSys.r, startAngle: toCanvas(center - width / 2), endAngle: toCanvas(center + width / 2), clockwise: true },
      style: { fill: color },
    }),
  };
}

/** The angle axis of the round plots: north up, clockwise, the compass marked. */
const ANGLE_AXIS = {
  type: 'value',
  min: 0,
  max: DEGREES_PER_TURN,
  interval: COMPASS_STEP_DEG,
  startAngle: 90,
  clockwise: true,
  axisLabel: { formatter: (value: number) => COMPASS[Math.round(value / COMPASS_STEP_DEG) % COMPASS.length] },
};

/** A shaded sector of a cartesian plot, as areas on a 0–360° axis (two across north). */
function cartesianAreas(center: number, width: number, color: string) {
  const [from, to] = [wrapped(center - width / 2), wrapped(center + width / 2)];
  const area = (start: number, end: number) => [{ xAxis: start, itemStyle: { color } }, { xAxis: end }];
  if (width >= DEGREES_PER_TURN) return [area(0, DEGREES_PER_TURN)];
  return from < to ? [area(from, to)] : [area(from, DEGREES_PER_TURN), area(0, to)];
}

/** What a sensor's rule needs (boom mode). */
interface SensorSettings {
  enabled: boolean;
  direction: string;
  width: number;
}

/** What a pair's rules need (pattern mode). */
interface PairSettings {
  enabled: boolean;
  direction: string;
  centerA: number;
  widthA: number;
  centerB: number;
  widthB: number;
}

/** A rule flagging `column` while `direction` is in `center ± width/2`. */
const sectorRule = (column: string, direction: string, center: number, width: number, flag: number): FlagRuleDto => ({
  criterion: { kind: 'direction_in_sector', direction, center: wrapped(center), width },
  columns: [column],
  flag,
  minSteps: ONE_STEP,
});

/** The compass of one sensor: its boom and the sector it is shaded in. */
function BoomPlot({ boom, width }: { boom: number | undefined; width: number }) {
  const option = useMemo(
    () => ({
      animation: false,
      polar: { radius: '78%' },
      angleAxis: ANGLE_AXIS,
      radiusAxis: { type: 'value', min: 0, max: 1, show: false },
      series:
        boom == null
          ? []
          : [
              wedge('À sombra', wrapped(boom + HALF_TURN), width, SHADE_A),
              { type: 'line', name: 'Lança', coordinateSystem: 'polar', data: [[0, boom], [1, boom]], showSymbol: false, lineStyle: { color: BOOM_COLOR, width: 3 } },
            ],
    }),
    [boom, width],
  );
  return <Plot option={option} height={CHART_HEIGHT} />;
}

/** The median ratio of a pair by sector, with both shaded sectors. */
function MedianPlot({ pair, sectors, settings, polar }: { pair: ShadingPairDto; sectors: number; settings: PairSettings; polar: boolean }) {
  const option = useMemo(() => {
    const step = DEGREES_PER_TURN / sectors;
    const title = { text: `Mediana da razão de ${pair.a} para ${pair.b}`, left: 'center', textStyle: { fontSize: 12 } };
    // A radius from 0 would flatten the pattern: it spans the medians, as the original's.
    const present = pair.medians.filter((median): median is number => median != null);
    const bounds = present.length ? ratioBounds(Math.min(...present), Math.max(...present)) : {};
    if (polar) {
      const points = pair.medians.map((median, sector) => [median, sector * step]);
      return {
        animation: false,
        title,
        tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
        polar: { radius: '72%', center: ['50%', '56%'] },
        angleAxis: ANGLE_AXIS,
        radiusAxis: { type: 'value', ...bounds },
        series: [
          wedge(pair.a, settings.centerA, settings.widthA, SHADE_A),
          wedge(pair.b, settings.centerB, settings.widthB, SHADE_B),
          { type: 'line', name: 'Mediana da razão', coordinateSystem: 'polar', data: [...points, points[0]], showSymbol: false, connectNulls: true, lineStyle: { color: MEDIAN_COLOR, width: 2 } },
        ],
      };
    }
    return {
      animation: false,
      title,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' },
      grid: { left: 56, right: 16, top: 36, bottom: 36 },
      xAxis: { type: 'value', min: 0, max: DEGREES_PER_TURN, interval: COMPASS_STEP_DEG, name: 'Direção (°)', nameLocation: 'middle', nameGap: 24 },
      yAxis: { type: 'value', ...bounds },
      series: [
        {
          type: 'line',
          name: 'Mediana da razão',
          data: pair.medians.map((median, sector) => [sector * step, median]),
          showSymbol: false,
          lineStyle: { color: MEDIAN_COLOR },
          markArea: { silent: true, data: [...cartesianAreas(settings.centerA, settings.widthA, SHADE_A), ...cartesianAreas(settings.centerB, settings.widthB, SHADE_B)] },
        },
      ],
    };
  }, [pair, sectors, settings, polar]);
  return <Plot option={option} height={CHART_HEIGHT} />;
}

/** Every step's ratio of a pair against the direction, with both shaded sectors. */
function RatioPlot({ api, dataset, pair, settings, polar }: { api: WindApi; dataset: DatasetId; pair: ShadingPairDto; settings: PairSettings; polar: boolean }) {
  const request = { dataset, a: pair.a, b: pair.b, direction: settings.direction, measure: 'ratio' as const };
  const { data, error } = useQuery(() => api.distortionDetail(request), [api, dataset, pair.a, pair.b, settings.direction]);
  const option = useMemo(() => {
    if (!data) return null;
    const sorted = [...data.pointValues].sort((first, second) => first - second);
    const [low, high] = sorted.length ? [sorted[Math.floor(LOW_QUANTILE * (sorted.length - 1))], sorted[Math.ceil(HIGH_QUANTILE * (sorted.length - 1))]] : [0, 1];
    const points = data.pointDirections.flatMap((direction, index) => {
      const value = data.pointValues[index];
      return value >= low && value <= high ? [polar ? [value, direction] : [direction, value]] : [];
    });
    const title = { text: `Razão de ${pair.a} para ${pair.b}`, left: 'center', textStyle: { fontSize: 12 } };
    const bounds = ratioBounds(low, high);
    const scatter = { type: 'scatter', name: 'Razão', data: points, symbol: 'circle', symbolSize: SCATTER_POINT_SIZE, large: true, itemStyle: { color: SCATTER_COLOR } };
    if (polar) {
      return {
        animation: false,
        title,
        polar: { radius: '72%', center: ['50%', '56%'] },
        angleAxis: ANGLE_AXIS,
        radiusAxis: { type: 'value', ...bounds },
        series: [wedge(pair.a, settings.centerA, settings.widthA, SHADE_A), wedge(pair.b, settings.centerB, settings.widthB, SHADE_B), { ...scatter, coordinateSystem: 'polar' }],
      };
    }
    return {
      animation: false,
      title,
      grid: { left: 56, right: 16, top: 36, bottom: 36 },
      xAxis: { type: 'value', min: 0, max: DEGREES_PER_TURN, interval: COMPASS_STEP_DEG, name: 'Direção (°)', nameLocation: 'middle', nameGap: 24 },
      yAxis: { type: 'value', ...bounds },
      series: [{ ...scatter, markArea: { silent: true, data: [...cartesianAreas(settings.centerA, settings.widthA, SHADE_A), ...cartesianAreas(settings.centerB, settings.widthB, SHADE_B)] } }],
    };
  }, [data, pair, settings, polar]);
  if (error) return <div className="note err">{error}</div>;
  return option ? <Plot option={option} height={CHART_HEIGHT} /> : null;
}

interface Props {
  api: WindApi;
  entries: Entry[];
  /** The dataset the window starts on (the explorer's selection); the window's own choice afterwards. */
  initialDataset: DatasetId | null;
  workbook: Workbook;
  disabled: boolean;
}

/** The Sombra da torre window. */
export function TowerShadingWindow({ api, entries, initialDataset, workbook, disabled }: Props) {
  const [chosen, setChosen] = useState<DatasetId | null>(initialDataset);
  const entry = entries.find((candidate) => candidate.result.dataset.id === chosen) ?? entries[0];
  if (!entry) return <div className="summary-empty">Adicione um dataset para usar esta janela.</div>;
  return <ShadingForDataset key={entry.result.dataset.id} api={api} entry={entry} entries={entries} onChoose={setChosen} workbook={workbook} disabled={disabled} />;
}

type Basis = 'boom' | 'pattern';

/** The window for one dataset; a new dataset starts it afresh. */
function ShadingForDataset({
  api,
  entry,
  entries,
  onChoose,
  workbook,
  disabled,
}: {
  api: WindApi;
  entry: Entry;
  entries: Entry[];
  onChoose: (id: DatasetId) => void;
  workbook: Workbook;
  disabled: boolean;
}) {
  const dataset = entry.result.dataset;
  const booms = boomsOf(entry);
  const reserved = dataset.flags.findIndex((flag) => flag.name === TOWER_SHADOW_FLAG_NAME);
  const [flag, setFlag] = useState(Math.max(reserved, 0));
  const [basis, setBasis] = useState<Basis>(() => (Object.keys(booms).length ? 'boom' : 'pattern'));
  const [polar, setPolar] = useState(true);
  const { data, error } = useQuery(() => api.towerShading({ dataset: dataset.id, flag }), [dataset, flag]);
  const defaultWidth = data?.defaultWidthDeg ?? FALLBACK_WIDTH_DEG;
  const speeds = dataset.columns.filter(isMeanSpeed);
  const [sensor, setSensor] = useState(() => speeds[0]?.id ?? '');
  const [sensorSettings, setSensorSettings] = useState<Record<string, SensorSettings>>({});
  const [pairIndex, setPairIndex] = useState(0);
  const [pairSettings, setPairSettings] = useState<Record<string, PairSettings>>({});
  const [running, setRunning] = useState(false);
  const [outcome, setOutcome] = useState('');

  const settingsOf = (id: string): SensorSettings => {
    const column = speeds.find((candidate) => candidate.id === id);
    return sensorSettings[id] ?? { enabled: booms[id] != null, direction: (column && nearestVane(dataset, column)) ?? '', width: defaultWidth };
  };
  const pairKey = (pair: ShadingPairDto) => `${pair.a}|${pair.b}`;
  const pairOf = (pair: ShadingPairDto): PairSettings =>
    pairSettings[pairKey(pair)] ?? {
      enabled: pair.shadedA != null || pair.shadedB != null,
      direction: pair.direction,
      centerA: pair.shadedA ?? 0,
      widthA: defaultWidth,
      centerB: pair.shadedB ?? 0,
      widthB: defaultWidth,
    };

  const generated: FlagRuleDto[] =
    basis === 'boom'
      ? speeds.flatMap((column) => {
          const settings = settingsOf(column.id);
          const boom = booms[column.id];
          return settings.enabled && boom != null && settings.direction ? [sectorRule(column.id, settings.direction, boom + HALF_TURN, settings.width, flag)] : [];
        })
      : (data?.pairs ?? []).flatMap((pair) => {
          const settings = pairOf(pair);
          if (!settings.enabled) return [];
          return [
            ...(pair.shadedA != null ? [sectorRule(pair.a, settings.direction, settings.centerA, settings.widthA, flag)] : []),
            ...(pair.shadedB != null ? [sectorRule(pair.b, settings.direction, settings.centerB, settings.widthB, flag)] : []),
          ];
        });
  const flagName = dataset.flags[flag]?.name ?? TOWER_SHADOW_FLAG_NAME;

  async function run() {
    setRunning(true);
    setOutcome('');
    try {
      const answer = await api.flagByRules({ dataset: dataset.id, rules: generated });
      workbook.showEdit(entry, Promise.resolve(answer.state));
      const steps = answer.tallies.reduce((total, tally) => total + tally.steps, 0);
      setOutcome(`${formatCount(answer.found.length)} trechos, ${formatCount(steps)} passos marcados.`);
      const known = new Set(entry.rules.map((rule) => JSON.stringify(rule)));
      const added = generated.filter((rule) => !known.has(JSON.stringify(rule)));
      if (added.length) workbook.updateEntry(dataset.id, (latest) => ({ rules: [...latest.rules, ...added] }));
    } catch (thrown) {
      setOutcome(errorMessage(thrown));
    }
    setRunning(false);
  }

  const directionSelect = (value: string, onChange: (id: string) => void) => (
    <label>
      Sensor de direção para as regras
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {dataset.columns.filter(isMeanDirection).map((column) => (
          <option key={column.id} value={column.id}>
            {column.id}
          </option>
        ))}
      </select>
    </label>
  );
  const number = (value: number, onChange: (value: number) => void, label: string) => (
    <input className="num" type="number" step="any" aria-label={label} value={value} onChange={(event) => Number.isFinite(event.target.valueAsNumber) && onChange(event.target.valueAsNumber)} />
  );

  const boomDetails = () => {
    const column = speeds.find((candidate) => candidate.id === sensor);
    if (!column) return <p className="note">O dataset não tem velocidade média medida.</p>;
    const settings = settingsOf(column.id);
    const update = (patch: Partial<SensorSettings>) => setSensorSettings({ ...sensorSettings, [column.id]: { ...settings, ...patch } });
    const position = dataset.columns.indexOf(column);
    const boom = booms[column.id];
    return (
      <div className="shading-details">
        <fieldset>
          <legend>Detalhes do sensor</legend>
          <label className="check">
            <input type="checkbox" checked={settings.enabled} disabled={boom == null} onChange={(event) => update({ enabled: event.target.checked })} />
            Gerar regra para este sensor
          </label>
          <label title="Graus a partir do norte; o sensor fica à sombra com o vento do lado oposto, 180° dela. É a mesma Lança (°) do Configurar dataset.">
            Orientação da lança (°)
            <input
              className="num"
              type="number"
              step="any"
              min={0}
              max={DEGREES_PER_TURN}
              value={boom ?? ''}
              placeholder="não informada"
              onChange={(event) => {
                const value = event.target.value === '' ? undefined : wrapped(event.target.valueAsNumber);
                workbook.setCorrection(entry, position, { boomDeg: value });
                if (value != null) update({ enabled: true });
              }}
            />
          </label>
          {directionSelect(settings.direction, (direction) => update({ direction }))}
          <label>
            Largura do setor sombreado (°){number(settings.width, (width) => update({ width }), `Largura do setor de ${column.id}`)}
          </label>
          {boom != null && <p className="hint">À sombra com o vento de {sectorText(boom + HALF_TURN, settings.width)}.</p>}
        </fieldset>
        <BoomPlot boom={boom} width={settings.width} />
      </div>
    );
  };

  const pairDetails = () => {
    if (!data) return null;
    const pair = data.pairs[pairIndex];
    if (!pair) return <p className="note">Nenhum par de anemômetros na mesma altura com um sensor de direção: não há padrão de sombra a detectar.</p>;
    const settings = pairOf(pair);
    const update = (patch: Partial<PairSettings>) => setPairSettings({ ...pairSettings, [pairKey(pair)]: { ...settings, ...patch } });
    const sectorRow = (sensorId: string, shaded: number | null, center: number, width: number, onCenter: (value: number) => void, onWidth: (value: number) => void, color: string) =>
      shaded == null ? (
        <p className="hint">{sensorId}: setor não detectado.</p>
      ) : (
        <div className="sector-row">
          centrado em {number(center, onCenter, `Centro do setor de ${sensorId}`)}° com largura de {number(width, onWidth, `Largura do setor de ${sensorId}`)}°, marcar{' '}
          <span className="sensor-tag" style={{ background: color }}>
            {sensorId}
          </span>
        </div>
      );
    return (
      <div className="shading-details pair">
        <fieldset>
          <legend>Detalhes do par</legend>
          <label className="check">
            <input type="checkbox" checked={settings.enabled} onChange={(event) => update({ enabled: event.target.checked })} />
            Gerar regras para este par
          </label>
          {directionSelect(settings.direction, (direction) => update({ direction }))}
          <p>Com a direção do vento no setor:</p>
          {sectorRow(pair.a, pair.shadedA, settings.centerA, settings.widthA, (centerA) => update({ centerA }), (widthA) => update({ widthA }), SHADE_A)}
          {sectorRow(pair.b, pair.shadedB, settings.centerB, settings.widthB, (centerB) => update({ centerB }), (widthB) => update({ widthB }), SHADE_B)}
          <p className="hint">{formatCount(pair.counts.reduce((total, count) => total + count, 0))} passos com os três sensores.</p>
        </fieldset>
        <div className="shading-plots">
          <MedianPlot pair={pair} sectors={data.sectors} settings={settings} polar={polar} />
          <RatioPlot api={api} dataset={dataset.id} pair={pair} settings={settings} polar={polar} />
        </div>
      </div>
    );
  };

  return (
    <section className="card shading-window">
      <div className="cardhead">
        <h3>Sombra da torre</h3>
        <label className="dataset-select">
          Dataset
          <select value={dataset.id} onChange={(event) => onChoose(Number(event.target.value))}>
            {entries.map((candidate) => (
              <option key={candidate.result.dataset.id} value={candidate.result.dataset.id}>
                {candidate.result.dataset.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="shading-basis" role="radiogroup" aria-label="Sinalizar com base em">
        <span>Sinalizar a sombra da torre com base em</span>
        <label className="check">
          <input type="radio" checked={basis === 'boom'} onChange={() => setBasis('boom')} />
          orientação da lança
        </label>
        <label className="check">
          <input type="radio" checked={basis === 'pattern'} onChange={() => setBasis('pattern')} />
          padrão de sombra detectado
        </label>
        <label className="check">
          <input type="checkbox" checked={polar} onChange={(event) => setPolar(event.target.checked)} />
          Gráfico polar
        </label>
      </div>
      {error && <div className="note err">{error}</div>}
      <div className="shading-body">
        <label className="shading-list">
          {basis === 'boom' ? 'Sensores de velocidade' : 'Pares de sensores de velocidade'}
          {basis === 'boom' ? (
            <select size={LIST_ROWS} value={sensor} onChange={(event) => setSensor(event.target.value)}>
              {speeds.map((column) => (
                <option key={column.id} value={column.id}>
                  {column.id}
                  {booms[column.id] != null ? ` · lança ${formatNumber(booms[column.id])}°` : ''}
                </option>
              ))}
            </select>
          ) : (
            <select size={LIST_ROWS} value={pairIndex} onChange={(event) => setPairIndex(Number(event.target.value))}>
              {data?.pairs.map((pair, index) => (
                <option key={pairKey(pair)} value={index}>
                  {pair.a} | {pair.b}
                </option>
              ))}
            </select>
          )}
        </label>
        {basis === 'boom' ? boomDetails() : pairDetails()}
      </div>
      <fieldset className="shading-rules">
        <legend>Regras geradas</legend>
        <label>
          Flag a aplicar
          <select value={flag} onChange={(event) => setFlag(Number(event.target.value))}>
            {dataset.flags.map((known, index) => (
              <option key={index} value={index}>
                {known.name}
              </option>
            ))}
          </select>
        </label>
        <ul className="rule-lines">
          {generated.length === 0 && <li className="hint">Nenhuma regra: escolha um sensor com lança ou um par com setor detectado.</li>}
          {generated.map((rule, index) =>
            rule.criterion.kind === 'direction_in_sector' ? (
              <li key={index}>
                Aplicar a flag '{flagName}' a '{rule.columns[0]}' onde o valor de '{rule.criterion.direction}' estiver de {sectorText(rule.criterion.center, rule.criterion.width)}
              </li>
            ) : null,
          )}
        </ul>
      </fieldset>
      <div className="cardfoot">
        {outcome && <span className="note">{outcome}</span>}
        <button className="btn primary" disabled={disabled || running || !generated.length} onClick={run}>
          {running ? 'Executando…' : 'Executar regras'}
        </button>
      </div>
    </section>
  );
}

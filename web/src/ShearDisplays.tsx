/**
 * The Wind Shear window's displays beyond the overall speed profile (manual
 * dlg-analyze-wind-shear.htm): the mean speed, speed SD and direction shear
 * subdivided by up to two of hour, month, month of the record, direction
 * sector and bins of a column; mean speed and shear by hour; the hodograph;
 * and the frequency of the step-by-step α or veer. The figures are the core's
 * (crates/wind-core/src/analysis/shear_views.rs).
 */
import { useMemo, useState, type ReactNode } from 'react';
import type { WindApi } from './api/client';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { FilterDto } from './api/generated/FilterDto';
import type { ShearAggregation } from './api/generated/ShearAggregation';
import type { ShearDivisionDto } from './api/generated/ShearDivisionDto';
import type { ShearFrequencyDto } from './api/generated/ShearFrequencyDto';
import type { ShearQuantityDto } from './api/generated/ShearQuantityDto';
import type { ShearViewDto } from './api/generated/ShearViewDto';
import type { VectorMagnitudeDto } from './api/generated/VectorMagnitudeDto';
import { AnalysisLayout, ColumnSelect, HOUR_LABELS, isMeanDirection, isMeanSpeed, Plot, SAFE_TOOLTIP, sectorLabels, useQuery } from './Analyses';
import { MONTH_ABBREVIATIONS } from './labels';
import { formatCount, formatNumber, formatPercent, NO_VALUE } from './format';

/** The window's Display choices, as the original lists them. */
export type ShearDisplay = 'speed' | 'speed_hour' | 'speed_sd' | 'direction' | 'hodograph' | 'freq_speed' | 'freq_sd' | 'freq_direction';
export const SHEAR_DISPLAY_LABEL: Record<ShearDisplay, string> = {
  speed_hour: 'Velocidade média e cisalhamento por hora',
  speed: 'Cisalhamento da velocidade média',
  speed_sd: 'Cisalhamento do DP da velocidade',
  direction: 'Cisalhamento da direção média (veer)',
  hodograph: 'Velocidade e direção médias (hodógrafo)',
  freq_speed: 'Frequência do cisalhamento da velocidade',
  freq_sd: 'Frequência do cisalhamento do DP',
  freq_direction: 'Frequência do cisalhamento da direção',
};
/** The displays that subdivide. */
export const SUBDIVIDED_DISPLAYS: ShearDisplay[] = ['speed', 'speed_sd', 'direction'];

/** "Subdividir por": one division or a pair, as the original's lists. */
export type Subdivision = 'none' | 'hour' | 'month' | 'month_of_record' | 'sector' | 'bins' | 'month_hour' | 'sector_hour' | 'sector_month' | 'bins_sector';
export const SUBDIVISION_LABEL: Record<Subdivision, string> = {
  none: 'Altura (sem subdivisão)',
  hour: 'Hora do dia',
  month: 'Mês',
  month_of_record: 'Mês do dataset',
  sector: 'Setor de direção',
  bins: 'Faixas de uma coluna',
  month_hour: 'Mês e hora do dia',
  sector_hour: 'Setor e hora do dia',
  sector_month: 'Setor e mês',
  bins_sector: 'Faixas e setor',
};
const MAGNITUDE_LABEL: Record<VectorMagnitudeDto, string> = {
  primary: 'Velocidade primária',
  associated: 'Velocidade associada (mais próxima em altura)',
  unit: 'Nenhuma (vetores unitários)',
};
const SECTOR_CHOICES = [4, 8, 12, 16, 36];
const DEFAULT_SECTORS = 12;
/** Default bin widths: α, the veer rate (°/100 m) and a binned column. */
const DEFAULT_ALPHA_BIN = 0.02;
const DEFAULT_VEER_BIN = 1;
const DEFAULT_COLUMN_BIN = 1;
const PARAMETER_DIGITS = 3;
const VEER_DIGITS = 2;
const ROUGHNESS_DIGITS = 4;
const CHART_HEIGHT = 340;
const HODOGRAPH_ARROW_SIZE = 14;
/** The headroom around the hodograph's longest arrow. */
const HODOGRAPH_MARGIN = 1.15;
const HODOGRAPH_GRID = { top: 48, bottom: 48 };
const PERCENT = 100;

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  filter?: FilterDto;
  display: ShearDisplay;
  subdivision: Subdivision;
  aggregation: ShearAggregation;
  /** The selected mean speed columns. */
  speedIds: string[];
  /** The window's own controls (display, subdivision, method, speed columns). */
  controls: ReactNode;
}

const quantityOf = (display: ShearDisplay): ShearQuantityDto =>
  display === 'speed_sd' || display === 'freq_sd' ? 'speed_sd' : display === 'direction' || display === 'hodograph' || display === 'freq_direction' ? 'direction' : 'speed';
const parameterName = (quantity: ShearQuantityDto) => (quantity === 'direction' ? 'Veer (°/100 m)' : 'α');
const parameterDigits = (quantity: ShearQuantityDto) => (quantity === 'direction' ? VEER_DIGITS : PARAMETER_DIGITS);

/** The labels of one division's groups. */
function axisLabels(kind: ShearDivisionDto['kind'], axis: ShearViewDto['axes'][number]): string[] {
  switch (kind) {
    case 'hour':
      return HOUR_LABELS;
    case 'month':
      return MONTH_ABBREVIATIONS;
    case 'month_of_record':
      return axis.months.map(([year, month]) => `${MONTH_ABBREVIATIONS[month - 1]}/${year}`);
    case 'sector':
      return sectorLabels(axis.groups);
    case 'bins':
      return Array.from({ length: axis.groups }, (_, bin) => `${formatNumber(axis.start + bin * axis.width)}–${formatNumber(axis.start + (bin + 1) * axis.width)}`);
  }
}

/** Mean of the dataset's column ids, by role. */
function useColumns(dataset: DatasetDto, speedIds: string[]) {
  return useMemo(() => {
    const sdOf = (speed: string) => dataset.columns.find((column) => column.parent === speed && column.subtype === 'std_dev' && column.heightM != null)?.id;
    const vanes = dataset.columns.filter((column) => isMeanDirection(column) && column.heightM != null).map((column) => column.id);
    return { sds: speedIds.map(sdOf).filter((id): id is string => id != null), vanes };
  }, [dataset, speedIds]);
}

/** One of the displays of the Wind Shear window other than the overall speed profile. */
export function ShearDisplays({ api, dataset, filter, display, subdivision, aggregation, speedIds, controls }: Props) {
  const { sds, vanes } = useColumns(dataset, speedIds);
  const [vaneIds, setVaneIds] = useState<string[]>(vanes);
  const [magnitude, setMagnitude] = useState<VectorMagnitudeDto>('primary');
  const [directionId, setDirectionId] = useState<string | null>(() => dataset.columns.find(isMeanDirection)?.id ?? null);
  const [sectors, setSectors] = useState(DEFAULT_SECTORS);
  const [binColumn, setBinColumn] = useState<string | null>(() => dataset.columns.find(isMeanSpeed)?.id ?? null);
  const [binWidth, setBinWidth] = useState(DEFAULT_COLUMN_BIN);
  const quantity = quantityOf(display);
  const [frequencyBin, setFrequencyBin] = useState<Record<ShearQuantityDto, number>>({ speed: DEFAULT_ALPHA_BIN, speed_sd: DEFAULT_ALPHA_BIN, direction: DEFAULT_VEER_BIN });
  const columns = quantity === 'speed' ? speedIds : quantity === 'speed_sd' ? sds : vaneIds;
  const frequency = display.startsWith('freq_');

  const divisions: ShearDivisionDto[] | null = useMemo(() => {
    if (display === 'speed_hour') return [{ kind: 'hour' }];
    if (!SUBDIVIDED_DISPLAYS.includes(display)) return [];
    const sector: ShearDivisionDto | null = directionId ? { kind: 'sector', direction: directionId, sectors } : null;
    const bins: ShearDivisionDto | null = binColumn ? { kind: 'bins', column: binColumn, width: binWidth } : null;
    const parts: Record<Subdivision, (ShearDivisionDto | null)[]> = {
      none: [],
      hour: [{ kind: 'hour' }],
      month: [{ kind: 'month' }],
      month_of_record: [{ kind: 'month_of_record' }],
      sector: [sector],
      bins: [bins],
      month_hour: [{ kind: 'month' }, { kind: 'hour' }],
      sector_hour: [sector, { kind: 'hour' }],
      sector_month: [sector, { kind: 'month' }],
      bins_sector: [bins, sector],
    };
    const chosen = parts[subdivision];
    return chosen.every((part) => part != null) ? (chosen as ShearDivisionDto[]) : null;
  }, [display, subdivision, directionId, sectors, binColumn, binWidth]);

  const viewRequest =
    frequency || divisions == null || columns.length < 2
      ? null
      : {
          dataset: dataset.id,
          quantity,
          columns,
          magnitude: display === 'hodograph' ? ('associated' as const) : magnitude,
          divisions,
          aggregation,
          filter,
        };
  const frequencyRequest = frequency && columns.length >= 2 ? { dataset: dataset.id, quantity, columns, width: frequencyBin[quantity], filter } : null;
  const { data: view, error: viewError } = useQuery(
    () => (viewRequest ? api.shearView(viewRequest).then((answer) => ({ answer, divisions: viewRequest.divisions, display })) : undefined),
    [api, dataset, JSON.stringify(viewRequest), display],
  );
  const { data: binned, error: frequencyError } = useQuery(
    () => (frequencyRequest ? api.shearFrequency(frequencyRequest) : undefined),
    [api, dataset, JSON.stringify(frequencyRequest)],
  );

  const missing =
    columns.length < 2
      ? quantity === 'speed_sd'
        ? 'O cisalhamento do DP precisa do desvio padrão de pelo menos duas das velocidades escolhidas, em alturas diferentes.'
        : quantity === 'direction'
          ? 'O cisalhamento da direção precisa de pelo menos dois sensores de direção com altura informada.'
          : 'Escolha pelo menos duas colunas de velocidade.'
      : divisions == null
        ? 'Escolha a coluna da subdivisão.'
        : undefined;

  const ownControls = (
    <>
      {display !== 'hodograph' && SUBDIVIDED_DISPLAYS.includes(display) && subdivision.includes('sector') && (
        <>
          <ColumnSelect label="Sensor de direção (setores)" dataset={dataset} value={directionId} onChange={setDirectionId} only={isMeanDirection} />
          <label>
            Setores
            <select value={sectors} onChange={(event) => setSectors(Number(event.target.value))}>
              {SECTOR_CHOICES.map((choice) => (
                <option key={choice}>{choice}</option>
              ))}
            </select>
          </label>
        </>
      )}
      {SUBDIVIDED_DISPLAYS.includes(display) && subdivision.startsWith('bins') && (
        <>
          <ColumnSelect label="Coluna das faixas" dataset={dataset} value={binColumn} onChange={setBinColumn} />
          <label>
            Largura da faixa
            <input type="number" min={0} step="any" value={binWidth} onChange={(event) => setBinWidth(Number(event.target.value))} />
          </label>
        </>
      )}
      {quantity === 'direction' && display === 'direction' && (
        <label title="O que pesa a média vetorial de cada sensor de direção. A primária usa a mesma velocidade em todos, o que deixa os resultados mais fáceis de comparar.">
          Magnitude dos vetores
          <select value={magnitude} onChange={(event) => setMagnitude(event.target.value as VectorMagnitudeDto)}>
            {Object.entries(MAGNITUDE_LABEL).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
      )}
      {frequency && (
        <label>
          Largura da faixa ({quantity === 'direction' ? '°/100 m' : 'α'})
          <input
            type="number"
            min={0}
            step="any"
            value={frequencyBin[quantity]}
            onChange={(event) => setFrequencyBin({ ...frequencyBin, [quantity]: Number(event.target.value) })}
          />
        </label>
      )}
      {quantity === 'direction' && (
        <fieldset className="columns">
          <legend>Sensores de direção</legend>
          {vanes.map((id) => (
            <label key={id} className="check">
              <input
                type="checkbox"
                checked={vaneIds.includes(id)}
                onChange={() => setVaneIds(vaneIds.includes(id) ? vaneIds.filter((vane) => vane !== id) : vanes.filter((vane) => vane === id || vaneIds.includes(vane)))}
              />
              {id} · {formatNumber(dataset.columns.find((column) => column.id === id)?.heightM)} m
            </label>
          ))}
        </fieldset>
      )}
    </>
  );

  const layoutControls = (
    <>
      {controls}
      {ownControls}
    </>
  );
  if (frequency) {
    return (
      <AnalysisLayout
        recipe={frequencyRequest && { analysis: 'shear_frequency', request: frequencyRequest }}
        controls={layoutControls}
        error={missing ?? frequencyError}
        facts={binned && <FrequencyFacts binned={binned} quantity={quantity} />}
        graph={binned && <FrequencyCharts binned={binned} quantity={quantity} />}
        table={binned && <FrequencyTable binned={binned} quantity={quantity} />}
        note={<p className="hint">O parâmetro de cada passo com dado em todas as alturas, ajustado só àquele passo, com a sua altura de deslocamento.</p>}
      />
    );
  }
  const answer = view?.display === display ? view : undefined;
  return (
    <AnalysisLayout
      recipe={viewRequest && { analysis: 'shear_view', request: viewRequest }}
      controls={layoutControls}
      error={missing ?? viewError}
      facts={answer && <ViewFacts view={answer.answer} />}
      graph={answer && <ViewChart view={answer.answer} divisions={answer.divisions} display={display} />}
      table={answer && <ViewTable view={answer.answer} divisions={answer.divisions} columns={columns} />}
      note={
        <p className="hint">
          Sobre os passos em que todas as colunas envolvidas têm dado. Pelos métodos de série temporal, o parâmetro é a média ou a mediana dos
          ajustes passo a passo; as médias mostradas são as simples. Dentro de uma subdivisão, a média das médias mensais pesa cada mês pelos
          seus dias, sem o fator de completude.
        </p>
      }
    />
  );
}

function ViewFacts({ view }: { view: ShearViewDto }) {
  if (view.axes.length > 0) return null;
  const cell = view.cells[0];
  return (
    <div className="facts">
      <span>
        <b>{parameterName(view.quantity)}</b> {formatNumber(cell.parameter, parameterDigits(view.quantity))}
      </span>
      {view.quantity === 'speed' && (
        <span>
          <b>Rugosidade z₀</b> {cell.roughnessM == null ? NO_VALUE : `${formatNumber(cell.roughnessM, ROUGHNESS_DIGITS)} m`}
        </span>
      )}
      <span>
        <b>Passos simultâneos</b> {formatCount(cell.samples)}
      </span>
    </div>
  );
}

function ViewChart({ view, divisions, display }: { view: ShearViewDto; divisions: ShearDivisionDto[]; display: ShearDisplay }) {
  const option = useMemo(() => {
    const digits = parameterDigits(view.quantity);
    const tooltip = { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value, digits) };
    const grid = { left: 64, right: 56, top: 40, bottom: 48 };
    if (display === 'hodograph') {
      const cell = view.cells[0];
      const tips = cell.means.map((direction, vane) => {
        const speed = cell.magnitudes[vane];
        if (direction == null || speed == null) return null;
        const radians = (direction * Math.PI) / 180;
        return [speed * Math.sin(radians), speed * Math.cos(radians), direction];
      });
      const reach = Math.ceil(Math.max(1, ...tips.map((tip) => (tip ? Math.hypot(tip[0], tip[1]) : 0))) * HODOGRAPH_MARGIN);
      // A square plot, so each arrow's angle on screen is its direction.
      const side = CHART_HEIGHT - HODOGRAPH_GRID.top - HODOGRAPH_GRID.bottom;
      return {
        tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
        legend: { top: 4 },
        grid: { ...HODOGRAPH_GRID, left: 'center', width: side, height: side },
        xAxis: { type: 'value', min: -reach, max: reach, name: 'Leste (m/s)', nameLocation: 'middle', nameGap: 28 },
        yAxis: { type: 'value', min: -reach, max: reach, name: 'Norte (m/s)', nameLocation: 'middle', nameGap: 36 },
        series: tips.flatMap((tip, vane) => {
          const name = `${formatNumber(view.heightsM[vane])} m`;
          if (!tip) return [];
          return [
            { type: 'line', name, data: [[0, 0], [tip[0], tip[1]]], showSymbol: false },
            { type: 'scatter', name, data: [{ value: [tip[0], tip[1]], symbolRotate: -tip[2] }], symbol: 'arrow', symbolSize: HODOGRAPH_ARROW_SIZE },
          ];
        }),
      };
    }
    if (divisions.length === 0) {
      const cell = view.cells[0];
      const unit = view.quantity === 'direction' ? '°' : 'm/s';
      return {
        tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
        grid,
        xAxis: { type: 'value', scale: true, name: unit, nameLocation: 'middle', nameGap: 28 },
        yAxis: { type: 'value', name: 'Altura (m)', min: 0 },
        series: [{ type: 'scatter', name: 'Média', data: cell.means.map((mean, column) => [mean, view.heightsM[column]]), symbolSize: 10 }],
      };
    }
    const labels = divisions.map((division, axis) => axisLabels(division.kind, view.axes[axis]));
    const parameter = view.cells.map((cell) => cell.parameter);
    if (display === 'speed_hour') {
      const highest = view.heightsM.indexOf(Math.max(...view.heightsM));
      return {
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' },
        legend: { top: 4 },
        grid,
        xAxis: { type: 'category', data: labels[0] },
        yAxis: [
          { type: 'value', name: 'm/s', scale: true },
          { type: 'value', name: 'α', scale: true },
        ],
        series: [
          { type: 'line', name: `Velocidade a ${formatNumber(view.heightsM[highest])} m`, data: view.cells.map((cell) => cell.means[highest]) },
          { type: 'line', name: 'α', yAxisIndex: 1, data: parameter },
        ],
      };
    }
    if (divisions.length === 1) {
      const bars = divisions[0].kind === 'sector' || divisions[0].kind === 'bins';
      return {
        tooltip,
        grid,
        xAxis: { type: 'category', data: labels[0] },
        yAxis: { type: 'value', name: parameterName(view.quantity), scale: true },
        series: [{ type: bars ? 'bar' : 'line', name: parameterName(view.quantity), data: parameter }],
      };
    }
    const inner = view.axes[1].groups;
    return {
      tooltip,
      legend: { top: 4, type: 'scroll' },
      grid: { ...grid, top: 56 },
      xAxis: { type: 'category', data: labels[1] },
      yAxis: { type: 'value', name: parameterName(view.quantity), scale: true },
      series: labels[0].map((label, outer) => ({ type: 'line', name: label, data: parameter.slice(outer * inner, (outer + 1) * inner), connectNulls: false })),
    };
  }, [view, divisions, display]);
  return <Plot option={option} height={CHART_HEIGHT} />;
}

function ViewTable({ view, divisions, columns }: { view: ShearViewDto; divisions: ShearDivisionDto[]; columns: string[] }) {
  const digits = parameterDigits(view.quantity);
  const unit = view.quantity === 'direction' ? '°' : 'm/s';
  if (divisions.length === 0) {
    const cell = view.cells[0];
    return (
      <div className="tablewrap">
        <table className="grid">
          <thead>
            <tr>
              <th>Coluna</th>
              <th className="r">Altura (m)</th>
              <th className="r">Média ({unit})</th>
              {view.quantity === 'direction' && <th className="r">Velocidade (m/s)</th>}
            </tr>
          </thead>
          <tbody>
            {view.heightsM.map((height, column) => (
              <tr key={column}>
                <td>{columns[column]}</td>
                <td className="r">{formatNumber(height)}</td>
                <td className="r">{formatNumber(cell.means[column], digits)}</td>
                {view.quantity === 'direction' && (
                  <td className="r" title={view.magnitudeColumns[column] ?? undefined}>
                    {formatNumber(cell.magnitudes[column], digits)}
                  </td>
                )}
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={view.quantity === 'direction' ? 4 : 3}>
                {parameterName(view.quantity)} {formatNumber(cell.parameter, digits)}
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
    );
  }
  const labels = divisions.map((division, axis) => axisLabels(division.kind, view.axes[axis]));
  if (divisions.length === 2) {
    const inner = view.axes[1].groups;
    return (
      <div className="tablewrap">
        <table className="grid">
          <thead>
            <tr>
              <th>{parameterName(view.quantity)}</th>
              {labels[1].map((label) => (
                <th key={label} className="r">
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {labels[0].map((label, outer) => (
              <tr key={label}>
                <td>{label}</td>
                {view.cells.slice(outer * inner, (outer + 1) * inner).map((cell, index) => (
                  <td key={index} className="r">
                    {formatNumber(cell.parameter, digits)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }
  return (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>Grupo</th>
            <th className="r">Passos</th>
            <th className="r">{parameterName(view.quantity)}</th>
            {columns.map((column) => (
              <th key={column} className="r">
                {column} ({unit})
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {view.cells.map((cell, group) => (
            <tr key={group}>
              <td>{labels[0][group]}</td>
              <td className="r">{formatCount(cell.samples)}</td>
              <td className="r">{formatNumber(cell.parameter, digits)}</td>
              {cell.means.map((mean, column) => (
                <td key={column} className="r">
                  {formatNumber(mean, digits)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const binLabel = (binned: ShearFrequencyDto, bin: number, digits: number) =>
  `${formatNumber(binned.start + bin * binned.width, digits)} a ${formatNumber(binned.start + (bin + 1) * binned.width, digits)}`;

function FrequencyFacts({ binned, quantity }: { binned: ShearFrequencyDto; quantity: ShearQuantityDto }) {
  return (
    <div className="facts">
      <span>
        <b>Passos</b> {formatCount(binned.samples)}
      </span>
      <span>
        <b>Média</b> {formatNumber(binned.mean, parameterDigits(quantity))}
      </span>
    </div>
  );
}

/** The histogram, then the shares below and above each bin edge on a log scale (the tails). */
function FrequencyCharts({ binned, quantity }: { binned: ShearFrequencyDto; quantity: ShearQuantityDto }) {
  const options = useMemo(() => {
    const digits = parameterDigits(quantity);
    const edges = binned.frequency.map((_, bin) => binned.start + bin * binned.width);
    const centres = edges.map((edge) => edge + binned.width / 2);
    let below = 0;
    const cumulativeBelow = binned.frequency.map((share) => (below += share));
    const positive = (share: number) => (share > 0 ? share * PERCENT : null);
    const grid = { left: 64, right: 24, top: 40, bottom: 48 };
    const xAxis = { type: 'value', name: parameterName(quantity), nameLocation: 'middle', nameGap: 28, scale: true };
    return {
      histogram: {
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => `${formatNumber(value, digits)} %` },
        grid,
        xAxis,
        yAxis: { type: 'value', name: 'Frequência (%)' },
        series: [{ type: 'bar', name: 'Frequência', data: centres.map((centre, bin) => [centre, binned.frequency[bin] * PERCENT]), barWidth: '90%' }],
      },
      tails: {
        tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' },
        legend: { top: 4 },
        grid,
        xAxis,
        yAxis: { type: 'log', name: '% do tempo' },
        series: [
          { type: 'line', name: 'Abaixo de', data: edges.map((edge, bin) => [edge + binned.width, positive(cumulativeBelow[bin])]), showSymbol: false },
          { type: 'line', name: 'Acima de', data: edges.map((edge, bin) => [edge, positive(1 - (cumulativeBelow[bin] - binned.frequency[bin]))]), showSymbol: false },
        ],
      },
    };
  }, [binned, quantity]);
  return (
    <>
      <Plot option={options.histogram} height={CHART_HEIGHT} />
      <h3>Caudas (escala logarítmica)</h3>
      <Plot option={options.tails} height={CHART_HEIGHT} />
    </>
  );
}

function FrequencyTable({ binned, quantity }: { binned: ShearFrequencyDto; quantity: ShearQuantityDto }) {
  const digits = parameterDigits(quantity);
  return (
    <div className="tablewrap">
      <table className="grid">
        <thead>
          <tr>
            <th>{parameterName(quantity)}</th>
            <th className="r">Frequência</th>
          </tr>
        </thead>
        <tbody>
          {binned.frequency.map((share, bin) => (
            <tr key={bin}>
              <td>{binLabel(binned, bin, digits)}</td>
              <td className="r">{formatPercent(share)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

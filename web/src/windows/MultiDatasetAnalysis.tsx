/**
 * An analysis window on "<todos visíveis>" (ESPEC-INTERFACE §3.1): the same
 * analysis of every dataset checked as visible in the explorer, each in its
 * colour, on one graph and one table. Each dataset is read through its
 * primary columns (Colunas primárias) and its own "Filtrar por". Histogram,
 * wind rose, diurnal profile and shear offer it.
 */
import { useMemo } from 'react';
import type { WindApi } from '../api/client';
import type { DiurnalDto } from '../api/generated/DiurnalDto';
import type { HistogramDto } from '../api/generated/HistogramDto';
import type { ShearDto } from '../api/generated/ShearDto';
import type { WindRoseDto } from '../api/generated/WindRoseDto';
import { AnalysisLayout, HOUR_LABELS, Plot, SAFE_TOOLTIP, sectorLabels, useQuery, type AnalysisKind } from '../Analyses';
import { filterRequest } from '../FilterBy';
import { errorMessage, formatNumber, formatPercent, NO_VALUE } from '../format';
import { legendLabels, lineOption, roseOption, ROSE_SECTORS, shearOption } from '../summary/SummaryGraphs';
import { isDerived, isMeanSpeed, primaryColumns, type Entry } from '../workbook/model';

/** Windows that can show every visible dataset at once. */
export const MULTI_DATASET_KINDS: ReadonlySet<AnalysisKind> = new Set<AnalysisKind>(['histogram', 'rose', 'diurnal', 'shear']);
/** Histogram bins of 1 m/s, from 0, so every dataset shares them. */
const HISTOGRAM_BIN_M_S = 1;
/** Shear by sector is not drawn here; the request still needs a count. */
const SHEAR_SECTORS = 12;
const PERCENT = 100;
const HALF = 0.5;
/** Digits of the shear exponent and of Weibull's k. */
const PARAMETER_DIGITS = 3;
const GRAPH_PX = 420;
const NEEDS: Record<string, string> = {
  histogram: 'uma velocidade primária',
  rose: 'uma direção primária',
  diurnal: 'uma velocidade primária',
  shear: 'duas alturas de velocidade',
};

/** One dataset's answer, or why it has none. */
interface Answer {
  label: string;
  color: string;
  histogram: HistogramDto | null;
  rose: WindRoseDto | null;
  diurnal: DiurnalDto | null;
  shear: ShearDto | null;
  /** Why the dataset is left out. */
  missing?: string;
}

/** The request `kind` sends for `entry`, through its primary columns and its filter; `null` when the dataset cannot answer. */
async function ask(api: WindApi, kind: AnalysisKind, entry: Entry): Promise<Partial<Answer>> {
  const { dataset } = entry.result;
  const filter = filterRequest(entry.filter, dataset, kind);
  const { speed, direction } = primaryColumns(dataset);
  const id = dataset.id;
  if (kind === 'histogram' && speed) return { histogram: await api.histogram({ dataset: id, column: speed.id, binWidth: HISTOGRAM_BIN_M_S, weibull: 'max_likelihood', filter }) };
  if (kind === 'diurnal' && speed) return { diurnal: await api.diurnal({ dataset: id, column: speed.id, filter }) };
  if (kind === 'rose' && direction) return { rose: await api.windRose({ dataset: id, direction: direction.id, speed: speed?.id, sectors: ROSE_SECTORS, filter }) };
  const speeds = dataset.columns.filter((column) => isMeanSpeed(column) && !isDerived(column) && column.heightM != null);
  if (kind === 'shear' && new Set(speeds.map((column) => column.heightM)).size >= 2) {
    return { shear: await api.shear({ dataset: id, columns: speeds.map((column) => column.id), sectors: SHEAR_SECTORS, filter }) };
  }
  return { missing: `sem ${NEEDS[kind]}` };
}

interface Props {
  api: WindApi;
  kind: AnalysisKind;
  entries: Entry[];
}

export function MultiDatasetAnalysis({ api, kind, entries }: Props) {
  const visible = entries.filter((entry) => entry.visible);
  const { data: answers, error } = useQuery(() => {
    const labels = legendLabels(visible.map((entry) => entry.result.dataset.name));
    return Promise.all(
      visible.map(async (entry, position): Promise<Answer> => {
        const blank = { label: labels[position], color: entry.color, histogram: null, rose: null, diurnal: null, shear: null };
        try {
          return { ...blank, ...(await ask(api, kind, entry)) };
        } catch (thrown) {
          return { ...blank, missing: errorMessage(thrown) };
        }
      }),
    );
  }, [api, entries, kind]);
  const drawn = answers?.filter((answer) => !answer.missing) ?? [];
  const left = answers?.filter((answer) => answer.missing) ?? [];

  const views = useMemo(() => {
    if (!drawn.length) return null;
    switch (kind) {
      case 'histogram': {
        const bins = Math.max(...drawn.map((answer) => answer.histogram!.frequency.length));
        const center = (bin: number) => (bin + HALF) * HISTOGRAM_BIN_M_S;
        const name = (answer: Answer) => {
          const fit = answer.histogram!.weibull;
          return fit ? `${answer.label} (k ${formatNumber(fit.k, PARAMETER_DIGITS)} · A ${formatNumber(fit.a)})` : answer.label;
        };
        return {
          graph: {
            animation: false,
            tooltip: { ...SAFE_TOOLTIP, trigger: 'axis' as const, valueFormatter: (value: number) => `${formatNumber(value)}%` },
            legend: { top: 0, type: 'scroll' as const },
            grid: { left: 52, right: 16, top: 36, bottom: 40 },
            xAxis: { type: 'value', name: 'Velocidade (m/s)', nameLocation: 'middle', nameGap: 26, min: 0 },
            yAxis: { type: 'value', name: '%' },
            series: drawn.map((answer) => ({
              type: 'line',
              name: name(answer),
              color: answer.color,
              symbolSize: 4,
              data: answer.histogram!.frequency.map((share, bin) => [center(bin), share * PERCENT]),
            })),
          },
          rows: Array.from({ length: bins }, (_, bin) => ({
            label: `${bin * HISTOGRAM_BIN_M_S}–${(bin + 1) * HISTOGRAM_BIN_M_S}`,
            cells: drawn.map((answer) => formatPercent(answer.histogram!.frequency[bin])),
          })),
          rowHeader: 'm/s',
          facts: drawn.map((answer) => `${answer.label}: média ${formatNumber(answer.histogram!.mean)} m/s`),
        };
      }
      case 'diurnal':
        return {
          graph: lineOption(HOUR_LABELS, 'm/s', drawn, (answer) => answer.diurnal!.all),
          rows: HOUR_LABELS.map((hour, index) => ({ label: hour, cells: drawn.map((answer) => formatNumber(answer.diurnal!.all[index])) })),
          rowHeader: 'Hora',
          facts: [],
        };
      case 'rose':
        return {
          graph: roseOption(drawn),
          rows: sectorLabels(ROSE_SECTORS).map((sector, index) => ({ label: sector, cells: drawn.map((answer) => formatPercent(answer.rose!.frequency[index])) })),
          rowHeader: 'Setor',
          facts: drawn.map((answer) => `${answer.label}: calmaria ${formatPercent(answer.rose!.calm)}`),
        };
      default:
        return {
          graph: shearOption(drawn),
          rows: [
            { label: 'Expoente α', cells: drawn.map((answer) => formatNumber(answer.shear!.alpha, PARAMETER_DIGITS)) },
            { label: 'Rugosidade z₀ (m)', cells: drawn.map((answer) => formatNumber(answer.shear!.roughnessLengthM, PARAMETER_DIGITS)) },
            { label: 'Alturas (m)', cells: drawn.map((answer) => answer.shear!.heightsM.join(', ')) },
            { label: 'Passos simultâneos', cells: drawn.map((answer) => String(answer.shear!.samples)) },
          ],
          rowHeader: '',
          facts: [],
        };
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [answers, kind]);

  const table = views && (
    <div className="tablewrap">
      <table className="grid multi-dataset">
        <thead>
          <tr>
            <th>{views.rowHeader}</th>
            {drawn.map((answer) => (
              <th key={answer.label} className="r">
                <span className="swatch" style={{ background: answer.color }} /> {answer.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {views.rows.map((row) => (
            <tr key={row.label}>
              <td>{row.label}</td>
              {row.cells.map((cell, index) => (
                <td key={index} className="r">
                  {cell || NO_VALUE}
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
      controls={<p className="hint">Cada dataset visível no explorador entra com a sua coluna primária e o seu "Filtrar por" (escolha um dataset na lista para ajustá-los).</p>}
      error={error ?? (answers && !drawn.length ? 'Nenhum dataset visível tem o que esta análise pede.' : undefined)}
      facts={views && views.facts.length > 0 && <div className="facts">{views.facts.map((fact) => <span key={fact}>{fact}</span>)}</div>}
      graph={views ? <Plot option={views.graph} height={GRAPH_PX} /> : undefined}
      table={table || undefined}
      note={left.length > 0 && <p className="hint">Fora: {left.map((answer) => `${answer.label} (${answer.missing})`).join('; ')}.</p>}
    />
  );
}

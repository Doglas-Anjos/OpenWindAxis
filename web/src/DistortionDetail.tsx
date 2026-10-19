/**
 * Analisar › Distorção da torre, one pair (manual dlg-analyze-tower-distortion.htm):
 * the median by sector per calendar year and per calendar month, and the
 * scatter of every step's value against the direction, alone or per year
 * or month with each group's median line over it
 * (crates/wind-core/src/analysis/distortion.rs, `distortion_detail`).
 */
import { useMemo } from 'react';
import type { WindApi } from './api/client';
import type { DistortionMeasureDto } from './api/generated/DistortionMeasureDto';
import type { FilterDto } from './api/generated/FilterDto';
import { Plot, SAFE_TOOLTIP, useQuery } from './Analyses';
import { MONTH_ABBREVIATIONS } from './labels';
import { formatCount } from './format';

/** The pair's displays beyond the statistics by sector. */
export type DistortionDisplay = 'sectors' | 'year' | 'month' | 'scatter' | 'scatter_year' | 'scatter_month';
export const DISTORTION_DISPLAY_LABEL: Record<DistortionDisplay, string> = {
  sectors: 'Estatística por setor',
  year: 'Mediana por setor, por ano',
  month: 'Mediana por setor, por mês',
  scatter: 'Dispersão',
  scatter_year: 'Dispersão por ano',
  scatter_month: 'Dispersão por mês',
};
const FULL_TURN_DEG = 360;
const DIRECTION_TICK_DEG = 30;
const POINT_SIZE = 2;
const CHART_HEIGHT = 420;
/** The scatter's axis spans these quantiles of the values: a near-zero B makes ratios in the hundreds. */
const LOW_QUANTILE = 0.005;
const HIGH_QUANTILE = 0.995;

interface Props {
  api: WindApi;
  dataset: number;
  pair: { a: string; b: string; direction: string };
  measure: DistortionMeasureDto;
  filter?: FilterDto;
  display: Exclude<DistortionDisplay, 'sectors'>;
  polar: boolean;
  /** The measure's axis name. */
  axisName: string;
}

/** One pair's per-year, per-month or scatter display. */
export function DistortionDetailChart({ api, dataset, pair, measure, filter, display, polar, axisName }: Props) {
  const request = { dataset, a: pair.a, b: pair.b, direction: pair.direction, measure, filter };
  const { data, error } = useQuery(() => api.distortionDetail(request), [api, dataset, pair.a, pair.b, pair.direction, measure, filter]);
  const option = useMemo(() => {
    if (!data) return null;
    const width = FULL_TURN_DEG / data.sectors;
    const place = (direction: number, value: number | null) => (polar ? [value, direction] : [direction, value]);
    const coordinateSystem = polar ? 'polar' : 'cartesian2d';
    const line = (name: string, medians: (number | null)[]) => {
      const points = medians.map((median, sector) => place(sector * width, median));
      return { type: 'line', name, coordinateSystem, data: polar ? [...points, points[0]] : points, showSymbol: false, connectNulls: true };
    };
    // Group of each step for the per-year and per-month scatters, and its label.
    const groups: { label: string; medians: (number | null)[]; member: (index: number) => boolean }[] =
      display === 'year' || display === 'scatter_year'
        ? data.byYear.map(({ year, medians }) => ({ label: String(year), medians, member: (index: number) => data.pointYears[index] === year }))
        : display === 'month' || display === 'scatter_month'
          ? data.byMonth.map((medians, month) => ({ label: MONTH_ABBREVIATIONS[month], medians, member: (index: number) => data.pointMonths[index] === month }))
          : [];
    const sorted = [...data.pointValues].sort((first, second) => first - second);
    const [low, high] = sorted.length ? [sorted[Math.floor(LOW_QUANTILE * (sorted.length - 1))], sorted[Math.ceil(HIGH_QUANTILE * (sorted.length - 1))]] : [0, 0];
    const inside = (value: number) => value >= low && value <= high;
    const scatterOf = (name: string, keep: (index: number) => boolean) => ({
      type: 'scatter',
      name,
      coordinateSystem,
      symbolSize: POINT_SIZE,
      large: true,
      data: data.pointDirections.flatMap((direction, index) => (keep(index) && inside(data.pointValues[index]) ? [place(direction, data.pointValues[index])] : [])),
    });
    const scattered = display.startsWith('scatter');
    const bounds = scattered ? { min: low, max: high } : {};
    const series =
      display === 'scatter'
        ? [scatterOf('Passos', () => true)]
        : display === 'year' || display === 'month'
          ? groups.map((group) => line(group.label, group.medians))
          : groups.flatMap((group) => [scatterOf(group.label, group.member), { ...line(group.label, group.medians), lineStyle: { width: 2 } }]);
    const axes = polar
      ? {
          polar: { radius: '72%' },
          angleAxis: { type: 'value', min: 0, max: FULL_TURN_DEG, interval: DIRECTION_TICK_DEG, startAngle: 90, clockwise: true },
          radiusAxis: { type: 'value', scale: true, ...bounds },
        }
      : {
          grid: { left: 64, right: 24, top: 48, bottom: 44 },
          xAxis: { type: 'value', min: 0, max: FULL_TURN_DEG, interval: DIRECTION_TICK_DEG, name: 'Direção (°)', nameLocation: 'middle', nameGap: 26 },
          yAxis: { type: 'value', scale: true, name: axisName, ...bounds },
        };
    const outside = scattered ? data.pointValues.filter((value) => !inside(value)).length : 0;
    return { chart: { animation: false, tooltip: { ...SAFE_TOOLTIP, trigger: 'item' }, legend: { top: 4, type: 'scroll' }, ...axes, series }, outside };
  }, [data, display, polar, axisName]);
  if (error) return <div className="note err">{error}</div>;
  if (!option) return null;
  return (
    <>
      <Plot option={option.chart} height={CHART_HEIGHT} />
      {option.outside > 0 && (
        <p className="hint">
          {formatCount(option.outside)} de {formatCount(data?.pointValues.length ?? 0)} pontos ficam fora do eixo, que vai do quantil de 0,5 % ao de 99,5 % (uma
          velocidade B perto de zero dá razões enormes).
        </p>
      )}
    </>
  );
}

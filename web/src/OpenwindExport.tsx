/**
 * The Openwind part of the export card (wind-core `export::openwind`): the
 * met mast file (MM 2.1) of one height, as the original's Export Data ›
 * Openwind writes it. The speed, vane and thermometer take a column each, or
 * are chosen automatically (the primary speed, the vane nearest it, the
 * primary temperature); without a thermometer the standard atmosphere's.
 * As the original's page: the whole record or a part of it, the three means
 * of the exported speeds, and the speeds scaled to a target mean.
 */
import { useState } from 'react';
import type { WindApi } from './api/client';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { ExportOpenwindRequest } from './api/generated/ExportOpenwindRequest';
import { useQuery } from './Analyses';
import { errorMessage, formatCount, formatNumber, NO_VALUE } from './format';
import { lastStepMs, typedNumber } from './workbook/model';
import { textFile } from './workbook/useWorkbook';

/** Select value of "chosen automatically" (the field left out) and of "none" (`""`). */
const AUTOMATIC = 'auto';
const NONE = '';
const OPENWIND_MEDIA_TYPE = 'text/csv';
/** Digits of a mean speed and of the scale factor. */
const MEAN_DIGITS = 3;
const FACTOR_DIGITS = 4;
/** A `datetime-local` value (minutes) of epoch ms, on the dataset's clock (UTC). */
const localValue = (epochMs: number) => new Date(epochMs).toISOString().slice(0, 16);
const epochOf = (value: string) => (value ? Date.parse(`${value}:00Z`) : undefined);

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** The source file's name, written in the file beside the dataset's. */
  sourceName: string;
  onDownload: (name: string, contents: Blob) => void;
  onError: (message: string) => void;
}

/** Form and button that download the Openwind file of the dataset. */
export function OpenwindExport({ api, dataset, sourceName, onDownload, onError }: Props) {
  const [speed, setSpeed] = useState(AUTOMATIC);
  const [direction, setDirection] = useState(AUTOMATIC);
  const [temperature, setTemperature] = useState(AUTOMATIC);
  const [whole, setWhole] = useState(true);
  const [from, setFrom] = useState(() => localValue(dataset.startMs));
  const [to, setTo] = useState(() => localValue(lastStepMs(dataset)));
  const [scale, setScale] = useState(false);
  const [targetText, setTargetText] = useState('');
  const [busy, setBusy] = useState(false);
  const means = (kind: string) => dataset.columns.filter((column) => column.kind === kind && column.subtype === 'mean');

  const request: ExportOpenwindRequest = { dataset: dataset.id, sourceName };
  if (speed !== AUTOMATIC) request.speed = speed;
  if (direction !== AUTOMATIC) request.direction = direction;
  if (temperature !== AUTOMATIC) request.temperature = temperature;
  if (!whole) {
    request.fromMs = epochOf(from);
    request.toMs = epochOf(to);
  }
  const { data: found, error } = useQuery(() => api.openwindMeans(request), [api, dataset, JSON.stringify(request)]);
  const target = typedNumber(targetText);
  const targetValid = target !== undefined && Number.isFinite(target) && target > 0;
  const factor = scale && targetValid && found?.simple ? target / found.simple : undefined;

  const exportOpenwind = async () => {
    setBusy(true);
    try {
      const file = await api.exportOpenwind(scale && targetValid ? { ...request, targetMean: target } : request);
      onDownload(file.name, textFile(file.text, OPENWIND_MEDIA_TYPE));
    } catch (thrown) {
      onError(errorMessage(thrown));
    }
    setBusy(false);
  };

  const select = (label: string, value: string, onChange: (value: string) => void, kind: string, noneLabel?: string) => (
    <label>
      {label}
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        <option value={AUTOMATIC}>Automático</option>
        {noneLabel && <option value={NONE}>{noneLabel}</option>}
        {means(kind).map((column) => (
          <option key={column.id} value={column.id}>
            {column.id}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <div className="controls">
      {select('Sensor de velocidade', speed, setSpeed, 'wind_speed')}
      {select('Sensor de direção', direction, setDirection, 'wind_direction')}
      {select('Sensor de temperatura', temperature, setTemperature, 'temperature', 'Nenhum (atmosfera padrão)')}
      <label className="check">
        <input type="radio" checked={whole} onChange={() => setWhole(true)} />
        Todo o registro
      </label>
      <label className="check">
        <input type="radio" checked={!whole} onChange={() => setWhole(false)} />
        Um trecho
      </label>
      {!whole && (
        <div className="row">
          <label>
            De
            <input type="datetime-local" value={from} onChange={(event) => setFrom(event.target.value)} />
          </label>
          <label>
            A
            <input type="datetime-local" value={to} onChange={(event) => setTo(event.target.value)} />
          </label>
        </div>
      )}
      {error && <div className="note err">{error}</div>}
      {found && (
        <table className="grid openwind-means">
          <tbody>
            <tr title="A média simples das velocidades exportadas">
              <td>Média simples</td>
              <td className="r">{formatNumber(found.simple, MEAN_DIGITS)} m/s</td>
            </tr>
            <tr title="A média da distribuição das velocidades em faixas de 1 m/s, como o Openwind calcula">
              <td>Média da distribuição</td>
              <td className="r">{formatNumber(found.binned, MEAN_DIGITS)} m/s</td>
            </tr>
            <tr title="A média da distribuição sem viés sazonal (cada mês pesado pelos seus dias), em faixas de 1 m/s">
              <td>Média sem viés sazonal</td>
              <td className="r">{formatNumber(found.unbiased, MEAN_DIGITS)} m/s</td>
            </tr>
            <tr>
              <td>Passos exportados</td>
              <td className="r">{formatCount(found.steps)}</td>
            </tr>
          </tbody>
        </table>
      )}
      <label className="check" title="Escala todas as velocidades exportadas (e seus desvios, que mantêm a TI) para que a média simples seja a escolhida.">
        <input type="checkbox" checked={scale} onChange={(event) => setScale(event.target.checked)} />
        Escalar as velocidades para a média (m/s)
        <input inputMode="decimal" value={targetText} disabled={!scale} onChange={(event) => setTargetText(event.target.value)} />
      </label>
      {scale && <p className="hint">Fator de escala: {factor === undefined ? NO_VALUE : formatNumber(factor, FACTOR_DIGITS)}</p>}
      <p className="hint">
        Arquivo de torre (MM 2.1) de uma altura: velocidade, direção, temperatura, densidade do ar e TI de cada passo com todos os valores
        válidos.
      </p>
      <button className="btn" disabled={busy || (scale && !targetValid)} onClick={exportOpenwind}>
        {busy ? 'Exportando…' : 'Exportar Openwind'}
      </button>
    </div>
  );
}

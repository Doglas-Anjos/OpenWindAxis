/**
 * The EPE part of the export card (wind-core `export::epe`): the file the
 * Brazilian Energy Research Agency takes wind measurements in, as its note
 * NT EPE-DEE-RE-057/2016-R4 specifies. Each sensor (upper, intermediate and
 * lower anemometers, upper and lower vanes, barometer, thermometer,
 * hygrometer) takes a column, or is chosen automatically, or is marked as
 * missing; the file is named as EPE asks.
 */
import { useState } from 'react';
import type { WindApi } from './api/client';
import type { ColumnDto } from './api/generated/ColumnDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { ExportEpeRequest } from './api/generated/ExportEpeRequest';
import { errorMessage } from './format';
import { textFile } from './workbook/useWorkbook';

/** Select value of "chosen automatically" (the field left out of the request) and of "left out" (`""`). */
const AUTOMATIC = 'auto';
const LEFT_OUT = '';
const MS_PER_DAY = 86_400_000;
/** The step EPE takes, s, and its clock: UTC−3 without daylight saving (NT EPE-DEE-RE-057/2016-R4, 5.11). */
const EPE_STEP_S = 600;
const EPE_UTC_OFFSET_MIN = -180;
const EPE_MEDIA_TYPE = 'text/plain';

type GroupField = 'upperSpeed' | 'middleSpeed' | 'lowerSpeed' | 'upperVane' | 'lowerVane' | 'pressure' | 'temperature' | 'humidity';
/** The groups, their labels and the columns each takes. */
const GROUPS: { field: GroupField; label: string; takes: (column: ColumnDto) => boolean }[] = [
  { field: 'upperSpeed', label: 'Anemômetro superior', takes: (column) => column.kind === 'wind_speed' },
  { field: 'middleSpeed', label: 'Anemômetro intermediário', takes: (column) => column.kind === 'wind_speed' },
  { field: 'lowerSpeed', label: 'Anemômetro inferior', takes: (column) => column.kind === 'wind_speed' },
  { field: 'upperVane', label: 'Wind vane superior', takes: (column) => column.kind === 'wind_direction' },
  { field: 'lowerVane', label: 'Wind vane inferior', takes: (column) => column.kind === 'wind_direction' },
  { field: 'pressure', label: 'Pressão', takes: (column) => column.kind === 'pressure' },
  { field: 'temperature', label: 'Temperatura', takes: (column) => column.kind === 'temperature' },
  { field: 'humidity', label: 'Umidade relativa', takes: (column) => column.kind === 'relative_humidity' },
];

/** The digits of a name, the usual EPE station number ("1234567_torre.csv" → "1234567"). */
const digitsOf = (name: string) => /\d{4,}/.exec(name)?.[0] ?? '';
/** Epoch ms of the start of a "YYYY-MM-DD" day, UTC. */
const dayStartMs = (day: string) => Date.parse(`${day}T00:00:00Z`);

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  onDownload: (name: string, contents: Blob) => void;
  onError: (message: string) => void;
}

/** Form and button that download the EPE file of the dataset. */
export function EpeExport({ api, dataset, onDownload, onError }: Props) {
  const [station, setStation] = useState(() => digitsOf(dataset.name));
  const [fromDay, setFromDay] = useState('');
  const [toDay, setToDay] = useState('');
  const [choices, setChoices] = useState<Record<GroupField, string>>(() => Object.fromEntries(GROUPS.map(({ field }) => [field, AUTOMATIC])) as Record<GroupField, string>);
  const [busy, setBusy] = useState(false);
  const means = dataset.columns.filter((column) => column.subtype === 'mean');

  const exportEpe = async () => {
    setBusy(true);
    try {
      const request: ExportEpeRequest = { dataset: dataset.id, station: station.trim() };
      if (fromDay) request.fromMs = dayStartMs(fromDay);
      if (toDay) request.toMs = dayStartMs(toDay) + MS_PER_DAY - 1;
      for (const { field } of GROUPS) if (choices[field] !== AUTOMATIC) request[field] = choices[field];
      const file = await api.exportEpe(request);
      onDownload(file.name, textFile(file.text, EPE_MEDIA_TYPE));
    } catch (thrown) {
      onError(errorMessage(thrown));
    }
    setBusy(false);
  };

  return (
    <div className="epe">
      <div className="properties">
        <label>
          Código da estação (numérico)
          <input value={station} placeholder="ex.: 1234567" onChange={(event) => setStation(event.target.value)} />
        </label>
        <label>
          De (dia, vazio = início)
          <input type="date" value={fromDay} onChange={(event) => setFromDay(event.target.value)} />
        </label>
        <label>
          Até (dia, vazio = fim)
          <input type="date" value={toDay} onChange={(event) => setToDay(event.target.value)} />
        </label>
        {GROUPS.map(({ field, label, takes }) => (
          <label key={field}>
            {label}
            <select value={choices[field]} onChange={(event) => setChoices({ ...choices, [field]: event.target.value })}>
              <option value={AUTOMATIC}>Automático</option>
              <option value={LEFT_OUT}>A estação não tem</option>
              {means.filter(takes).map((column) => (
                <option key={column.id} value={column.id}>
                  {column.id}
                </option>
              ))}
            </select>
          </label>
        ))}
      </div>
      <div className="actions">
        <button className="btn" disabled={busy || !station.trim()} onClick={exportEpe}>
          Baixar arquivo EPE
        </button>
      </div>
      {dataset.stepS !== EPE_STEP_S && <div className="note err">A EPE recebe intervalos de 10 minutos; este dataset tem outro passo.</div>}
      {dataset.utcOffsetMin != null && dataset.utcOffsetMin !== EPE_UTC_OFFSET_MIN && (
        <div className="note warn">A EPE pede os horários em UTC−3, sem horário de verão; o relógio deste dataset está em outro fuso.</div>
      )}
      <p className="hint">
        Conforme a NT EPE-DEE-RE-057/2016-R4: 22 canais separados por “|”, vírgula decimal, registros entre “dados” e “fimdados”. Valor ausente, excluído por flag ou
        implausível (pressão fora de 800–1060 hPa, temperatura fora de −15–50 °C, umidade fora de 0–110 %, velocidade média acima de 50 m/s ou máxima acima de 70 m/s)
        sai como “-” e soma o bit do sensor no código de erro. Automático: os anemômetros e wind vanes pela altura (o mais alto, o mais baixo e, com três ou mais, um
        intermediário) e a pressão, a temperatura e a umidade primárias. A EPE pede os horários em UTC−3 sem horário de verão: o relógio do dataset vai como está.
      </p>
    </div>
  );
}

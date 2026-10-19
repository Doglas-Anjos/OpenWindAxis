/**
 * Revise › Configure Dataset (ESPEC-INTERFACE §2.3): the dataset at the top,
 * then tabs. "Propriedades básicas" holds the site (latitude, longitude,
 * elevation), the period and step, what the timestamps mark and how the file
 * is read; "Colunas" the column table with type, subtype and height
 * corrections and each column's statistics; "Colunas primárias" the column
 * that represents each type; "Altura de deslocamento" the height the shear
 * laws take off the sensors'; "Exibição de colunas" how the columns are
 * named, ordered, shown and coloured (applied without re-reading the file). OK re-reads the file when something changed;
 * Cancel drops the column corrections made here.
 */
import { useState } from 'react';
import type { CalculatedColumnDto } from '../api/generated/CalculatedColumnDto';
import type { ColumnKind } from '../api/generated/ColumnKind';
import type { ColumnSubtype } from '../api/generated/ColumnSubtype';
import type { DatasetId } from '../api/generated/DatasetId';
import type { TimestampMeaning } from '../api/generated/TimestampMeaning';
import { formatCount, formatNumber, formatPercent, NO_VALUE } from '../format';
import { COLUMN_KIND_LABEL, COLUMN_SUBTYPE_LABEL, DELIMITER_LABEL, TIMESTAMP_MEANING_LABEL } from '../labels';
import { boomsOf, coordinateText, formatDateTime, formatStep, isDerived, lastStepMs, optionsOf, type Entry, type ImportOptions } from '../workbook/model';
import type { Workbook } from '../workbook/useWorkbook';
import type { WindApi } from '../api/client';
import type { DisplacementDto } from '../api/generated/DisplacementDto';
import type { PrimaryColumnDto } from '../api/generated/PrimaryColumnDto';
import { DisplacementTab } from './DisplacementTab';
import { ColumnDisplayTab } from './ColumnDisplayTab';
import type { ColumnDisplayDto } from '../api/generated/ColumnDisplayDto';
import { DEFAULT_DISPLAY } from '../display';
import { AMMONIT_FORMAT, POWER_FORMAT, SYMPRO_FORMAT, TOA5_FORMAT } from '../api/generated/version';
import { Modal } from './Modal';
import { PrimaryColumnsTab } from './PrimaryColumnsTab';
import { CalculatedColumns } from './CalculatedColumns';

/** The IEC cut a dataset may choose for TI, m/s (wind-core `TI_IEC_MIN_SPEED_M_S`). */
const TI_IEC_MIN_SPEED_M_S = 4;

type Tab = 'basic' | 'columns' | 'primary' | 'displacement' | 'display';

/** Tooltip of a derived column's tag. */
const derivedTitle = (sources: string[]) =>
  `Calculada a partir de ${sources.join(', ')}; recalculada a cada importação. Herda as marcações dessas colunas e não recebe marcação própria.`;

interface Props {
  api: WindApi;
  entries: Entry[];
  initialDataset: DatasetId;
  workbook: Workbook;
  onClose: () => void;
}

export function ConfigureDatasetDialog({ api, entries, initialDataset, workbook, onClose }: Props) {
  const [chosen, setChosen] = useState<DatasetId>(initialDataset);
  const entry = entries.find((candidate) => candidate.result.dataset.id === chosen);
  const [tab, setTab] = useState<Tab>('basic');
  const [options, setOptions] = useState<ImportOptions>(() => (entry ? optionsOf(entry.config) : optionsOf({ columns: [] })));
  const [primary, setPrimary] = useState<PrimaryColumnDto[]>(() => entry?.config.primary ?? []);
  const [displacement, setDisplacement] = useState<DisplacementDto | undefined>(() => entry?.config.displacement);
  const [calculated, setCalculated] = useState<CalculatedColumnDto[]>(() => entry?.config.calculated ?? []);
  const [tiCutoff, setTiCutoff] = useState(() => entry?.config.tiCutoff != null);
  const [display, setDisplay] = useState<ColumnDisplayDto>(() => entry?.display ?? DEFAULT_DISPLAY);
  /** The corrections as they were when the dialog opened, for Cancel. */
  const [opened] = useState(() => (entry ? { corrections: entry.corrections, dirty: entry.dirty } : null));
  if (!entry || !opened) return null;
  const { dataset, report, summaries } = entry.result;
  const primaryChanged = JSON.stringify(primary) !== JSON.stringify(entry.config.primary ?? []);
  const displacementChanged = JSON.stringify(displacement) !== JSON.stringify(entry.config.displacement);
  const calculatedChanged = JSON.stringify(calculated) !== JSON.stringify(entry.config.calculated ?? []);
  const tiCutoffChanged = tiCutoff !== (entry.config.tiCutoff != null);
  const optionsChanged =
    JSON.stringify(options) !== JSON.stringify(optionsOf(entry.config)) || primaryChanged || displacementChanged || calculatedChanged || tiCutoffChanged;
  const calculatedIds = new Set((entry.config.calculated ?? []).map((spec) => spec.id));
  const set = (patch: Partial<ImportOptions>) => setOptions({ ...options, ...patch });
  const cancel = () => {
    workbook.updateEntry(dataset.id, () => opened);
    onClose();
  };
  const displayChanged = JSON.stringify(display) !== JSON.stringify(entry.display);
  const accept = async () => {
    onClose();
    if (displayChanged) workbook.updateEntry(dataset.id, () => ({ display }));
    const patch = {
      ...(primaryChanged && { primary: primary.length ? primary : undefined }),
      ...(displacementChanged && { displacement }),
      ...(calculatedChanged && { calculated: calculated.length ? calculated : undefined }),
      ...(tiCutoffChanged && { tiCutoff: tiCutoff ? TI_IEC_MIN_SPEED_M_S : undefined }),
    };
    if (optionsChanged || entry.dirty) await workbook.reapply(entry, options, patch);
  };
  const choose = (id: DatasetId) => {
    const next = entries.find((candidate) => candidate.result.dataset.id === id);
    if (!next) return;
    setChosen(id);
    setOptions(optionsOf(next.config));
    setPrimary(next.config.primary ?? []);
    setDisplacement(next.config.displacement);
    setCalculated(next.config.calculated ?? []);
    setTiCutoff(next.config.tiCutoff != null);
    setDisplay(next.display);
  };

  return (
    <Modal
      title="Configurar dataset"
      onClose={cancel}
      wide
      buttons={
        <>
          <button className="btn primary" onClick={accept}>
            OK
          </button>
          <button className="btn" onClick={cancel}>
            Cancelar
          </button>
        </>
      }
    >
      <label className="dataset-select">
        Dataset
        <select value={dataset.id} onChange={(event) => choose(Number(event.target.value))} disabled={optionsChanged || displayChanged || entry.dirty} title={optionsChanged || displayChanged || entry.dirty ? 'Confirme ou cancele as mudanças antes de trocar de dataset' : undefined}>
          {entries.map((candidate) => (
            <option key={candidate.result.dataset.id} value={candidate.result.dataset.id}>
              {candidate.result.dataset.name}
            </option>
          ))}
        </select>
      </label>
      <div className="dialog-tabs" role="tablist">
        <button role="tab" aria-selected={tab === 'basic'} className={tab === 'basic' ? 'on' : ''} onClick={() => setTab('basic')}>
          Propriedades básicas
        </button>
        <button role="tab" aria-selected={tab === 'columns'} className={tab === 'columns' ? 'on' : ''} onClick={() => setTab('columns')}>
          Colunas
        </button>
        <button role="tab" aria-selected={tab === 'primary'} className={tab === 'primary' ? 'on' : ''} onClick={() => setTab('primary')}>
          Colunas primárias
        </button>
        <button role="tab" aria-selected={tab === 'displacement'} className={tab === 'displacement' ? 'on' : ''} onClick={() => setTab('displacement')}>
          Altura de deslocamento
        </button>
        <button role="tab" aria-selected={tab === 'display'} className={tab === 'display' ? 'on' : ''} onClick={() => setTab('display')}>
          Exibição de colunas
        </button>
      </div>

      {tab === 'display' ? (
        <ColumnDisplayTab dataset={dataset} value={display} onChange={setDisplay} />
      ) : tab === 'displacement' ? (
        <DisplacementTab api={api} dataset={dataset} value={displacement} onChange={setDisplacement} />
      ) : tab === 'primary' ? (
        <PrimaryColumnsTab dataset={dataset} choices={primary} onChange={setPrimary} />
      ) : tab === 'basic' ? (
        <div className="properties">
          <fieldset>
            <legend>Geral</legend>
            <label>
              Nome
              <input readOnly value={dataset.name} />
            </label>
          </fieldset>
          <fieldset>
            <legend>Localização</legend>
            <label>
              Latitude (°, sul negativa)
              <input inputMode="decimal" value={options.latitude} placeholder={dataset.latitude == null ? 'Desconhecida' : coordinateText(dataset.latitude)} onChange={(event) => set({ latitude: event.target.value })} />
            </label>
            <label>
              Longitude (°, oeste negativa)
              <input inputMode="decimal" value={options.longitude} placeholder={dataset.longitude == null ? 'Desconhecida' : coordinateText(dataset.longitude)} onChange={(event) => set({ longitude: event.target.value })} />
            </label>
            <label title="Com temperatura e sem coluna de pressão, a densidade do ar usa a pressão da atmosfera padrão (ISO 2533) nesta elevação.">
              Elevação (m)
              <input inputMode="decimal" value={options.elevationM} placeholder="Desconhecida" onChange={(event) => set({ elevationM: event.target.value })} />
            </label>
            <p className="hint">Em branco, latitude e longitude vêm do cabeçalho do logger quando ele as traz.</p>
          </fieldset>
          <fieldset>
            <legend>Intensidade de turbulência</legend>
            <label className="check" title="Sem marcar, como o original: DP ÷ média com média acima de 0,01 m/s, e 0 abaixo. Marcado, a TI fica indefinida com média abaixo de 4 m/s, o início da faixa de calibração dos anemômetros (IEC 61400-12-1, anexo F).">
              <input type="checkbox" checked={tiCutoff} onChange={(event) => setTiCutoff(event.target.checked)} />
              Só com velocidade a partir de {TI_IEC_MIN_SPEED_M_S} m/s
            </label>
          </fieldset>
          <fieldset>
            <legend>Período e passo</legend>
            <label>
              Início
              <input readOnly value={formatDateTime(dataset.startMs)} />
            </label>
            <label>
              Fim (último passo)
              <input readOnly value={formatDateTime(lastStepMs(dataset))} />
            </label>
            <label>
              Passo
              <input readOnly value={formatStep(dataset.stepS)} />
            </label>
            <label>
              O carimbo de tempo indica
              <select value={options.timestampMeaning} onChange={(event) => set({ timestampMeaning: event.target.value as TimestampMeaning })}>
                {Object.entries(TIMESTAMP_MEANING_LABEL).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          </fieldset>
          <fieldset>
            <legend>Leitura do arquivo</legend>
            <label>
              Separador
              <select value={options.delimiter} onChange={(event) => set({ delimiter: event.target.value })}>
                <option value="">Automático</option>
                <option value=",">Vírgula</option>
                <option value=";">Ponto e vírgula</option>
                <option value={'\t'}>Tab</option>
                <option value="|">Barra</option>
              </select>
            </label>
            <label>
              Decimal
              <select value={options.decimal} onChange={(event) => set({ decimal: event.target.value as ImportOptions['decimal'] })}>
                <option value="">Automático</option>
                <option value="dot">Ponto</option>
                <option value="comma">Vírgula</option>
              </select>
            </label>
            <label>
              Formato da data
              <input value={options.timeFormat} placeholder="Automático, ex.: %d/%m/%Y %H:%M" onChange={(event) => set({ timeFormat: event.target.value })} />
            </label>
            <p className="hint">
              Detectado: separador {DELIMITER_LABEL[report.delimiter] ?? report.delimiter} · decimal {report.decimalComma ? 'vírgula' : 'ponto'} · data{' '}
              <code>{report.timeFormat}</code>
              {report.timeColumns.length > 1 && ' em duas colunas'} · {formatCount(report.rowsRead)} linhas lidas · {formatCount(report.missingSteps)} passos faltando ·{' '}
              {report.duplicates} duplicadas
              {report.offGrid > 0 && ` · ${report.offGrid} fora da grade`}
              {report.rowsBadTime > 0 && ` · ${report.rowsBadTime} com data inválida`}
            </p>
            {report.format && (
              <p className="hint format-note">
                Formato reconhecido: <b>{report.format}</b>.{' '}
                {report.format === TOA5_FORMAT && 'Os carimbos da Campbell marcam o fim de cada intervalo: confira "Carimbo indica" acima.'}
                {report.format === POWER_FORMAT && 'A NASA POWER escreve a hora local solar (LST), salvo se o arquivo foi pedido em UTC.'}
                {report.format === AMMONIT_FORMAT && 'Alturas, local e fuso vêm da parte de configuração do arquivo; contagens, somas, medianas e o endereço ficam de fora.'}
                {report.format === SYMPRO_FORMAT && 'Os canais analógicos tomam o nome do sensor no Sensor History; local e fuso vêm de Site Properties.'}
              </p>
            )}
          </fieldset>
        </div>
      ) : (
        <div className="tablewrap">
          <table className="grid columns-table">
            <thead>
              <tr>
                <th>ID</th>
                <th>Tipo</th>
                <th>Subtipo</th>
                <th>Altura (m)</th>
                <th title="Orientação da lança do sensor, graus a partir do norte (Calibração › Boom orientation do original): o sensor fica à sombra da torre com o vento do lado oposto, 180° dela.">
                  Lança (°)
                </th>
                <th>Unidade</th>
                <th>Associada a</th>
                <th className="r" title="Após descontar os trechos marcados">
                  Recuperação
                </th>
                <th className="r">Média</th>
                <th className="r">Mín</th>
                <th className="r">Máx</th>
              </tr>
            </thead>
            <tbody>
              {dataset.columns.map((column, position) => {
                const correction = entry.corrections[report.fileColumnByColumn[position]] ?? {};
                const summary = summaries[position];
                const unparsed = report.unparsedByColumn[position];
                // A correction's height wins, including null ("no height").
                const height = correction.heightM !== undefined ? correction.heightM : column.heightM;
                return (
                  <tr key={column.id} className={`kind-${column.kind}`}>
                    <td className="id">
                      {column.id}
                      {column.derivedFrom &&
                        (calculatedIds.has(column.id) ? (
                          <span className="tag" title={`Coluna calculada a partir de ${column.derivedFrom.join(', ')}, com a filtragem padrão.`}>
                            calculada
                          </span>
                        ) : (
                          <span className="tag" title={derivedTitle(column.derivedFrom)}>
                            derivada
                          </span>
                        ))}
                      {unparsed > 0 && (
                        <span className="warn" title="Campos que não são números">
                          {' '}
                          ⚠ {unparsed}
                        </span>
                      )}
                    </td>
                    <td>
                      <select
                        aria-label={`Tipo de ${column.id}`}
                        disabled={isDerived(column)}
                        value={correction.kind ?? column.kind}
                        onChange={(event) => workbook.setCorrection(entry, position, { kind: event.target.value as ColumnKind })}
                      >
                        {Object.entries(COLUMN_KIND_LABEL).map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <select
                        aria-label={`Subtipo de ${column.id}`}
                        disabled={isDerived(column)}
                        value={correction.subtype ?? column.subtype}
                        onChange={(event) => workbook.setCorrection(entry, position, { subtype: event.target.value as ColumnSubtype })}
                      >
                        {Object.entries(COLUMN_SUBTYPE_LABEL).map(([value, label]) => (
                          <option key={value} value={value}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <input
                        className="num"
                        type="number"
                        step="any"
                        aria-label={`Altura de ${column.id}`}
                        disabled={isDerived(column)}
                        value={height ?? ''}
                        onChange={(event) => workbook.setCorrection(entry, position, { heightM: event.target.value === '' ? null : Number(event.target.value) })}
                      />
                    </td>
                    <td>
                      <input
                        className="num"
                        type="number"
                        step="any"
                        min={0}
                        max={360}
                        aria-label={`Lança de ${column.id}`}
                        disabled={isDerived(column) || !(column.kind === 'wind_speed' || column.kind === 'wind_direction') || column.subtype !== 'mean'}
                        value={correction.boomDeg ?? ''}
                        onChange={(event) => workbook.setCorrection(entry, position, { boomDeg: event.target.value === '' ? undefined : Number(event.target.value) })}
                      />
                    </td>
                    <td>{column.units || NO_VALUE}</td>
                    <td>{column.parent ?? column.derivedFrom?.join(', ') ?? NO_VALUE}</td>
                    <td className="r">{formatPercent(summary.recovery)}</td>
                    <td className="r">{formatNumber(summary.mean)}</td>
                    <td className="r">{formatNumber(summary.min)}</td>
                    <td className="r">{formatNumber(summary.max)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {dataset.columns.some(isDerived) && (
            <p className="hint">
              Colunas derivadas: intensidade de turbulência (DP ÷ média, como no original: 0 com média de até 0,01 m/s, ou indefinida abaixo de{' '}
              {TI_IEC_MIN_SPEED_M_S} m/s se escolhido em Propriedades básicas), densidade de potência (½·ρ·U³) e densidade do
              ar (ar úmido da IEC 61400-12-1, com a temperatura, a pressão e a umidade medidas, ou a pressão da atmosfera padrão na elevação do local e 50 % de umidade).
            </p>
          )}
          {entry.dirty && <p className="hint">As correções valem ao confirmar com OK: o arquivo é lido de novo, e as marcações acompanham.</p>}
          <CalculatedColumns
            api={api}
            dataset={dataset}
            booms={boomsOf(entry)}
            calculated={calculated}
            onChange={setCalculated}
          />
        </div>
      )}
    </Modal>
  );
}

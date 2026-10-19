/**
 * Configurar dataset › Altura de deslocamento (ESPEC-INTERFACE §2.3): the
 * height where the wind reaches zero over vegetation, constant, by month or
 * by the sector of a direction sensor, with a backup for steps without a
 * direction ("Calcular": the sectors' mean, weighted by how often the wind
 * blows from each). The shear laws, the vertical extrapolation and the
 * reconstruction from another height use heights less it. OK re-reads the
 * file with it.
 */
import { useState } from 'react';
import type { WindApi } from '../api/client';
import type { DatasetDto } from '../api/generated/DatasetDto';
import type { DisplacementDto } from '../api/generated/DisplacementDto';
import { errorMessage, formatNumber } from '../format';
import { MONTH_ABBREVIATIONS } from '../labels';
import { isDerived, isMeanDirection } from '../workbook/model';

const MONTHS = MONTH_ABBREVIATIONS.length;
const DEGREES_PER_TURN = 360;
/** Sector counts offered. */
const SECTOR_CHOICES = [4, 8, 12, 16];
const DEFAULT_SECTORS = 12;
/** Decimals of the computed backup. */
const BACKUP_DIGITS = 2;
type Mode = 'none' | DisplacementDto['by'];
const MODE_LABEL: Record<Mode, string> = { none: 'Nenhuma', constant: 'Constante', month: 'Por mês', sector: 'Por setor da direção' };

/** A typed height: a finite number not below 0, else `fallback`. */
const height = (typed: string, fallback: number) => {
  const value = Number(typed.replace(',', '.'));
  return typed.trim() !== '' && Number.isFinite(value) && value >= 0 ? value : fallback;
};

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  value: DisplacementDto | undefined;
  onChange: (value: DisplacementDto | undefined) => void;
}

export function DisplacementTab({ api, dataset, value, onChange }: Props) {
  const [error, setError] = useState('');
  const directions = dataset.columns.filter((column) => isMeanDirection(column) && !isDerived(column));
  const constantOf = (current: DisplacementDto | undefined) =>
    current?.by === 'constant' ? current.heightM : current?.by === 'sector' ? current.backupM : (current?.heightsM[0] ?? 0);
  const switchTo = (mode: Mode) => {
    const base = constantOf(value);
    if (mode === 'none') onChange(undefined);
    else if (mode === 'constant') onChange({ by: 'constant', heightM: base });
    else if (mode === 'month') onChange({ by: 'month', heightsM: Array(MONTHS).fill(base) });
    else onChange({ by: 'sector', direction: directions[0]?.id ?? '', heightsM: Array(DEFAULT_SECTORS).fill(base), backupM: base });
  };
  const setAt = (list: number[], index: number, typed: string) => list.map((item, position) => (position === index ? height(typed, item) : item));
  const calculate = async () => {
    if (value?.by !== 'sector') return;
    try {
      const rose = await api.windRose({ dataset: dataset.id, direction: value.direction, sectors: value.heightsM.length });
      const weights = rose.frequency.map((share) => share ?? 0);
      const total = weights.reduce((sum, weight) => sum + weight, 0);
      if (!(total > 0)) throw new Error('Sem direções válidas para ponderar os setores.');
      const backupM = weights.reduce((sum, weight, sector) => sum + weight * value.heightsM[sector], 0) / total;
      onChange({ ...value, backupM: Number(backupM.toFixed(BACKUP_DIGITS)) });
      setError('');
    } catch (thrown) {
      setError(errorMessage(thrown));
    }
  };

  return (
    <div className="properties displacement">
      <fieldset>
        <legend>Altura de deslocamento</legend>
        <div className="choices" role="radiogroup">
          {(Object.keys(MODE_LABEL) as Mode[]).map((mode) => (
            <label key={mode} className="check">
              <input type="radio" checked={(value?.by ?? 'none') === mode} disabled={mode === 'sector' && !directions.length} onChange={() => switchTo(mode)} />
              {MODE_LABEL[mode]}
            </label>
          ))}
        </div>
        {value?.by === 'constant' && (
          <label>
            Altura (m)
            <input className="num" defaultValue={value.heightM} onChange={(event) => onChange({ by: 'constant', heightM: height(event.target.value, value.heightM) })} />
          </label>
        )}
        {value?.by === 'month' && (
          <div className="grid-inputs">
            {MONTH_ABBREVIATIONS.map((month, index) => (
              <label key={month}>
                {month} (m)
                <input className="num" defaultValue={value.heightsM[index]} onChange={(event) => onChange({ by: 'month', heightsM: setAt(value.heightsM, index, event.target.value) })} />
              </label>
            ))}
          </div>
        )}
        {value?.by === 'sector' && (
          <>
            <label>
              Sensor de direção
              <select value={value.direction} onChange={(event) => onChange({ ...value, direction: event.target.value })}>
                {directions.map((column) => (
                  <option key={column.id}>{column.id}</option>
                ))}
              </select>
            </label>
            <label>
              Setores
              <select
                value={value.heightsM.length}
                onChange={(event) => onChange({ ...value, heightsM: Array(Number(event.target.value)).fill(constantOf(value)) })}
              >
                {SECTOR_CHOICES.map((sectors) => (
                  <option key={sectors}>{sectors}</option>
                ))}
              </select>
            </label>
            <div className="grid-inputs" key={value.heightsM.length}>
              {value.heightsM.map((heightM, index) => (
                <label key={index}>
                  {formatNumber((index * DEGREES_PER_TURN) / value.heightsM.length, 1)}° (m)
                  <input className="num" defaultValue={heightM} onChange={(event) => onChange({ ...value, heightsM: setAt(value.heightsM, index, event.target.value) })} />
                </label>
              ))}
            </div>
            <div className="backup">
              <label>
                Reserva, sem direção (m)
                <input className="num" key={value.backupM} defaultValue={value.backupM} onChange={(event) => onChange({ ...value, backupM: height(event.target.value, value.backupM) })} />
              </label>
              <button className="btn" onClick={calculate}>
                Calcular
              </button>
            </div>
          </>
        )}
        {error && <div className="note err">{error}</div>}
        <p className="hint">
          Acima de vegetação o vento zera acima do solo, na altura de deslocamento (por regra prática, 0,6 a 0,9 vez a altura da vegetação). O cisalhamento, a
          extrapolação vertical e a reconstrução por outra altura usam a altura do sensor menos ela. Vale ao confirmar com OK: o arquivo é lido de novo.
        </p>
      </fieldset>
    </div>
  );
}

/**
 * Revisar › Extrapolação vertical (manual dlg-revise-vertical-extrapolation.htm):
 * speed, its SD, and optionally direction and temperature synthesized at
 * other heights as derived columns (crates/wind-core/src/extrapolation.rs).
 * Each step fits the chosen sensors (power law, veer, gradient), with the
 * month × hour table of medians where it cannot; the profile passes through
 * the sensor nearest the height, a chosen one, or each step's line of best
 * fit. The list lives in the dataset's import configuration, so it is saved
 * with the workbook and redone on every re-read; OK re-reads the file and,
 * when asked, flags the new columns.
 */
import { useState } from 'react';
import type { ColumnDto } from '../api/generated/ColumnDto';
import type { DatasetId } from '../api/generated/DatasetId';
import type { ExtrapolationDto } from '../api/generated/ExtrapolationDto';
import { formatNumber } from '../format';
import { isDerived, isMeanSpeed, lastStepMs, optionsOf, typedNumber, type Entry } from '../workbook/model';
import type { Workbook } from '../workbook/useWorkbook';
import { Modal } from './Modal';

/** The original's default limits of a step's exponent (wind-core DEFAULT_ALPHA_RANGE). */
const DEFAULT_ALPHA_MIN = '-0.25';
const DEFAULT_ALPHA_MAX = '1.25';
/** "Extrapolar a partir de": the nearest sensor, the line of best fit, or `sensor:<id>`. */
const NEAREST = 'nearest';
const BEST_FIT = 'best_fit';
const SENSOR_PREFIX = 'sensor:';
/** Ids of the columns an extrapolation synthesizes (and may flag): not their TI or WPD. */
const SYNTHESIZED_ID = / extrapolada( DP)?(_\d+)?$/;
/** Heights typed together: "120; 140" or "120 140". */
const HEIGHT_SEPARATORS = /[;\s]+/;

interface Props {
  entries: Entry[];
  initialDataset: DatasetId;
  workbook: Workbook;
  onClose: () => void;
}

/** Checkboxes over `columns`, all ticked at first. */
function SensorChecks({ legend, columns, chosen, onChange }: { legend: string; columns: ColumnDto[]; chosen: string[]; onChange: (ids: string[]) => void }) {
  return (
    <fieldset className="columns">
      <legend>{legend}</legend>
      {columns.map((column) => (
        <label key={column.id} className="check">
          <input
            type="checkbox"
            checked={chosen.includes(column.id)}
            onChange={() => onChange(chosen.includes(column.id) ? chosen.filter((id) => id !== column.id) : columns.map((each) => each.id).filter((id) => id === column.id || chosen.includes(id)))}
          />
          {column.id} · {formatNumber(column.heightM)} m
        </label>
      ))}
    </fieldset>
  );
}

/** One line of the list. */
function describe(spec: ExtrapolationDto): string {
  const sensors = spec.sensors ?? [spec.lower, spec.upper];
  const parts = [`${formatNumber(spec.heightM)} m, de ${sensors.join(', ')}`];
  if (spec.from?.kind === 'best_fit') parts.push('pela reta de melhor ajuste');
  if (spec.from?.kind === 'sensor') parts.push(`a partir de ${spec.from.column}`);
  if (spec.alphaRange) parts.push(`α entre ${formatNumber(spec.alphaRange[0])} e ${formatNumber(spec.alphaRange[1])}`);
  if (spec.vanes?.length) parts.push('com direção');
  if (spec.thermometers?.length) parts.push('com temperatura');
  if (spec.tableOnly) parts.push('só a tabela');
  return parts.join(' · ');
}

export function ExtrapolationDialog({ entries, initialDataset, workbook, onClose }: Props) {
  const entry = entries.find((candidate) => candidate.result.dataset.id === initialDataset) ?? entries[0];
  const dataset = entry.result.dataset;
  const measured = (kind: ColumnDto['kind']) =>
    dataset.columns.filter((column) => column.kind === kind && column.subtype === 'mean' && !isDerived(column) && column.heightM != null);
  const speeds = dataset.columns.filter((column) => isMeanSpeed(column) && !isDerived(column) && column.heightM != null);
  const vanes = measured('wind_direction');
  const thermometers = measured('temperature');
  const [list, setList] = useState<ExtrapolationDto[]>(() => entry.config.extrapolations ?? []);
  const [sensors, setSensors] = useState(() => speeds.map((column) => column.id));
  const [from, setFrom] = useState(NEAREST);
  const [tableOnly, setTableOnly] = useState(false);
  const [restrict, setRestrict] = useState(false);
  const [alphaMin, setAlphaMin] = useState(DEFAULT_ALPHA_MIN);
  const [alphaMax, setAlphaMax] = useState(DEFAULT_ALPHA_MAX);
  const [withDirection, setWithDirection] = useState(false);
  const [vaneIds, setVaneIds] = useState(() => vanes.map((column) => column.id));
  const [withTemperature, setWithTemperature] = useState(false);
  const [thermometerIds, setThermometerIds] = useState(() => thermometers.map((column) => column.id));
  const [heights, setHeights] = useState('');
  const [flagNew, setFlagNew] = useState(false);
  const [flag, setFlag] = useState(() => Math.max(0, dataset.flags.findIndex((known) => known.include)));

  const heightOf = (id: string) => dataset.columns.find((column) => column.id === id)?.heightM ?? 0;
  const atTwoHeights = (ids: string[]) => new Set(ids.map(heightOf)).size >= 2;
  const typedHeights = heights.split(HEIGHT_SEPARATORS).filter(Boolean).map(typedNumber);
  const [low, high] = [typedNumber(alphaMin), typedNumber(alphaMax)];
  const rangeValid = !restrict || (low !== undefined && high !== undefined && Number.isFinite(low) && Number.isFinite(high) && low <= high);
  const addable = atTwoHeights(sensors) && typedHeights.length > 0 && typedHeights.every((height) => height !== undefined && height > 0) && rangeValid;
  const add = () => {
    if (!addable) return;
    const ordered = [...sensors].sort((first, second) => heightOf(first) - heightOf(second));
    const chosenFrom: ExtrapolationDto['from'] =
      from === BEST_FIT && !tableOnly ? { kind: 'best_fit' } : from.startsWith(SENSOR_PREFIX) ? { kind: 'sensor', column: from.slice(SENSOR_PREFIX.length) } : undefined;
    const added = typedHeights.map((heightM) => ({
      lower: ordered[0],
      upper: ordered[ordered.length - 1],
      heightM: heightM!,
      sensors: ordered,
      tableOnly: tableOnly || undefined,
      from: chosenFrom,
      alphaRange: restrict ? ([low!, high!] as [number, number]) : undefined,
      vanes: withDirection && atTwoHeights(vaneIds) ? vaneIds : undefined,
      thermometers: withTemperature && atTwoHeights(thermometerIds) ? thermometerIds : undefined,
    }));
    setList([...list, ...added]);
    setHeights('');
  };
  const accept = async () => {
    onClose();
    const result = await workbook.reapply(entry, optionsOf(entry.config), { extrapolations: list.length ? list : undefined });
    if (!flagNew || !result) return;
    const synthesized = result.dataset.columns.filter((column) => isDerived(column) && SYNTHESIZED_ID.test(column.id)).map((column) => column.id);
    if (synthesized.length) {
      await workbook.applyOp(entry, { op: 'flag', columns: synthesized, flag, fromMs: result.dataset.startMs, toMs: lastStepMs(result.dataset) });
    }
  };
  return (
    <Modal
      title={`Extrapolação vertical — ${dataset.name}`}
      onClose={onClose}
      buttons={
        <>
          <button className="btn primary" onClick={accept}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
        </>
      }
    >
      {speeds.length < 2 ? (
        <p>O dataset precisa de duas velocidades médias medidas em alturas diferentes.</p>
      ) : (
        <div className="properties revise">
          <fieldset>
            <legend>Colunas extrapoladas</legend>
            {list.length === 0 && <p className="hint">Nenhuma ainda.</p>}
            {list.map((spec, index) => (
              <div key={index} className="extrapolation">
                <span>{describe(spec)}</span>
                <button className="btn" onClick={() => setList(list.filter((_, position) => position !== index))}>
                  Remover
                </button>
              </div>
            ))}
          </fieldset>
          <fieldset>
            <legend>Nova</legend>
            <label>
              Alturas de destino (m)
              <input inputMode="decimal" value={heights} placeholder="ex.: 120; 140" onChange={(event) => setHeights(event.target.value)} />
            </label>
            <label>
              Processo
              <select value={tableOnly ? 'table' : 'step'} onChange={(event) => setTableOnly(event.target.value === 'table')}>
                <option value="step">Calcular em cada passo (tabela mês × hora de reserva)</option>
                <option value="table">Só a tabela mês × hora</option>
              </select>
            </label>
            <SensorChecks legend="Calcular a partir de" columns={speeds} chosen={sensors} onChange={setSensors} />
            <label>
              Extrapolar a partir de
              <select value={from} onChange={(event) => setFrom(event.target.value)}>
                <option value={NEAREST}>Sensor mais próximo da altura</option>
                <option value={BEST_FIT} disabled={tableOnly}>
                  Reta de melhor ajuste
                </option>
                {speeds.map((column) => (
                  <option key={column.id} value={`${SENSOR_PREFIX}${column.id}`}>
                    {column.id} · {formatNumber(column.heightM)} m
                  </option>
                ))}
              </select>
            </label>
            <label className="check" title="Expoentes do passo fora da faixa ficam no limite (o original propõe −0,25 a 1,25).">
              <input type="checkbox" checked={restrict} onChange={(event) => setRestrict(event.target.checked)} />
              Restringir α do passo
            </label>
            {restrict && (
              <div className="row">
                <label>
                  Mínimo
                  <input inputMode="decimal" value={alphaMin} onChange={(event) => setAlphaMin(event.target.value)} />
                </label>
                <label>
                  Máximo
                  <input inputMode="decimal" value={alphaMax} onChange={(event) => setAlphaMax(event.target.value)} />
                </label>
              </div>
            )}
            <label className="check">
              <input type="checkbox" checked={withDirection} disabled={!atTwoHeights(vanes.map((column) => column.id))} onChange={(event) => setWithDirection(event.target.checked)} />
              Sintetizar também a direção (veer)
            </label>
            {withDirection && <SensorChecks legend="Sensores de direção" columns={vanes} chosen={vaneIds} onChange={setVaneIds} />}
            <label className="check">
              <input
                type="checkbox"
                checked={withTemperature}
                disabled={!atTwoHeights(thermometers.map((column) => column.id))}
                onChange={(event) => setWithTemperature(event.target.checked)}
              />
              Sintetizar também a temperatura (gradiente)
            </label>
            {withTemperature && <SensorChecks legend="Termômetros" columns={thermometers} chosen={thermometerIds} onChange={setThermometerIds} />}
            <button className="btn" disabled={!addable} onClick={add}>
              Adicionar
            </button>
          </fieldset>
          <label className="check">
            <input type="checkbox" checked={flagNew} onChange={(event) => setFlagNew(event.target.checked)} />
            Marcar as novas colunas com a flag
            <select value={flag} disabled={!flagNew} onChange={(event) => setFlag(Number(event.target.value))}>
              {dataset.flags.map((known, index) => (
                <option key={index} value={index}>
                  {known.name}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}
      <p className="hint">
        Cada passo ajusta a lei de potência às velocidades escolhidas que leem 4 m/s ou mais (o DP, quando elas têm, a direção pelo veer e a temperatura
        pelo gradiente); sem ajuste no passo, vale a mediana do mês e da hora. As colunas são derivadas: herdam as flags dos sensores que leem, a
        velocidade ganha TI e densidade de potência, e tudo é recalculado a cada leitura do arquivo.
      </p>
    </Modal>
  );
}

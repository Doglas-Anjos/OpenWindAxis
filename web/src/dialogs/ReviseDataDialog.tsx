/**
 * The value edits of Revisar (ESPEC-INTERFACE §1, menu Revise of the
 * reference product): Escala e offset, Excluir dados and Deslocar tempo.
 * Each picks the dataset, the measured columns and the period, then its own
 * numbers; OK applies one undoable edit (Editar › Desfazer takes it back).
 * Derived columns are not offered: they follow the columns they come from.
 */
import { useState } from 'react';
import type { DatasetId } from '../api/generated/DatasetId';
import type { OpDto } from '../api/generated/OpDto';
import { formatNumber } from '../format';
import { formatDateTime, formatStep, isDerived, isMeanSpeed, lastStepMs, typedNumber, type Entry } from '../workbook/model';
import type { Workbook } from '../workbook/useWorkbook';
import { Modal } from './Modal';

/** Which edit the dialog makes. */
export type ReviseKind = 'scale' | 'delete' | 'shift' | 'reconstruct';

const TITLE: Record<ReviseKind, string> = {
  scale: 'Escala e offset',
  delete: 'Excluir dados',
  shift: 'Deslocar tempo',
  reconstruct: 'Reconstruir lacunas',
};
/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** Characters of an ISO date-time down to the seconds ("2023-01-01T00:10:00"). */
const ISO_TO_SECONDS = 19;
/** A `datetime-local` value as UTC epoch ms (the record's clock is shown as UTC). */
const utcMs = (value: string) => Date.parse(`${value}Z`);
/** Epoch ms as a `datetime-local` value, read in UTC. */
const localValue = (epochMs: number) => new Date(epochMs).toISOString().slice(0, ISO_TO_SECONDS);
/** Default numbers: an edit that changes nothing until the user types. */
const NEUTRAL_SCALE = '1';
const NEUTRAL_OFFSET = '0';
const DEFAULT_SHIFT_STEPS = '1';

interface Props {
  kind: ReviseKind;
  entries: Entry[];
  initialDataset: DatasetId;
  workbook: Workbook;
  onClose: () => void;
}

export function ReviseDataDialog({ kind, entries, initialDataset, workbook, onClose }: Props) {
  const [chosen, setChosen] = useState<DatasetId>(initialDataset);
  const entry = entries.find((candidate) => candidate.result.dataset.id === chosen) ?? entries[0];
  const dataset = entry.result.dataset;
  const measured = dataset.columns.filter((column) => !isDerived(column));
  const [columns, setColumns] = useState<string[]>([]);
  const [from, setFrom] = useState(() => localValue(dataset.startMs));
  const [to, setTo] = useState(() => localValue(lastStepMs(dataset)));
  const [scale, setScale] = useState(NEUTRAL_SCALE);
  const [offset, setOffset] = useState(NEUTRAL_OFFSET);
  const [wholeDataset, setWholeDataset] = useState(true);
  const [newStart, setNewStart] = useState(() => localValue(dataset.startMs));
  const [steps, setSteps] = useState(DEFAULT_SHIFT_STEPS);
  const [later, setLater] = useState(true);
  const speedsWithHeight = measured.filter((column) => isMeanSpeed(column) && column.heightM != null);
  const [fromHeight, setFromHeight] = useState(false);
  const [source, setSource] = useState(() => speedsWithHeight[0]?.id ?? '');

  const choose = (id: DatasetId) => {
    const next = entries.find((candidate) => candidate.result.dataset.id === id);
    if (!next) return;
    setChosen(id);
    setColumns([]);
    setFrom(localValue(next.result.dataset.startMs));
    setTo(localValue(lastStepMs(next.result.dataset)));
    setNewStart(localValue(next.result.dataset.startMs));
  };
  const toggle = (id: string) => setColumns(columns.includes(id) ? columns.filter((column) => column !== id) : [...columns, id]);

  const [fromMs, toMs] = [utcMs(from), utcMs(to)];
  const stretch = { columns, fromMs, toMs };
  const stretchValid = columns.length > 0 && fromMs <= toMs;
  /** The op to apply, or a reason it cannot be applied yet. */
  const op: OpDto | string = (() => {
    if (kind === 'shift' && wholeDataset) {
      const startMs = utcMs(newStart);
      return Number.isFinite(startMs) ? { op: 'shift_start', byMs: startMs - dataset.startMs } : 'Informe o novo início.';
    }
    if (!stretchValid) return columns.length ? 'O fim do período vem antes do início.' : 'Escolha ao menos uma coluna.';
    if (kind === 'delete') return { op: 'delete_data', ...stretch };
    if (kind === 'reconstruct') {
      if (!fromHeight) return { op: 'reconstruct_gaps', ...stretch, method: { kind: 'markov' } };
      if (!source || columns.includes(source)) return 'Escolha uma velocidade de origem diferente das colunas a reconstruir.';
      return { op: 'reconstruct_gaps', ...stretch, method: { kind: 'from_height', source } };
    }
    if (kind === 'scale') {
      const [scaleValue, offsetValue] = [typedNumber(scale), typedNumber(offset)];
      if (scaleValue === undefined || offsetValue === undefined || !Number.isFinite(scaleValue) || !Number.isFinite(offsetValue)) return 'Escala e offset precisam ser números.';
      return { op: 'scale_offset', ...stretch, scale: scaleValue, offset: offsetValue };
    }
    const count = Number(steps);
    if (!Number.isInteger(count) || count <= 0) return 'O deslocamento é um número inteiro de passos, maior que zero.';
    return { op: 'shift_steps', ...stretch, bySteps: later ? count : -count };
  })();
  const accept = () => {
    if (typeof op === 'string') return;
    onClose();
    void workbook.applyOp(entry, op);
  };

  const showStretch = kind !== 'shift' || !wholeDataset;
  return (
    <Modal
      title={TITLE[kind]}
      onClose={onClose}
      buttons={
        <>
          <button className="btn primary" disabled={typeof op === 'string'} title={typeof op === 'string' ? op : undefined} onClick={accept}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancelar
          </button>
        </>
      }
    >
      <label className="dataset-select">
        Dataset
        <select value={dataset.id} onChange={(event) => choose(Number(event.target.value))}>
          {entries.map((candidate) => (
            <option key={candidate.result.dataset.id} value={candidate.result.dataset.id}>
              {candidate.result.dataset.name}
            </option>
          ))}
        </select>
      </label>
      <div className="properties revise">
        {kind === 'shift' && (
          <fieldset>
            <legend>Deslocar</legend>
            <label className="check">
              <input type="radio" checked={wholeDataset} onChange={() => setWholeDataset(true)} />
              Todo o dataset (só os carimbos de tempo mudam)
            </label>
            <label className="check">
              <input type="radio" checked={!wholeDataset} onChange={() => setWholeDataset(false)} />
              Um trecho de algumas colunas
            </label>
            {wholeDataset && (
              <>
                <label>
                  Início atual
                  <input readOnly value={formatDateTime(dataset.startMs)} />
                </label>
                <label>
                  Novo início
                  <input type="datetime-local" step={1} value={newStart} onChange={(event) => setNewStart(event.target.value)} />
                </label>
              </>
            )}
          </fieldset>
        )}
        {showStretch && (
          <>
            <fieldset className="columns">
              <legend>Colunas</legend>
              <label className="check">
                <input type="checkbox" checked={columns.length === measured.length} onChange={(event) => setColumns(event.target.checked ? measured.map((column) => column.id) : [])} />
                Selecionar todas
              </label>
              {measured.map((column) => (
                <label key={column.id} className="check">
                  <input type="checkbox" checked={columns.includes(column.id)} onChange={() => toggle(column.id)} />
                  {column.id}
                </label>
              ))}
            </fieldset>
            <fieldset>
              <legend>Período</legend>
              <label>
                Início
                <input type="datetime-local" step={1} value={from} onChange={(event) => setFrom(event.target.value)} />
              </label>
              <label>
                Fim (passo incluído)
                <input type="datetime-local" step={1} value={to} onChange={(event) => setTo(event.target.value)} />
              </label>
              {kind === 'scale' && (
                <>
                  <label>
                    Escala
                    <input inputMode="decimal" value={scale} onChange={(event) => setScale(event.target.value)} />
                  </label>
                  <label>
                    Offset (unidade da coluna)
                    <input inputMode="decimal" value={offset} onChange={(event) => setOffset(event.target.value)} />
                  </label>
                </>
              )}
              {kind === 'reconstruct' && (
                <>
                  <label className="check">
                    <input type="radio" checked={!fromHeight} onChange={() => setFromHeight(false)} />
                    Markov (a própria coluna)
                  </label>
                  <label className="check">
                    <input type="radio" checked={fromHeight} disabled={speedsWithHeight.length < 2} onChange={() => setFromHeight(true)} />
                    De outra altura
                  </label>
                  {fromHeight && (
                    <label>
                      Velocidade de origem
                      <select value={source} onChange={(event) => setSource(event.target.value)}>
                        {speedsWithHeight.map((column) => (
                          <option key={column.id} value={column.id}>
                            {column.id} · {formatNumber(column.heightM)} m
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </>
              )}
              {kind === 'shift' && (
                <>
                  <label>
                    Passos ({formatStep(dataset.stepS)} cada)
                    <input type="number" min={1} step={1} value={steps} onChange={(event) => setSteps(event.target.value)} />
                  </label>
                  <label className="check">
                    <input type="radio" checked={later} onChange={() => setLater(true)} />
                    Para frente (mais tarde)
                  </label>
                  <label className="check">
                    <input type="radio" checked={!later} onChange={() => setLater(false)} />
                    Para trás (mais cedo)
                  </label>
                </>
              )}
            </fieldset>
          </>
        )}
      </div>
      <p className="hint">
        {kind === 'scale' &&
          'Valor novo = valor × escala + offset. Direções voltam para 0–360°; temperaturas em °C são escaladas em kelvin. DP, mínimo e máximo não acompanham a média: marque-os se quiser (num DP, use só a escala).'}
        {kind === 'delete' && 'Os valores do período viram ausentes. Para só tirá-los das análises, sem apagar, marque-os com uma flag.'}
        {kind === 'reconstruct' &&
          (fromHeight
            ? 'Preenche só os valores ausentes: a velocidade de origem × (altura da coluna ÷ altura de origem)^α, com α a mediana do mês e da hora entre as duas. Os passos preenchidos recebem a flag Reconstruído.'
            : 'Preenche só os valores ausentes com uma cadeia de Markov da própria coluna (faixas de 1 unidade), partindo do valor antes de cada lacuna; o sorteio tem semente fixa, então refazer dá o mesmo. Os passos preenchidos recebem a flag Reconstruído; dados com flag ficam como estão.')}
        {kind === 'shift' &&
          (wholeDataset
            ? 'Corrige o relógio do logger (horário de verão, fuso): os dados não mudam, só os carimbos de tempo.'
            : 'Os valores do trecho andam o número de passos escolhido e cobrem o que houver lá; o trecho deixado para trás fica ausente. As flags ficam onde estão.')}{' '}
        As colunas derivadas são recalculadas, e Editar › Desfazer volta atrás.
      </p>
    </Modal>
  );
}

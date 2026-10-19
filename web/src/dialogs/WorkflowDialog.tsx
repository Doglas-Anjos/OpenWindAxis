/**
 * Fluxos (ESPEC-INTERFACE §1, menu Fluxos; ARQUITETURA C2): a workflow is a
 * saved list of edits and analyses, run on any datasets with the same
 * column ids, here or with `wind run`. It can be built from a dataset (its
 * flag rules, its value edits, and analyses of its primary columns), saved
 * as JSON, opened again and run; each dataset's edits are undoable as usual.
 */
import { useRef, useState } from 'react';
import type { WindApi } from '../api/client';
import type { DatasetId } from '../api/generated/DatasetId';
import type { EditDto } from '../api/generated/EditDto';
import type { OpDto } from '../api/generated/OpDto';
import { WORKFLOW_FORMAT } from '../api/generated/version';
import type { WorkflowAnalysisDto } from '../api/generated/WorkflowAnalysisDto';
import type { WorkflowDto } from '../api/generated/WorkflowDto';
import type { WorkflowOutcomeDto } from '../api/generated/WorkflowOutcomeDto';
import { errorMessage, formatCount } from '../format';
import { primaryColumns, type Entry } from '../workbook/model';
import { download, textFile, type Workbook } from '../workbook/useWorkbook';
import { Modal } from './Modal';

/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** Indentation of a saved workflow, spaces. */
const JSON_INDENT = 2;
/** File name suffix of a saved workflow. */
const WORKFLOW_SUFFIX = '.fluxo.json';
const NOT_A_WORKFLOW = 'O arquivo não é um fluxo.';

type Primary = ReturnType<typeof primaryColumns>;
/** Analyses a workflow can be built with, from the dataset's primary columns. */
const ANALYSIS_TEMPLATES: { key: string; label: string; build: (primary: Primary) => WorkflowAnalysisDto | null }[] = [
  { key: 'coverage', label: 'Cobertura de dados', build: () => ({ analysis: 'coverage', request: {} }) },
  {
    key: 'histogram',
    label: 'Histograma e Weibull da velocidade',
    build: ({ speed }) => (speed ? { analysis: 'histogram', request: { column: speed.id, binWidth: 1, weibull: 'max_likelihood' } } : null),
  },
  {
    key: 'rose',
    label: 'Rosa dos ventos',
    build: ({ speed, direction }) => (direction ? { analysis: 'rose', request: { direction: direction.id, speed: speed?.id, sectors: 16 } } : null),
  },
  { key: 'diurnal', label: 'Perfil diurno', build: ({ speed }) => (speed ? { analysis: 'diurnal', request: { column: speed.id } } : null) },
  { key: 'table', label: 'Tabela de médias mensais', build: ({ speed }) => (speed ? { analysis: 'table', request: { column: speed.id } } : null) },
  {
    key: 'turbulence',
    label: 'Turbulência (categoria IEC)',
    build: ({ speed }) => (speed ? { analysis: 'turbulence', request: { speeds: [speed.id], versus: { kind: 'speed' } } } : null),
  },
  {
    key: 'extremes',
    label: 'Vento de 50 anos (máximos mensais)',
    build: ({ speed }) => (speed ? { analysis: 'extremes', request: { column: speed.id, method: { kind: 'periodic_maxima', period: 'month' } } } : null),
  },
];

/** A dataset's value edits (steps) as ops (instants), on the clock each had
 *  when it was made: a time shift moves the clock of the edits after it. */
function opsOfEdits(edits: EditDto[], startMs: number, stepS: number): OpDto[] {
  const stepMs = stepS * MS_PER_SECOND;
  const shiftedMs = edits.reduce((total, edit) => (edit.edit === 'shift_start' ? total + edit.bySeconds * MS_PER_SECOND : total), 0);
  let clockMs = startMs - shiftedMs;
  const span = (startStep: number, endStep: number) => ({ fromMs: clockMs + startStep * stepMs, toMs: clockMs + (endStep - 1) * stepMs });
  return edits.map((edit): OpDto => {
    switch (edit.edit) {
      case 'scale_offset':
        return { op: 'scale_offset', columns: edit.columns, ...span(edit.startStep, edit.endStep), scale: edit.scale, offset: edit.offset };
      case 'delete':
        return { op: 'delete_data', columns: edit.columns, ...span(edit.startStep, edit.endStep) };
      case 'shift_steps':
        return { op: 'shift_steps', columns: edit.columns, ...span(edit.startStep, edit.endStep), bySteps: edit.bySteps };
      case 'reconstruct':
        return { op: 'reconstruct_gaps', columns: edit.columns, ...span(edit.startStep, edit.endStep), method: edit.method };
      case 'shift_start':
        clockMs += edit.bySeconds * MS_PER_SECOND;
        return { op: 'shift_start', byMs: edit.bySeconds * MS_PER_SECOND };
    }
  });
}

/** Whether parsed JSON has a workflow's shape (the core checks the rest). */
const isWorkflow = (value: unknown): value is WorkflowDto =>
  typeof value === 'object' && value !== null && 'format' in value && Array.isArray((value as WorkflowDto).ops) && Array.isArray((value as WorkflowDto).analyses);

interface Props {
  api: WindApi;
  entries: Entry[];
  initialDataset: DatasetId;
  workbook: Workbook;
  onClose: () => void;
}

export function WorkflowDialog({ api, entries, initialDataset, workbook, onClose }: Props) {
  const source = entries.find((entry) => entry.result.dataset.id === initialDataset) ?? entries[0];
  const [name, setName] = useState('Fluxo');
  const [includeEdits, setIncludeEdits] = useState(true);
  const [includeRules, setIncludeRules] = useState(source.rules.length > 0);
  const [chosen, setChosen] = useState<Set<string>>(() => new Set(['histogram', 'table']));
  const [opened, setOpened] = useState<WorkflowDto | null>(null);
  // The other datasets: on its own source the edits would apply twice.
  const [targets, setTargets] = useState<Set<DatasetId>>(() => new Set(entries.filter((entry) => entry !== source).map((entry) => entry.result.dataset.id)));
  const [outcomes, setOutcomes] = useState<WorkflowOutcomeDto[] | null>(null);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const toggle = <T,>(set: Set<T>, value: T) => {
    const next = new Set(set);
    if (!next.delete(value)) next.add(value);
    return next;
  };

  /** The workflow opened from a file, or the one the form builds from the source dataset. */
  const workflow = async (): Promise<WorkflowDto> => {
    if (opened) return opened;
    const { dataset } = source.result;
    const ops: OpDto[] = [];
    if (includeEdits) {
      const restore = await api.projectState({ dataset: dataset.id });
      ops.push(...opsOfEdits(restore.edits ?? [], dataset.startMs, dataset.stepS));
    }
    if (includeRules && source.rules.length) ops.push({ op: 'flag_by_rules', rules: source.rules });
    const primary = primaryColumns(dataset);
    const analyses = ANALYSIS_TEMPLATES.filter((template) => chosen.has(template.key)).flatMap((template) => template.build(primary) ?? []);
    return { format: WORKFLOW_FORMAT, name, ops, analyses };
  };
  const save = async () => {
    try {
      const built = await workflow();
      download(`${built.name ?? 'fluxo'}${WORKFLOW_SUFFIX}`, textFile(JSON.stringify(built, null, JSON_INDENT), 'application/json'));
    } catch (thrown) {
      setError(errorMessage(thrown));
    }
  };
  const open = async (file: File | undefined) => {
    if (!file) return;
    try {
      const parsed: unknown = JSON.parse(await file.text());
      if (!isWorkflow(parsed)) throw new Error(NOT_A_WORKFLOW);
      setOpened(parsed);
      setError('');
    } catch (thrown) {
      setError(`${file.name}: ${errorMessage(thrown)}`);
    }
  };
  const run = async () => {
    setRunning(true);
    try {
      const results = await api.runWorkflow({ datasets: [...targets], workflow: await workflow() });
      for (const outcome of results) {
        const entry = entries.find((candidate) => candidate.result.dataset.id === outcome.dataset);
        if (entry) await workbook.showEdit(entry, Promise.resolve(outcome.state));
      }
      setOutcomes(results);
      setError('');
    } catch (thrown) {
      setError(errorMessage(thrown));
    }
    setRunning(false);
  };

  return (
    <Modal
      title="Fluxos"
      onClose={onClose}
      wide
      buttons={
        <>
          <button className="btn primary" disabled={running || targets.size === 0} onClick={run}>
            {running ? 'Executando…' : 'Executar nos datasets marcados'}
          </button>
          <button className="btn" onClick={onClose}>
            Fechar
          </button>
        </>
      }
    >
      <input ref={fileInput} type="file" hidden accept=".json,application/json" onChange={(event) => (open(event.target.files?.[0]), (event.target.value = ''))} />
      <div className="properties revise">
        <fieldset>
          <legend>Fluxo</legend>
          {opened ? (
            <>
              <p>
                <b>{opened.name ?? 'Sem nome'}</b>: {formatCount(opened.ops.length)} edições e {formatCount(opened.analyses.length)} análises, aberto de arquivo.
              </p>
              <button className="btn" onClick={() => setOpened(null)}>
                Montar outro a partir do dataset
              </button>
            </>
          ) : (
            <>
              <label>
                Nome
                <input value={name} onChange={(event) => setName(event.target.value)} />
              </label>
              <p className="hint">A partir de {source.result.dataset.name}:</p>
              <label className="check">
                <input type="checkbox" checked={includeEdits} onChange={(event) => setIncludeEdits(event.target.checked)} />
                As edições de valor do dataset (escala, exclusões, deslocamentos, reconstruções)
              </label>
              <label className="check">
                <input type="checkbox" checked={includeRules} disabled={!source.rules.length} onChange={(event) => setIncludeRules(event.target.checked)} />
                As {formatCount(source.rules.length)} regras de flag do dataset
              </label>
              {ANALYSIS_TEMPLATES.map((template) => (
                <label key={template.key} className="check">
                  <input type="checkbox" checked={chosen.has(template.key)} onChange={() => setChosen(toggle(chosen, template.key))} />
                  {template.label}
                </label>
              ))}
            </>
          )}
          <div className="actions">
            <button className="btn" onClick={save}>
              Salvar fluxo…
            </button>
            <button className="btn" onClick={() => fileInput.current?.click()}>
              Abrir fluxo…
            </button>
          </div>
        </fieldset>
        <fieldset className="columns">
          <legend>Executar em</legend>
          {entries.map((entry) => (
            <label key={entry.result.dataset.id} className="check">
              <input type="checkbox" checked={targets.has(entry.result.dataset.id)} onChange={() => setTargets(toggle(targets, entry.result.dataset.id))} />
              {entry.result.dataset.name}
            </label>
          ))}
        </fieldset>
      </div>
      {error && <div className="note err">{error}</div>}
      {outcomes && (
        <div className="tablewrap">
          <table className="grid workflow-outcomes">
            <thead>
              <tr>
                <th>Dataset</th>
                <th>Edições</th>
                <th>Análises</th>
              </tr>
            </thead>
            <tbody>
              {outcomes.map((outcome) => (
                <tr key={outcome.dataset}>
                  <td>{outcome.name}</td>
                  <td>{outcome.opError ? `${formatCount(outcome.opsApplied)} aplicadas; parou em: ${outcome.opError.message}` : `${formatCount(outcome.opsApplied)} aplicadas`}</td>
                  <td>{outcome.results.map((result) => `${result.analysis}: ${result.error ? result.error.message : 'ok'}`).join(' · ') || '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <button className="btn" onClick={() => download(`${name}_resultados.json`, textFile(JSON.stringify(outcomes, null, JSON_INDENT), 'application/json'))}>
            Baixar resultados (JSON)
          </button>
        </div>
      )}
      <p className="hint">
        Um fluxo guarda as edições por id de coluna e por instante, e as análises por nome: vale para qualquer dataset com as mesmas colunas, aqui ou na linha de
        comando (wind run). As edições aplicadas se desfazem com Editar › Desfazer.
      </p>
    </Modal>
  );
}

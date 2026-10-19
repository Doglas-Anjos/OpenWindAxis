/**
 * Flag rules (ESPEC-INTERFACE §5.4–5.5, ROTEIRO item 8): define rules for the
 * selected dataset, run them as one undoable edit, and review what each rule
 * found, with the evidence of every stretch (ARQUITETURA B4). The semantics
 * of each test are in crates/wind-core/src/rules.rs.
 */
import { useState } from 'react';
import { ApiError, type WindApi } from './api/client';
import type { CriterionDto } from './api/generated/CriterionDto';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { EditStateDto } from './api/generated/EditStateDto';
import type { EvidenceDto } from './api/generated/EvidenceDto';
import type { FlagByRulesDto } from './api/generated/FlagByRulesDto';
import type { FlagRuleDto } from './api/generated/FlagRuleDto';
import { errorMessage, formatCount, formatDateTime, formatNumber, formatPercent, NO_VALUE } from './format';

type CriterionKind = CriterionDto['kind'];

/** pt-BR names of the tests. */
export const CRITERION_LABEL: Record<CriterionKind, string> = {
  outside_range: 'Fora da faixa',
  change_above: 'Variação acima do limite',
  stuck: 'Sensor travado',
  twin_drift: 'Deriva entre anemômetros gêmeos',
  clock_shift: 'Relógio deslocado',
  direction_in_sector: 'Direção no setor',
};

/** Shortest duration each test accepts, in steps (the core refuses less). */
const LEAST_MIN_STEPS: Record<CriterionKind, number> = { outside_range: 1, change_above: 1, stuck: 2, twin_drift: 1, clock_shift: 1, direction_in_sector: 1 };
/** Duration a new rule starts with: one step, or an hour of 10-minute steps for a flat. */
const DEFAULT_MIN_STEPS: Record<CriterionKind, number> = { outside_range: 1, change_above: 1, stuck: 6, twin_drift: 1, clock_shift: 1, direction_in_sector: 1 };
/** Percent in a share. */
const PERCENT = 100;
/** Largest clock shift the core measures, hours (half a day). */
const MAX_SHIFT_HOURS = 12;
/** A sector is at most a whole turn, degrees. */
const DEGREES_PER_TURN = 360;
/** Evidence rows listed after a run; the rest are only counted. */
const MAX_LISTED_EVIDENCE = 200;
const MS_PER_SECOND = 1000;
/** Shown for an open side of a range. */
const NO_LOWER_BOUND = '−∞';
const NO_UPPER_BOUND = '∞';

/** pt-BR text for the core's errors about rules; other codes show the core's message. */
const RULE_ERROR_TEXT: Record<string, string> = {
  rule_range_invalid: 'A faixa precisa de um mínimo menor que o máximo.',
  rule_threshold_invalid: 'Limite e tolerância precisam ser números não negativos; a deriva, maior que zero; o deslocamento do relógio, de 1 a 12 h; a largura do setor, de 0 a 360°.',
  rule_min_steps_too_small: 'A duração mínima é curta demais para esta regra (sensor travado precisa de pelo menos 2 passos).',
  too_many_rules: 'Regras demais numa só execução; execute-as em grupos menores.',
  too_many_rule_hits: 'As regras encontraram trechos demais; ajuste os limites ou aumente a duração mínima.',
  unknown_column: 'Uma regra cita uma coluna que o dataset não tem mais.',
  no_such_flag: 'Uma regra cita uma flag que o dataset não tem.',
};

const ruleErrorText = (thrown: unknown) =>
  (thrown instanceof ApiError && RULE_ERROR_TEXT[thrown.code]) || errorMessage(thrown);

/** The rule form, with numbers as typed. */
interface RuleDraft {
  kind: CriterionKind;
  min: string;
  max: string;
  limit: string;
  tolerance: string;
  /** Twin drift: the reference column and the departure allowed, percent. */
  reference: string;
  drift: string;
  /** Clock shift: the smallest shift, hours. */
  minHours: string;
  /** Direction in sector: the direction column, the sector's centre and width, degrees. */
  direction: string;
  center: string;
  width: string;
  minSteps: string;
  flag: number;
  columns: string[];
}

const EMPTY_DRAFT: RuleDraft = {
  kind: 'outside_range',
  min: '',
  max: '',
  limit: '',
  tolerance: '0',
  reference: '',
  drift: '3',
  minHours: '1',
  direction: '',
  center: '',
  width: '30',
  minSteps: '1',
  flag: 0,
  columns: [],
};

/** A number typed in a field; `null` when empty or not a number. */
const typed = (text: string) => (text.trim() === '' || !Number.isFinite(Number(text)) ? null : Number(text));

/** The rule a draft describes, or `null` while it is incomplete. */
function ruleOf(draft: RuleDraft): FlagRuleDto | null {
  const minSteps = typed(draft.minSteps);
  if (minSteps == null || !Number.isInteger(minSteps) || minSteps < LEAST_MIN_STEPS[draft.kind] || !draft.columns.length) return null;
  let criterion: CriterionDto;
  if (draft.kind === 'outside_range') {
    const [min, max] = [typed(draft.min), typed(draft.max)];
    if (min == null && max == null) return null;
    criterion = { kind: 'outside_range', min, max };
  } else if (draft.kind === 'change_above') {
    const limit = typed(draft.limit);
    if (limit == null) return null;
    criterion = { kind: 'change_above', limit };
  } else if (draft.kind === 'stuck') {
    const tolerance = typed(draft.tolerance);
    if (tolerance == null) return null;
    criterion = { kind: 'stuck', tolerance };
  } else if (draft.kind === 'twin_drift') {
    const drift = typed(draft.drift);
    // A column compared with itself never drifts.
    if (drift == null || drift <= 0 || !draft.reference || draft.columns.includes(draft.reference)) return null;
    criterion = { kind: 'twin_drift', reference: draft.reference, tolerance: drift / PERCENT };
  } else if (draft.kind === 'direction_in_sector') {
    const [center, width] = [typed(draft.center), typed(draft.width)];
    if (center == null || width == null || width <= 0 || width > DEGREES_PER_TURN || !draft.direction) return null;
    criterion = { kind: 'direction_in_sector', direction: draft.direction, center, width };
  } else {
    const minHours = typed(draft.minHours);
    if (minHours == null || !Number.isInteger(minHours) || minHours < 1 || minHours > MAX_SHIFT_HOURS) return null;
    criterion = { kind: 'clock_shift', minHours };
  }
  return { criterion, columns: draft.columns, flag: draft.flag, minSteps };
}

/** The test and its thresholds, in pt-BR. */
export function describeCriterion(criterion: CriterionDto): string {
  switch (criterion.kind) {
    case 'outside_range':
      return `fora de [${criterion.min == null ? NO_LOWER_BOUND : formatNumber(criterion.min)}; ${
        criterion.max == null ? NO_UPPER_BOUND : formatNumber(criterion.max)
      })`;
    case 'change_above':
      return `variação > ${formatNumber(criterion.limit)} por passo`;
    case 'stuck':
      return `travado: variação ≤ ${formatNumber(criterion.tolerance)} por passo`;
    case 'twin_drift':
      return `razão diária para ${criterion.reference} afastada mais de ${formatPercent(criterion.tolerance)} da do registro`;
    case 'clock_shift':
      return `ciclo diurno semanal deslocado ${formatNumber(criterion.minHours)} h ou mais`;
    case 'direction_in_sector':
      return `${criterion.direction} no setor de ${formatNumber(criterion.width)}° centrado em ${formatNumber(criterion.center)}°`;
  }
}

/** What made a stretch fire, in pt-BR. */
export function describeStatistic(evidence: EvidenceDto, units: string): string {
  const { statistic } = evidence;
  if (statistic == null) return NO_VALUE;
  const withUnits = (value: string) => (units ? `${value} ${units}` : value);
  switch (evidence.criterion.kind) {
    case 'outside_range':
      return `valor ${withUnits(formatNumber(statistic))}`;
    case 'change_above':
      return `variação de ${withUnits(`${statistic > 0 ? '+' : ''}${formatNumber(statistic)}`)}`;
    case 'stuck':
      return `${formatCount(statistic)} passos sem variar`;
    case 'twin_drift':
      return `razão ${statistic > 0 ? '+' : ''}${formatPercent(statistic)} em relação à do registro`;
    case 'clock_shift':
      return `relógio ${statistic > 0 ? 'adiantado' : 'atrasado'} ${formatNumber(Math.abs(statistic))} h`;
    case 'direction_in_sector':
      return `direção ${formatNumber(statistic)}°`;
  }
}

/** A flag's colour swatch and name. */
export function FlagName({ dataset, flag }: { dataset: DatasetDto; flag: number }) {
  const known = dataset.flags[flag];
  return (
    <span className="flagname">
      <span className="swatch" style={{ background: known?.color }} />
      {known?.name ?? `#${flag}`}
    </span>
  );
}

/** Time of the start of step `step`, as text. */
export const stepTime = (dataset: DatasetDto, step: number) => formatDateTime(dataset.startMs + step * dataset.stepS * MS_PER_SECOND);

/** Evidence rows: column, flag, first and last flagged step, length, test and what fired. */
export function EvidenceRows({ dataset, evidence }: { dataset: DatasetDto; evidence: EvidenceDto[] }) {
  const units = (columnId: string) => dataset.columns.find((column) => column.id === columnId)?.units ?? '';
  return (
    <>
      {evidence.map((record, row) => (
        <tr key={row}>
          <td className="id">{record.column}</td>
          <td>
            <FlagName dataset={dataset} flag={record.flag} />
          </td>
          <td>{stepTime(dataset, record.startStep)}</td>
          <td>{stepTime(dataset, record.endStep - 1)}</td>
          <td className="r">{formatCount(record.endStep - record.startStep)}</td>
          <td title={`duração mínima: ${record.minSteps} passo(s)`}>
            {CRITERION_LABEL[record.criterion.kind]}: {describeCriterion(record.criterion)}
          </td>
          <td title={`medido em ${stepTime(dataset, record.statisticStep)}`}>{describeStatistic(record, units(record.column))}</td>
        </tr>
      ))}
    </>
  );
}

/** Header of an {@link EvidenceRows} table. */
export const EVIDENCE_HEADERS = ['Coluna', 'Flag', 'Início', 'Último passo', 'Passos', 'Regra', 'Evidência'];

interface Props {
  api: WindApi;
  dataset: DatasetDto;
  /** The dataset's rules; kept by the page and saved with the project. */
  rules: FlagRuleDto[];
  onRulesChange: (rules: FlagRuleDto[]) => void;
  /** Shows the flags and summaries a run left, like any other edit. */
  onEdit: (edited: Promise<EditStateDto>) => void;
  disabled: boolean;
}

/** The "Flag por regras" card of the selected dataset. */
export function FlagRulesCard({ api, dataset, rules, onRulesChange, onEdit, disabled }: Props) {
  const [draft, setDraft] = useState<RuleDraft>(EMPTY_DRAFT);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState('');
  /** The last run's answer, with the list position of each rule it sent; `tested` when nothing was flagged. */
  const [lastRun, setLastRun] = useState<{ answer: FlagByRulesDto; positions: number[]; tested: boolean } | null>(null);
  /** Derived columns inherit their sources' flags, so rules search measured columns only. */
  const measured = dataset.columns.filter((column) => !column.derivedFrom);
  const columnIds = new Set(measured.map((column) => column.id));
  const applicable = (rule: FlagRuleDto) => rule.columns.every((id) => columnIds.has(id)) && rule.flag < dataset.flags.length;
  const runnable = rules.flatMap((rule, position) => (applicable(rule) ? [position] : []));
  const drafted = ruleOf(draft);
  const blocked = disabled || running;

  /** Runs the rules at `positions`, or only tests them: what they would flag, nothing flagged. */
  async function run(positions: number[], tested = false) {
    setRunning(true);
    setError('');
    try {
      const request = { dataset: dataset.id, rules: positions.map((position) => rules[position]) };
      const answer = tested ? await api.testRules(request) : await api.flagByRules(request);
      if (!tested) onEdit(Promise.resolve(answer.state));
      setLastRun({ answer, positions, tested });
    } catch (thrown) {
      setError(ruleErrorText(thrown));
    }
    setRunning(false);
  }

  const setKind = (kind: CriterionKind) => setDraft({ ...draft, kind, minSteps: String(DEFAULT_MIN_STEPS[kind]) });
  const toggleColumn = (id: string) =>
    setDraft({
      ...draft,
      columns: draft.columns.includes(id)
        ? draft.columns.filter((chosen) => chosen !== id)
        : measured.map((column) => column.id).filter((candidate) => candidate === id || draft.columns.includes(candidate)),
    });
  const numberField = (label: string, field: 'min' | 'max' | 'limit' | 'tolerance' | 'drift' | 'minHours' | 'center' | 'width', title: string) => (
    <label title={title}>
      {label}
      <input type="number" step="any" value={draft[field]} onChange={(event) => setDraft({ ...draft, [field]: event.target.value })} />
    </label>
  );

  return (
    <section className="card">
      <div className="cardhead">
        <h3>Flag por regras</h3>
        <button
          className="btn"
          disabled={blocked || !runnable.length}
          onClick={() => run(runnable, true)}
          title="Mostra o que as regras encontrariam, sem marcar nada (recomendado antes de executar)."
        >
          Testar todas
        </button>
        <button
          className="btn primary"
          disabled={blocked || !runnable.length}
          onClick={() => run(runnable)}
          title="Aplica as regras ao registro inteiro e informa quantos passos foram afetados."
        >
          {running ? 'Executando…' : 'Executar todas'}
        </button>
      </div>
      <p className="hint">
        Cada trecho marcado por uma regra guarda a evidência: o teste, o limite e o valor que disparou. Dados já excluídos
        por outra flag não disparam regras. Uma execução se desfaz como qualquer marcação (Desfazer, Ctrl+Z).
      </p>

      {rules.length > 0 && (
        <div className="tablewrap">
          <table>
            <thead>
              <tr>
                <th>#</th>
                <th>Teste</th>
                <th>Colunas</th>
                <th>Flag</th>
                <th className="r">Duração mínima</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rules.map((rule, position) => (
                <tr key={position}>
                  <td>{position + 1}</td>
                  <td>
                    {CRITERION_LABEL[rule.criterion.kind]}: {describeCriterion(rule.criterion)}
                  </td>
                  <td>
                    {rule.columns.join(', ')}
                    {!applicable(rule) && (
                      <span className="warn" title="A regra cita uma coluna ou flag que este dataset não tem e fica fora da execução.">
                        {' '}
                        ⚠ não se aplica
                      </span>
                    )}
                  </td>
                  <td>
                    <FlagName dataset={dataset} flag={rule.flag} />
                  </td>
                  <td className="r">{formatCount(rule.minSteps)} passo(s)</td>
                  <td className="r">
                    <button className="btn small" disabled={blocked || !applicable(rule)} onClick={() => run([position], true)}>
                      Testar
                    </button>
                    <button className="btn small" disabled={blocked || !applicable(rule)} onClick={() => run([position])}>
                      Executar
                    </button>
                    <button
                      className="x"
                      title="Excluir a regra (as flags já aplicadas ficam)"
                      aria-label={`Excluir a regra ${position + 1}`}
                      disabled={blocked}
                      onClick={() => onRulesChange(rules.filter((_, other) => other !== position))}
                    >
                      ×
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div className="controls ruleform">
        <label title="Condição de teste: valor fora de uma faixa, variação entre passos acima de um limite ou sensor parado.">
          Teste
          <select value={draft.kind} onChange={(event) => setKind(event.target.value as CriterionKind)}>
            {Object.entries(CRITERION_LABEL).map(([kind, label]) => (
              <option key={kind} value={kind}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {draft.kind === 'outside_range' && (
          <>
            {numberField('Mínimo', 'min', 'Valores abaixo disto disparam a regra; vazio = sem limite inferior.')}
            {numberField('Máximo', 'max', 'Valores iguais ou acima disto disparam a regra; vazio = sem limite superior.')}
          </>
        )}
        {draft.kind === 'change_above' &&
          numberField('Limite por passo', 'limit', 'Dispara no passo cuja diferença para o anterior passa deste valor.')}
        {draft.kind === 'stuck' &&
          numberField('Tolerância por passo', 'tolerance', 'Diferenças entre passos até este valor contam como valor parado.')}
        {draft.kind === 'twin_drift' && (
          <>
            <label title="O anemômetro gêmeo, na mesma altura, com que cada coluna marcada é comparada dia a dia.">
              Anemômetro de referência
              <select value={draft.reference} onChange={(event) => setDraft({ ...draft, reference: event.target.value })}>
                <option value="">—</option>
                {measured.map((column) => (
                  <option key={column.id} value={column.id}>
                    {column.id}
                  </option>
                ))}
              </select>
            </label>
            {numberField(
              'Tolerância (%)',
              'drift',
              'Dispara no dia em que a razão média para a referência se afasta mais que isto da razão do registro inteiro (só com a referência a partir de 4 m/s).',
            )}
          </>
        )}
        {draft.kind === 'clock_shift' &&
          numberField(
            'Deslocamento mínimo (h)',
            'minHours',
            'Dispara na semana cujo ciclo diurno está deslocado do ciclo do registro por pelo menos tantas horas. Use temperatura ou radiação solar.',
          )}
        {draft.kind === 'direction_in_sector' && (
          <>
            <label title="A coluna de direção que decide se o vento cruza a torre antes de chegar ao anemômetro.">
              Direção
              <select value={draft.direction} onChange={(event) => setDraft({ ...draft, direction: event.target.value })}>
                <option value="">—</option>
                {measured
                  .filter((column) => column.kind === 'wind_direction')
                  .map((column) => (
                    <option key={column.id} value={column.id}>
                      {column.id}
                    </option>
                  ))}
              </select>
            </label>
            {numberField('Centro (°)', 'center', 'Centro do setor; o setor vai de centro − largura/2 até antes de centro + largura/2, passando pelo norte se preciso.')}
            {numberField('Largura (°)', 'width', 'Largura do setor, até 360°.')}
          </>
        )}
        <label title="O critério precisa valer por pelo menos este número de passos seguidos para gerar flag.">
          Duração mínima (passos)
          <input
            type="number"
            min={LEAST_MIN_STEPS[draft.kind]}
            step="1"
            value={draft.minSteps}
            onChange={(event) => setDraft({ ...draft, minSteps: event.target.value })}
          />
        </label>
        <label title="Flag aplicada aos trechos encontrados.">
          Flag
          <select value={draft.flag} onChange={(event) => setDraft({ ...draft, flag: Number(event.target.value) })}>
            {dataset.flags.map((flag, index) => (
              <option key={index} value={index}>
                {flag.name}
              </option>
            ))}
          </select>
        </label>
        <fieldset className="checks" title="Colunas pesquisadas; cada uma recebe a flag onde a regra dispara (DP, mín., máx. e rajada seguem a média).">
          <legend>Colunas</legend>
          {measured.map((column) => (
            <label key={column.id} className="check">
              <input type="checkbox" checked={draft.columns.includes(column.id)} onChange={() => toggleColumn(column.id)} />
              {column.id}
            </label>
          ))}
        </fieldset>
        <button className="btn" disabled={!drafted} onClick={() => drafted && onRulesChange([...rules, drafted])}>
          Adicionar regra
        </button>
      </div>

      {error && <div className="note err">{error}</div>}
      {lastRun && <RunSummary dataset={dataset} answer={lastRun.answer} positions={lastRun.positions} tested={lastRun.tested} />}
    </section>
  );
}

/** What the last run found: per rule and column, then every stretch with its evidence. */
function RunSummary({ dataset, answer, positions, tested }: { dataset: DatasetDto; answer: FlagByRulesDto; positions: number[]; tested: boolean }) {
  const listed = answer.found.slice(0, MAX_LISTED_EVIDENCE);
  return (
    <div className="runsummary">
      <h4>{tested ? 'Teste: o que as regras marcariam (nada foi marcado)' : 'Última execução'}</h4>
      <div className="tablewrap">
        <table>
          <thead>
            <tr>
              <th>Regra</th>
              <th>Coluna</th>
              <th className="r">Trechos</th>
              <th className="r" title="Passos nos trechos encontrados, inclusive os que já tinham esta flag">
                Passos
              </th>
            </tr>
          </thead>
          <tbody>
            {answer.tallies.map((tally) => (
              <tr key={`${tally.rule}-${tally.column}`}>
                <td>{positions[tally.rule] + 1}</td>
                <td className="id">{tally.column}</td>
                <td className="r">{formatCount(tally.stretches)}</td>
                <td className="r">{formatCount(tally.steps)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {answer.found.length > 0 && (
        <details>
          <summary>Evidências ({formatCount(answer.found.length)} trechos)</summary>
          <div className="tablewrap">
            <table>
              <thead>
                <tr>
                  {EVIDENCE_HEADERS.map((header) => (
                    <th key={header}>{header}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                <EvidenceRows dataset={dataset} evidence={listed} />
              </tbody>
            </table>
          </div>
          {answer.found.length > listed.length && (
            <p className="hint">
              Mostrando {formatCount(listed.length)} de {formatCount(answer.found.length)}; todos os trechos aparecem
              em Estatística de flags.
            </p>
          )}
        </details>
      )}
    </div>
  );
}

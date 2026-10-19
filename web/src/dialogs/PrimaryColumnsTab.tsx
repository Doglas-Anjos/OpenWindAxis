/**
 * Configurar dataset › Colunas primárias (ESPEC-INTERFACE §2.3): for each type
 * the dataset measures, which column represents it, by id or by a criterion
 * on height (nearest-in-height rule, decided in the core). The choice lives in
 * the dataset's configuration: OK re-reads the file with it.
 */
import type { ColumnKind } from '../api/generated/ColumnKind';
import type { DatasetDto } from '../api/generated/DatasetDto';
import type { PrimaryChoiceDto } from '../api/generated/PrimaryChoiceDto';
import type { PrimaryColumnDto } from '../api/generated/PrimaryColumnDto';
import { COLUMN_KIND_LABEL } from '../labels';
import { isDerived, typedNumber } from '../workbook/model';

/** Select value of "no choice": the core's default. */
const DEFAULT = 'default';
/** Prefix of a select value naming a column. */
const COLUMN_PREFIX = 'column:';
const CRITERION_LABEL: Record<Exclude<PrimaryChoiceDto['by'], 'column'>, string> = {
  highest: 'A mais alta',
  lowest: 'A mais baixa',
  nearest: 'A mais próxima da altura',
};

const valueOf = (choice: PrimaryChoiceDto | undefined) =>
  choice === undefined ? DEFAULT : choice.by === 'column' ? `${COLUMN_PREFIX}${choice.column}` : choice.by;

interface Props {
  dataset: DatasetDto;
  choices: PrimaryColumnDto[];
  onChange: (choices: PrimaryColumnDto[]) => void;
}

export function PrimaryColumnsTab({ dataset, choices, onChange }: Props) {
  const measured = dataset.columns.filter((column) => column.subtype === 'mean' && !isDerived(column));
  const kinds = [...new Set(measured.map((column) => column.kind))];
  const choose = (kind: ColumnKind, choice: PrimaryChoiceDto | undefined) =>
    onChange([...choices.filter((candidate) => candidate.kind !== kind), ...(choice ? [{ kind, choice }] : [])]);
  const pick = (kind: ColumnKind, value: string, current: PrimaryChoiceDto | undefined) => {
    if (value === DEFAULT) return choose(kind, undefined);
    if (value.startsWith(COLUMN_PREFIX)) return choose(kind, { by: 'column', column: value.slice(COLUMN_PREFIX.length) });
    if (value === 'nearest') return choose(kind, { by: 'nearest', heightM: current?.by === 'nearest' ? current.heightM : (measured.find((column) => column.kind === kind)?.heightM ?? 0) });
    choose(kind, { by: value as 'highest' | 'lowest' });
  };

  return (
    <div className="tablewrap">
      <table className="grid primary-columns">
        <thead>
          <tr>
            <th>Tipo</th>
            <th>Coluna primária</th>
            <th>Altura-alvo (m)</th>
            <th>Agora</th>
          </tr>
        </thead>
        <tbody>
          {kinds.map((kind) => {
            const choice = choices.find((candidate) => candidate.kind === kind)?.choice;
            return (
              <tr key={kind}>
                <td>{COLUMN_KIND_LABEL[kind]}</td>
                <td>
                  <select aria-label={`Coluna primária de ${COLUMN_KIND_LABEL[kind]}`} value={valueOf(choice)} onChange={(event) => pick(kind, event.target.value, choice)}>
                    <option value={DEFAULT}>{kind === 'wind_direction' ? 'Padrão: a mais próxima da velocidade primária' : 'Padrão: a mais alta'}</option>
                    {Object.entries(CRITERION_LABEL).map(([by, label]) => (
                      <option key={by} value={by}>
                        {label}
                      </option>
                    ))}
                    {measured
                      .filter((column) => column.kind === kind)
                      .map((column) => (
                        <option key={column.id} value={`${COLUMN_PREFIX}${column.id}`}>
                          {column.id}
                        </option>
                      ))}
                  </select>
                </td>
                <td>
                  {choice?.by === 'nearest' && (
                    <input
                      className="num"
                      aria-label={`Altura-alvo de ${COLUMN_KIND_LABEL[kind]}`}
                      defaultValue={choice.heightM}
                      onChange={(event) => {
                        const heightM = typedNumber(event.target.value);
                        if (heightM !== undefined && Number.isFinite(heightM)) choose(kind, { by: 'nearest', heightM });
                      }}
                    />
                  )}
                </td>
                <td>{dataset.primary.find((primary) => primary.kind === kind)?.column ?? '—'}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="hint">
        Critérios de altura seguem a regra da mais próxima: sensores a menos de 1 m um do outro empatam, e vence o de mais dados. A temperatura e a pressão primárias
        (entre as de unidade conhecida) dão a densidade do ar. Vale ao confirmar com OK: o arquivo é lido de novo.
      </p>
    </div>
  );
}

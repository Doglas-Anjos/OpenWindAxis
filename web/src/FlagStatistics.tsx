/**
 * Flag statistics (ESPEC-INTERFACE §5.3, "Inspecionar e remover flags";
 * ROTEIRO item 9): flagged steps and share of the record per flag and
 * column, every flagged segment with the rule evidence inside it, and
 * removal of a flag from one column or from all, as an undoable edit.
 */
import { useEffect, useState } from 'react';
import type { WindApi } from './api/client';
import type { DatasetDto } from './api/generated/DatasetDto';
import type { EditStateDto } from './api/generated/EditStateDto';
import type { FlaggedSegmentDto } from './api/generated/FlaggedSegmentDto';
import type { FlagStatisticsDto } from './api/generated/FlagStatisticsDto';
import { CRITERION_LABEL, describeCriterion, describeStatistic, FlagName, stepTime } from './FlagRules';
import { errorMessage, formatCount, formatNumber, NO_VALUE } from './format';

/** Segment rows listed; the rest are only counted. */
const MAX_LISTED_SEGMENTS = 300;
const PERCENT = 100;
/** Decimal places of a share of the record: small flags still show. */
const SHARE_DIGITS = 2;

const formatShare = (share: number) => `${formatNumber(share * PERCENT, SHARE_DIGITS)}%`;

interface Props {
  api: WindApi;
  /** A new object after every edit, so the statistics follow it. */
  dataset: DatasetDto;
  /** Shows the flags and summaries a removal left, like any other edit. */
  onEdit: (edited: Promise<EditStateDto>) => void;
  disabled: boolean;
  /** Open from the start (as a window of its own). */
  startOpen?: boolean;
}

/** The "Estatística de flags" card of the selected dataset. */
export function FlagStatisticsCard({ api, dataset, onEdit, disabled, startOpen = false }: Props) {
  /** The answer with the dataset it describes, so a late answer is drawn with its own columns and flags. */
  const [shown, setShown] = useState<{ statistics: FlagStatisticsDto; dataset: DatasetDto } | null>(null);
  const [error, setError] = useState('');
  /** Closed, the card asks nothing: the segment list can be long, and edits should not pay for it. */
  const [open, setOpen] = useState(startOpen);

  useEffect(() => {
    if (!open) return;
    let current = true;
    api.flagStatistics({ dataset: dataset.id }).then(
      (statistics) => {
        if (!current) return;
        setShown({ statistics, dataset });
        setError('');
      },
      (thrown) => {
        if (current) setError(errorMessage(thrown));
      },
    );
    return () => {
      current = false;
    };
  }, [api, dataset, open]);

  const remove = (flag: number, columns?: string[]) => onEdit(api.removeFlag({ dataset: dataset.id, flag, columns }));

  return (
    <section className="card">
      <details className="statistics" open={startOpen || undefined} onToggle={(event) => setOpen(event.currentTarget.open)}>
        <summary>
          <h3>Estatística de flags</h3>
        </summary>
        <p className="hint">
          Passos com cada flag por coluna e a parcela do registro. DP, mín., máx. e rajada seguem a média e não têm flag
          própria. Remover tira a flag do registro inteiro; os valores medidos não mudam, e Ctrl+Z desfaz.
        </p>
        {error && <div className="note err">{error}</div>}
        {shown && <StatisticsTables {...shown} remove={remove} disabled={disabled} />}
      </details>
    </section>
  );
}

/** The tables of one statistics answer: counts per flag and column, then the segments. */
export function StatisticsTables(props: {
  statistics: FlagStatisticsDto;
  dataset: DatasetDto;
  remove: (flag: number, columns?: string[]) => void;
  disabled: boolean;
}) {
  const { statistics, dataset, remove, disabled } = props;
  const used = (flag: number) => statistics.steps[flag].some((steps) => steps > 0);
  const listed = statistics.segments.slice(0, MAX_LISTED_SEGMENTS);
  return (
    <>
      <div className="tablewrap">
        <table>
          <thead>
            <tr>
              <th>Coluna</th>
              {dataset.flags.map((flag, index) => (
                <th key={index} className="r">
                  <FlagName dataset={dataset} flag={index} />
                  {used(index) && (
                    <button
                      className="x"
                      title={`Remover a flag ${flag.name} de todas as colunas`}
                      aria-label={`Remover a flag ${flag.name} de todas as colunas`}
                      disabled={disabled}
                      onClick={() => remove(index)}
                    >
                      ×
                    </button>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {/* Derived columns inherit their sources' flags and never carry their own. */}
            {dataset.columns.map((column, position) => column.derivedFrom ? null : (
              <tr key={column.id}>
                <td className="id">{column.id}</td>
                {dataset.flags.map((flag, index) => {
                  const steps = statistics.steps[index][position];
                  return (
                    <td key={index} className="r">
                      {steps === 0 ? (
                        NO_VALUE
                      ) : (
                        <>
                          {formatCount(steps)} <span className="share">({formatShare(statistics.share[index][position])})</span>
                          <button
                            className="x"
                            title={`Remover a flag ${flag.name} de ${column.id}`}
                            aria-label={`Remover a flag ${flag.name} de ${column.id}`}
                            disabled={disabled}
                            onClick={() => remove(index, [column.id])}
                          >
                            ×
                          </button>
                        </>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {statistics.segments.length > 0 && (
        <details className="segments">
          <summary>Trechos com flag ({formatCount(statistics.segments.length)})</summary>
          <div className="tablewrap">
            <table>
              <thead>
                <tr>
                  <th>Coluna</th>
                  <th>Flag</th>
                  <th>Início</th>
                  <th>Último passo</th>
                  <th className="r">Passos</th>
                  <th>Evidência</th>
                </tr>
              </thead>
              <tbody>
                {listed.map((segment) => (
                  <tr key={`${segment.column}-${segment.flag}-${segment.startStep}`}>
                    <td className="id">{segment.column}</td>
                    <td>
                      <FlagName dataset={dataset} flag={segment.flag} />
                    </td>
                    <td>{stepTime(dataset, segment.startStep)}</td>
                    <td>{stepTime(dataset, segment.endStep - 1)}</td>
                    <td className="r">{formatCount(segment.endStep - segment.startStep)}</td>
                    <td>
                      <SegmentEvidence dataset={dataset} segment={segment} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {statistics.segments.length > listed.length && (
            <p className="hint">
              Mostrando {formatCount(listed.length)} de {formatCount(statistics.segments.length)} trechos.
            </p>
          )}
        </details>
      )}
    </>
  );
}

/** The rules that flagged parts of a segment, or "manual". */
function SegmentEvidence({ dataset, segment }: { dataset: DatasetDto; segment: FlaggedSegmentDto }) {
  if (!segment.evidence.length) return <span className="muted">manual</span>;
  const units = dataset.columns.find((column) => column.id === segment.column)?.units ?? '';
  return (
    <>
      {segment.evidence.map((record, index) => {
        const partial = record.startStep !== segment.startStep || record.endStep !== segment.endStep;
        return (
          <div key={index} title={`duração mínima: ${record.minSteps} passo(s); medido em ${stepTime(dataset, record.statisticStep)}`}>
            {CRITERION_LABEL[record.criterion.kind]}: {describeCriterion(record.criterion)} — {describeStatistic(record, units)}
            {partial && (
              <span className="muted">
                {' '}
                ({stepTime(dataset, record.startStep)} a {stepTime(dataset, record.endStep - 1)})
              </span>
            )}
          </div>
        );
      })}
    </>
  );
}

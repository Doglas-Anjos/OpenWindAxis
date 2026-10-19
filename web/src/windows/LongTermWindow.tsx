/**
 * Revisar › Ajuste de longo prazo (MCP; PLANO § Longo prazo): the site's
 * speed against a long reference (reanalysis, another mast) imported as
 * another dataset of the workbook. The eight speed methods side by side,
 * their cross-validation and the recommendation with its reason [B2], the
 * long-term mean and Weibull, and the synthesized series as CSV.
 */
import { useMemo, useState } from 'react';
import { AnalysisLayout, ColumnSelect, isMeanDirection, isMeanSpeed, Plot, SAFE_TOOLTIP, useQuery } from '../Analyses';
import type { WindApi } from '../api/client';
import type { AveragingDto } from '../api/generated/AveragingDto';
import type { DatasetId } from '../api/generated/DatasetId';
import type { McpMethodDto } from '../api/generated/McpMethodDto';
import type { OffsetCorrelationDto } from '../api/generated/OffsetCorrelationDto';
import type { TimeOffsetDto } from '../api/generated/TimeOffsetDto';
import { errorMessage, formatCount, formatNumber, formatWithInterval, NO_VALUE } from '../format';
import { baseName, formatDay, typedNumber, type Entry } from '../workbook/model';
import { download, textFile, type Workbook } from '../workbook/useWorkbook';

const METHOD_LABEL: Record<McpMethodDto, string> = {
  least_squares: 'Mínimos quadrados',
  total_least_squares: 'Mínimos quadrados totais',
  variance_ratio: 'Razão de variância',
  speed_ratio: 'Razão de médias',
  speed_sort: 'SpeedSort (ordenadas)',
  weibull_fit: 'Ajuste Weibull',
  vertical_slice: 'Fatias verticais',
  matrix_time_series: 'Matrix Time Series (MTS)',
};
/** The original's matrix time series defaults: 1 m/s bins and a 3 h moving average. */
const DEFAULT_MTS_BIN = '1';
const DEFAULT_MTS_WINDOW_H = '3';
const AVERAGING_LABEL: Record<AveragingDto, string> = { hour: 'Horárias', day: 'Diárias' };
/** Digits of a line's coefficients and of the correlation. */
const COEFFICIENT_DIGITS = 4;
/** A spread ratio this far from 1 shrinks or stretches the variance noticeably. */
const SPREAD_TOLERANCE = 0.02;
/** Sector counts offered for the fits by reference direction (1: one fit). */
const SECTOR_CHOICES = [1, 4, 8, 12, 16];
/** Digits of the direction offset, °. */
const OFFSET_DIGITS = 1;
const MS_PER_HOUR = 3_600_000;
/** Digits of a shift, hours. */
const SHIFT_DIGITS = 1;
const OFFSET_CHART_HEIGHT = 260;
const PERCENT = 100;
/** Yearly divisions offered (wind-core mcp::YEARLY_DIVISIONS): groups of 12, 6, 4, 3, 2 or 1 months. */
const DIVISION_CHOICES = [1, 2, 3, 4, 6, 12];

interface Props {
  api: WindApi;
  entries: Entry[];
  initialDataset: DatasetId | null;
  workbook: Workbook;
}

/** The optimal time offset analysis: correlation against the shift, as the original's graph and table. */
function TimeOffsetPanel({ offset, referenceName, onApply }: { offset: TimeOffsetDto; referenceName: string; onApply: () => void }) {
  const rows: [string, OffsetCorrelationDto | null][] = [
    ['Velocidade', offset.speed],
    ['Direção', offset.direction],
    ['Temperatura', offset.temperature],
  ];
  const option = useMemo(
    () => ({
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'axis', valueFormatter: (value: number) => formatNumber(value, COEFFICIENT_DIGITS) },
      legend: { top: 4 },
      grid: { left: 64, right: 24, top: 40, bottom: 48 },
      xAxis: { type: 'value', name: 'Deslocamento da referência (h)', nameLocation: 'middle', nameGap: 28 },
      yAxis: { type: 'value', name: 'r', scale: true },
      series: rows
        .filter((row): row is [string, OffsetCorrelationDto] => row[1] != null)
        .map(([name, found]) => ({ type: 'line', name, showSymbol: false, data: found.correlations.map((value, index) => [offset.shiftsH[index], value]) })),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [offset],
  );
  const shiftText = (hours: number) => `${hours > 0 ? '+' : hours < 0 ? '−' : ''}${formatNumber(Math.abs(hours), SHIFT_DIGITS)} h`;
  return (
    <div className="time-offset">
      <h3>Deslocamento de tempo ótimo</h3>
      <Plot option={option} height={OFFSET_CHART_HEIGHT} />
      <table className="grid">
        <thead>
          <tr>
            <th>Grandeza</th>
            <th className="r">r sem deslocamento</th>
            <th className="r">r máximo</th>
            <th className="r">Diferença</th>
            <th className="r">Deslocamento</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(([name, found]) =>
            found ? (
              <tr key={name}>
                <td>{name}</td>
                <td className="r">{formatNumber(found.zeroShift, COEFFICIENT_DIGITS)}</td>
                <td className="r">{formatNumber(found.best, COEFFICIENT_DIGITS)}</td>
                <td className="r">
                  {found.best != null && found.zeroShift ? `${formatNumber(((found.best - found.zeroShift) / Math.abs(found.zeroShift)) * PERCENT, SHIFT_DIGITS)} %` : NO_VALUE}
                </td>
                <td className="r">{found.bestShiftH == null ? NO_VALUE : shiftText(found.bestShiftH)}</td>
              </tr>
            ) : null,
          )}
        </tbody>
      </table>
      {offset.recommendedH ? (
        <p className="hint">
          Recomendado (pela velocidade, como no original): deslocar {referenceName} em <b>{shiftText(offset.recommendedH)}</b>.{' '}
          <button className="btn" onClick={onApply}>
            Aplicar à referência
          </button>
        </p>
      ) : (
        <p className="hint">A correlação da velocidade já é máxima sem deslocamento.</p>
      )}
    </div>
  );
}

export function LongTermWindow({ api, entries, initialDataset, workbook }: Props) {
  const [siteId, setSiteId] = useState<DatasetId | null>(initialDataset ?? entries[0]?.result.dataset.id ?? null);
  const site = entries.find((entry) => entry.result.dataset.id === siteId) ?? entries[0];
  const others = entries.filter((entry) => entry !== site);
  const [referenceId, setReferenceId] = useState<DatasetId | null>(others[0]?.result.dataset.id ?? null);
  const reference = others.find((entry) => entry.result.dataset.id === referenceId) ?? others[0];
  const firstSpeed = (entry?: Entry) => entry?.result.dataset.columns.find(isMeanSpeed)?.id ?? null;
  const firstDirection = (entry?: Entry) => entry?.result.dataset.columns.find(isMeanDirection)?.id ?? null;
  const [siteColumn, setSiteColumn] = useState<string | null>(() => firstSpeed(site));
  const [referenceColumn, setReferenceColumn] = useState<string | null>(() => firstSpeed(reference));
  const [averaging, setAveraging] = useState<AveragingDto>('hour');
  const [referenceDirection, setReferenceDirection] = useState<string | null>(() => firstDirection(reference));
  const [siteDirection, setSiteDirection] = useState<string | null>(() => firstDirection(site));
  const [sectors, setSectors] = useState(1);
  const [exportMethod, setExportMethod] = useState<McpMethodDto | null>(null);
  const [confidence, setConfidence] = useState(false);
  const [divisions, setDivisions] = useState(1);
  const [cutoffText, setCutoffText] = useState('');
  const [zeroIntercept, setZeroIntercept] = useState(false);
  const [mtsReferenceBin, setMtsReferenceBin] = useState(DEFAULT_MTS_BIN);
  const [mtsTargetBin, setMtsTargetBin] = useState(DEFAULT_MTS_BIN);
  const [mtsWindow, setMtsWindow] = useState(DEFAULT_MTS_WINDOW_H);
  const cutoff = typedNumber(cutoffText);
  const positive = (text: string) => {
    const value = typedNumber(text);
    return value !== undefined && Number.isFinite(value) && value > 0 ? value : undefined;
  };
  const options = {
    yearlyDivisions: divisions,
    cutoff: cutoff !== undefined && Number.isFinite(cutoff) ? cutoff : undefined,
    zeroIntercept,
    mtsReferenceBin: positive(mtsReferenceBin),
    mtsTargetBin: positive(mtsTargetBin),
    mtsWindowH: positive(mtsWindow),
  };
  const request =
    site && reference && siteColumn && referenceColumn
      ? {
          site: { dataset: site.result.dataset.id, column: siteColumn },
          reference: { dataset: reference.result.dataset.id, column: referenceColumn },
          averaging,
          directions: referenceDirection ? { reference: referenceDirection, site: siteDirection ?? undefined, sectors } : undefined,
          options,
        }
      : null;
  const { data: answer, error } = useQuery(
    () => (request ? api.mcp({ ...request, confidence }) : undefined),
    [api, site?.result, reference?.result, siteColumn, referenceColumn, averaging, referenceDirection, siteDirection, sectors, confidence, JSON.stringify(options)],
  );
  const recommended = answer?.results.find((result) => result.method === answer.recommended);
  const [offset, setOffset] = useState<TimeOffsetDto | null>(null);
  const [offsetBusy, setOffsetBusy] = useState(false);
  const firstTemperature = (entry?: Entry) => entry?.result.dataset.columns.find((column) => column.kind === 'temperature' && column.subtype === 'mean')?.id;
  const analyseOffset = async () => {
    if (!request || !site || !reference) return;
    setOffsetBusy(true);
    try {
      const [siteTemperature, referenceTemperature] = [firstTemperature(site), firstTemperature(reference)];
      setOffset(
        await api.timeOffset({
          site: request.site,
          reference: request.reference,
          averaging,
          directions: siteDirection && referenceDirection ? [siteDirection, referenceDirection] : undefined,
          temperatures: siteTemperature && referenceTemperature ? [siteTemperature, referenceTemperature] : undefined,
        }),
      );
    } catch (thrown) {
      workbook.setErrors([errorMessage(thrown)]);
    }
    setOffsetBusy(false);
  };
  const applyOffset = async () => {
    if (!offset?.recommendedH || !reference) return;
    await workbook.applyOp(reference, { op: 'shift_start', byMs: offset.recommendedH * MS_PER_HOUR });
    setOffset(null);
  };
  const chosen = exportMethod ?? answer?.recommended ?? 'least_squares';

  const option = useMemo(() => {
    if (!answer) return null;
    return {
      animation: false,
      tooltip: { ...SAFE_TOOLTIP, trigger: 'item' },
      legend: { top: 4, type: 'scroll' },
      grid: { left: 64, right: 24, top: 40, bottom: 48 },
      xAxis: { type: 'value', scale: true, name: `Referência: ${referenceColumn}`, nameLocation: 'middle', nameGap: 28 },
      // Beside the axis, not above it, where the legend runs.
      yAxis: { type: 'value', scale: true, name: `Local: ${siteColumn}`, nameLocation: 'middle', nameGap: 40 },
      series: [
        { type: 'scatter', name: 'Períodos concorrentes', data: answer.pairsX.map((x, index) => [x, answer.pairsY[index]]), symbolSize: 3, large: true },
        ...answer.results.map((result) => ({
          type: 'line',
          name: METHOD_LABEL[result.method],
          showSymbol: false,
          data: result.curve,
        })),
      ],
    };
  }, [answer, referenceColumn, siteColumn]);

  const exportSeries = async () => {
    if (!request || !site) return;
    try {
      const csv = await api.mcpSeries({ ...request, method: chosen });
      download(`${baseName(site.file)}_longo_prazo_${chosen}.csv`, textFile(csv, 'text/csv'));
    } catch (thrown) {
      workbook.setErrors([errorMessage(thrown)]);
    }
  };

  if (entries.length < 2 || !site || !reference) {
    return (
      <div className="summary-empty">
        O ajuste de longo prazo compara o local com uma referência longa (reanálise ou outra torre). Adicione a série de referência como outro dataset do
        workbook (Arquivo › Adicionar dataset).
      </div>
    );
  }
  const datasetSelect = (label: string, value: DatasetId, choices: Entry[], onChange: (id: DatasetId) => void) => (
    <label>
      {label}
      <select value={value} onChange={(event) => onChange(Number(event.target.value))}>
        {choices.map((entry) => (
          <option key={entry.result.dataset.id} value={entry.result.dataset.id}>
            {entry.result.dataset.name}
          </option>
        ))}
      </select>
    </label>
  );
  const facts = answer && (
    <div className="facts">
      <span>
        {formatCount(answer.concurrent)} períodos concorrentes, {formatDay(answer.concurrentFromMs)} a {formatDay(answer.concurrentToMs)}
      </span>
      <span>
        r <b>{formatNumber(answer.correlation, COEFFICIENT_DIGITS)}</b>
      </span>
      <span>
        Referência: longo prazo {formatNumber(answer.referenceLongTermMean)} · concorrente {formatNumber(answer.referenceConcurrentMean)} (
        {formatCount(answer.referencePeriods)} períodos)
      </span>
      {answer.direction && (
        <span>
          Direção: local = referência {answer.direction.offsetDeg >= 0 ? '+' : '−'} <b>{formatNumber(Math.abs(answer.direction.offsetDeg), OFFSET_DIGITS)}°</b> (
          {formatCount(answer.direction.concurrent)} períodos)
        </span>
      )}
      {answer.divisions > 1 && (
        <span>
          Ajustes em {answer.divisions} subdivisões
          {answer.sectors > 1 && ` (${answer.sectors} setores da direção da referência${cutoff !== undefined ? ' e as abaixo do corte' : ''})`}
          {divisions > 1 && `, ${divisions} grupos de meses`}
        </span>
      )}
    </div>
  );
  const reason =
    answer &&
    recommended &&
    (answer.folds > 0
      ? `Recomendado: ${METHOD_LABEL[recommended.method]}, o menor erro das médias mensais deixadas de fora (RMSE ${formatNumber(recommended.cvRmse, COEFFICIENT_DIGITS)} em ${answer.folds} meses).` +
        (recommended.spreadRatio != null && Math.abs(recommended.spreadRatio - 1) > SPREAD_TOLERANCE
          ? ` Ele ${recommended.spreadRatio < 1 ? 'encolhe' : 'alarga'} a dispersão (razão ${formatNumber(recommended.spreadRatio, COEFFICIENT_DIGITS)}): para a energia, prefira um método com razão perto de 1.`
          : ' Ele preserva a dispersão das velocidades.')
      : 'Sem dois meses concorrentes não há validação cruzada; os métodos estão lado a lado, sem recomendação.');
  const table = answer && (
    <div className="tablewrap">
      <p className="hint">{reason}</p>
      <table className="grid">
        <thead>
          <tr>
            <th>Método</th>
            <th className="r">Inclinação</th>
            <th className="r">Intercepto</th>
            <th className="r" title="Erro quadrático médio das médias mensais deixadas de fora">
              RMSE validação
            </th>
            <th className="r" title="Erro médio (previsto − observado) das médias mensais deixadas de fora">
              Viés validação
            </th>
            <th className="r" title="Desvio padrão previsto sobre o observado, no período concorrente">
              Razão da dispersão
            </th>
            <th className="r">Média de longo prazo</th>
            <th className="r">k</th>
            <th className="r">A</th>
            {answer.divisions > 1 && (
              <th className="r" title="Subdivisões com um dia de períodos (no mínimo 8) para um ajuste próprio; as outras usam o ajuste de todos">
                Subdivisões próprias
              </th>
            )}
          </tr>
        </thead>
        <tbody>
          {answer.results.map((result) => (
            <tr key={result.method} className={result.method === answer.recommended ? 'chosen' : ''}>
              <td>
                {METHOD_LABEL[result.method]}
                {result.method === answer.recommended && ' ★'}
              </td>
              <td className="r">{result.slope == null ? NO_VALUE : formatNumber(result.slope, COEFFICIENT_DIGITS)}</td>
              <td className="r">{result.intercept == null ? NO_VALUE : formatNumber(result.intercept, COEFFICIENT_DIGITS)}</td>
              <td className="r">{result.cvRmse == null ? NO_VALUE : formatNumber(result.cvRmse, COEFFICIENT_DIGITS)}</td>
              <td className="r">{result.cvBias == null ? NO_VALUE : formatNumber(result.cvBias, COEFFICIENT_DIGITS)}</td>
              <td className="r">{formatNumber(result.spreadRatio, COEFFICIENT_DIGITS)}</td>
              <td className="r">
                <b>{formatWithInterval(result.longTermMean, result.longTermInterval)}</b>
              </td>
              <td className="r">{formatNumber(result.longTermK, COEFFICIENT_DIGITS - 1)}</td>
              <td className="r">{formatNumber(result.longTermA)}</td>
              {answer.divisions > 1 && (
                <td className="r">
                  {result.divisionsFitted} de {answer.divisions}
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return (
    <AnalysisLayout
      controls={
        <>
          {datasetSelect('Local', site.result.dataset.id, entries, (id) => {
            const next = entries.find((entry) => entry.result.dataset.id === id);
            setSiteId(id);
            setSiteColumn(firstSpeed(next));
            setSiteDirection(firstDirection(next));
          })}
          <ColumnSelect label="Velocidade do local" dataset={site.result.dataset} value={siteColumn} onChange={setSiteColumn} only={isMeanSpeed} />
          {datasetSelect('Referência', reference.result.dataset.id, others, (id) => {
            const next = entries.find((entry) => entry.result.dataset.id === id);
            setReferenceId(id);
            setReferenceColumn(firstSpeed(next));
            setReferenceDirection(firstDirection(next));
          })}
          <ColumnSelect label="Velocidade da referência" dataset={reference.result.dataset} value={referenceColumn} onChange={setReferenceColumn} only={isMeanSpeed} />
          <ColumnSelect
            label="Direção da referência"
            dataset={reference.result.dataset}
            value={referenceDirection}
            onChange={setReferenceDirection}
            only={isMeanDirection}
            none="Sem direção"
          />
          <label title="Uma reta por setor da direção da referência; setores com poucos pares usam o ajuste de todos.">
            Setores
            <select value={sectors} disabled={!referenceDirection} onChange={(event) => setSectors(Number(event.target.value))}>
              {SECTOR_CHOICES.map((choice) => (
                <option key={choice} value={choice}>
                  {choice === 1 ? 'Um ajuste para todas as direções' : choice}
                </option>
              ))}
            </select>
          </label>
          <ColumnSelect
            label="Direção do local"
            dataset={site.result.dataset}
            value={siteDirection}
            onChange={setSiteDirection}
            only={isMeanDirection}
            none="Sem direção"
          />
          <label>
            Médias
            <select value={averaging} onChange={(event) => setAveraging(event.target.value as AveragingDto)}>
              {Object.entries(AVERAGING_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label title="Os meses em grupos ajustados à parte: 4 dá jan–mar, abr–jun, jul–set e out–dez.">
            Divisões anuais
            <select value={divisions} onChange={(event) => setDivisions(Number(event.target.value))}>
              {DIVISION_CHOICES.map((choice) => (
                <option key={choice} value={choice}>
                  {choice === 1 ? 'Ano inteiro' : choice}
                </option>
              ))}
            </select>
          </label>
          <label title="Com setores, os períodos com a referência abaixo dela formam um grupo próprio, sem direção; sem setores, ficam só fora dos ajustes. A razão de médias deixa de fora o par com qualquer das velocidades abaixo dela.">
            Velocidade de corte (m/s)
            <input inputMode="decimal" value={cutoffText} placeholder="nenhuma" onChange={(event) => setCutoffText(event.target.value)} />
          </label>
          <label className="check" title="Mínimos quadrados, mínimos quadrados totais, razão de variância e SpeedSort passam pela origem.">
            <input type="checkbox" checked={zeroIntercept} onChange={(event) => setZeroIntercept(event.target.checked)} />
            Intercepto zero
          </label>
          <fieldset className="mts">
            <legend title="Distribuição conjunta em faixas; faixas da referência com menos de 10 pares usam os mínimos quadrados.">Matrix Time Series</legend>
            <label>
              Faixa da referência (m/s)
              <input inputMode="decimal" value={mtsReferenceBin} onChange={(event) => setMtsReferenceBin(event.target.value)} />
            </label>
            <label>
              Faixa do local (m/s)
              <input inputMode="decimal" value={mtsTargetBin} onChange={(event) => setMtsTargetBin(event.target.value)} />
            </label>
            <label title="Média móvel centrada da série de percentis.">
              Janela da média móvel (h)
              <input inputMode="decimal" value={mtsWindow} onChange={(event) => setMtsWindow(event.target.value)} />
            </label>
          </fieldset>
          <label className="check">
            <input type="checkbox" checked={confidence} onChange={(event) => setConfidence(event.target.checked)} />
            Intervalo de confiança (95 %)
          </label>
          <label>
            Método da série exportada
            <select value={chosen} onChange={(event) => setExportMethod(event.target.value as McpMethodDto)}>
              {Object.entries(METHOD_LABEL).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <button className="btn" disabled={!answer} onClick={exportSeries}>
            Exportar série de longo prazo (CSV)
          </button>
          <button
            className="btn"
            disabled={!request || offsetBusy}
            onClick={analyseOffset}
            title="A correlação do local com a referência deslocada hora a hora, até 24 h para cada lado, na velocidade, na direção e na temperatura."
          >
            {offsetBusy ? 'Analisando…' : 'Deslocamento de tempo ótimo'}
          </button>
          <p className="hint">
            Os dois datasets são tomados no mesmo relógio. Um período conta com metade dos passos com dado; as flags e exclusões de cada dataset valem.
          </p>
        </>
      }
      facts={facts || undefined}
      error={error}
      graph={
        option ? (
          <>
            <Plot option={option} />
            {offset && <TimeOffsetPanel offset={offset} referenceName={reference.result.dataset.name} onApply={applyOffset} />}
          </>
        ) : undefined
      }
      table={table || undefined}
    />
  );
}

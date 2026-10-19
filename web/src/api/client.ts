/**
 * Typed access to the WebAssembly core, which runs in a Web Worker
 * (api/worker.ts): every call sends one request DTO as JSON and resolves to
 * one response DTO. The DTO types are generated from crates/wind-api
 * (`cargo test -p wind-api`), so the two sides cannot drift. Failures reject
 * with {@link ApiError}, carrying the core's stable error code.
 */
import type { AnalyzeRequest } from './generated/AnalyzeRequest';
import type { ApplyOpRequest } from './generated/ApplyOpRequest';
import type { CoverageDto } from './generated/CoverageDto';
import type { CoverageRequest } from './generated/CoverageRequest';
import type { DatasetRequest } from './generated/DatasetRequest';
import type { DiurnalDto } from './generated/DiurnalDto';
import type { DiurnalRequest } from './generated/DiurnalRequest';
import type { EditStateDto } from './generated/EditStateDto';
import type { ErrorDto } from './generated/ErrorDto';
import type { ExportCsvRequest } from './generated/ExportCsvRequest';
import type { ExtremesDto } from './generated/ExtremesDto';
import type { ExtremesRequest } from './generated/ExtremesRequest';
import type { FlagByRulesDto } from './generated/FlagByRulesDto';
import type { FlagByRulesRequest } from './generated/FlagByRulesRequest';
import type { FlagStatisticsDto } from './generated/FlagStatisticsDto';
import type { ExportWaspTabRequest } from './generated/ExportWaspTabRequest';
import type { FlagRequest } from './generated/FlagRequest';
import type { HistogramDto } from './generated/HistogramDto';
import type { HistogramRequest } from './generated/HistogramRequest';
import type { ImportRequest } from './generated/ImportRequest';
import type { ImportResultDto } from './generated/ImportResultDto';
import type { McpDto } from './generated/McpDto';
import type { McpRequest } from './generated/McpRequest';
import type { McpSeriesRequest } from './generated/McpSeriesRequest';
import type { MonthlyMeansDto } from './generated/MonthlyMeansDto';
import type { WorkflowOutcomeDto } from './generated/WorkflowOutcomeDto';
import type { DefineFlagsRequest } from './generated/DefineFlagsRequest';
import type { EpeFileDto } from './generated/EpeFileDto';
import type { ExportEpeRequest } from './generated/ExportEpeRequest';
import type { WorkflowRunRequest } from './generated/WorkflowRunRequest';
import type { MonthlyTableDto } from './generated/MonthlyTableDto';
import type { MonthlyTableRequest } from './generated/MonthlyTableRequest';
import type { PeriodMeansDto } from './generated/PeriodMeansDto';
import type { PeriodMeansRequest } from './generated/PeriodMeansRequest';
import type { ScatterDto } from './generated/ScatterDto';
import type { ScatterRequest } from './generated/ScatterRequest';
import type { MonthlyMeansRequest } from './generated/MonthlyMeansRequest';
import type { RemoveFlagRequest } from './generated/RemoveFlagRequest';
import type { RestoreDto } from './generated/RestoreDto';
import type { SeriesRequest } from './generated/SeriesRequest';
import type { ShearDto } from './generated/ShearDto';
import type { ShearRequest } from './generated/ShearRequest';
import type { ExportOpenwindRequest } from './generated/ExportOpenwindRequest';
import type { OpenwindFileDto } from './generated/OpenwindFileDto';
import type { OpenwindMeansDto } from './generated/OpenwindMeansDto';
import type { SelectionCountDto } from './generated/SelectionCountDto';
import type { SpeedDistributionDto } from './generated/SpeedDistributionDto';
import type { SpeedDistributionRequest } from './generated/SpeedDistributionRequest';
import type { SelectionRequest } from './generated/SelectionRequest';
import type { DistortionDetailDto } from './generated/DistortionDetailDto';
import type { DistortionDetailRequest } from './generated/DistortionDetailRequest';
import type { ExpressionCheckDto } from './generated/ExpressionCheckDto';
import type { ExpressionCheckRequest } from './generated/ExpressionCheckRequest';
import type { ShearFrequencyDto } from './generated/ShearFrequencyDto';
import type { TimeOffsetDto } from './generated/TimeOffsetDto';
import type { TimeOffsetRequest } from './generated/TimeOffsetRequest';
import type { ShearFrequencyRequest } from './generated/ShearFrequencyRequest';
import type { ShearViewDto } from './generated/ShearViewDto';
import type { ShearViewRequest } from './generated/ShearViewRequest';
import type { TowerDistortionDto } from './generated/TowerDistortionDto';
import type { TowerDistortionRequest } from './generated/TowerDistortionRequest';
import type { TowerShadingDto } from './generated/TowerShadingDto';
import type { TowerShadingRequest } from './generated/TowerShadingRequest';
import type { TurbulenceDto } from './generated/TurbulenceDto';
import type { TurbulenceRequest } from './generated/TurbulenceRequest';
import type { UnflagRequest } from './generated/UnflagRequest';
import type { WindRoseDto } from './generated/WindRoseDto';
import type { WindRoseRequest } from './generated/WindRoseRequest';
import { API_VERSION } from './generated/version';
import type { CoreMethod, WorkerAnswer, WorkerCall, WorkerReady } from './protocol';
import { errorMessage } from '../format';

/** Error code for a failure that did not come from the core's contract. */
const INTERNAL_ERROR = 'internal';
/** Error code for a core built from a different contract version. */
const VERSION_MISMATCH = 'api_version_mismatch';
/** Error code once the worker has died: every call fails until a reload. */
const WORKER_LOST = 'worker_lost';

/** A core failure, with the stable code the UI can translate. */
export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** The core reports failures as an `ErrorDto` in JSON; anything else is internal. */
function toApiError(text: string): ApiError {
  try {
    const dto = JSON.parse(text) as ErrorDto;
    return new ApiError(dto.code, dto.message);
  } catch {
    return new ApiError(INTERNAL_ERROR, text);
  }
}

interface PendingCall {
  resolve: (result: string | Float64Array | null) => void;
  reject: (error: ApiError) => void;
}

/** One session of the core, holding every imported dataset. */
export class WindApi {
  private nextCallId = 0;
  private readonly pending = new Map<number, PendingCall>();
  /** Set when the worker dies; from then on every call fails with it. */
  private lost: ApiError | null = null;

  private constructor(private readonly worker: Worker) {
    worker.onmessage = (event: MessageEvent<WorkerAnswer>) => this.settle(event.data);
    worker.onerror = (event) => this.fail(`the core's worker stopped: ${event.message || 'unknown error'}`);
    worker.onmessageerror = () => this.fail("an answer from the core's worker could not be read");
  }

  /** Rejects every pending call and all later ones: the session state is gone. */
  private fail(message: string) {
    this.lost = new ApiError(WORKER_LOST, `${message}; reload the page`);
    for (const call of this.pending.values()) call.reject(this.lost);
    this.pending.clear();
    this.worker.terminate();
  }

  /** Starts the core's worker and checks it speaks this contract version. */
  static load(): Promise<WindApi> {
    const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    return new Promise((resolve, reject) => {
      const refuse = (error: ApiError) => {
        worker.terminate();
        reject(error);
      };
      worker.onerror = (event) => refuse(new ApiError(INTERNAL_ERROR, event.message));
      worker.onmessage = (event: MessageEvent<WorkerReady>) => {
        const ready = event.data;
        if (!ready.ready) {
          refuse(new ApiError(INTERNAL_ERROR, ready.error));
        } else if (ready.apiVersion !== API_VERSION) {
          refuse(new ApiError(VERSION_MISMATCH, `core speaks API ${ready.apiVersion}, the UI expects ${API_VERSION}`));
        } else {
          resolve(new WindApi(worker));
        }
      };
    });
  }

  private settle(answer: WorkerAnswer) {
    const call = this.pending.get(answer.callId);
    if (!call) return;
    this.pending.delete(answer.callId);
    if ('error' in answer) call.reject(toApiError(answer.error));
    else call.resolve(answer.result);
  }

  /** Posts one call; `bytes` (import only) is transferred, so the caller's copy is emptied. */
  private call(method: CoreMethod, request: object, bytes?: Uint8Array): Promise<string | Float64Array | null> {
    if (this.lost) return Promise.reject(this.lost);
    const callId = this.nextCallId++;
    const message: WorkerCall = { callId, method, request: JSON.stringify(request), bytes };
    return new Promise((resolve, reject) => {
      this.pending.set(callId, { resolve, reject });
      try {
        this.worker.postMessage(message, bytes ? [bytes.buffer] : []);
      } catch (thrown) {
        // A value that cannot be cloned or transferred: nothing reached the worker.
        this.pending.delete(callId);
        reject(new ApiError(INTERNAL_ERROR, errorMessage(thrown)));
      }
    });
  }

  /** A call answered with a response DTO in JSON. */
  private async exchange<Response>(method: CoreMethod, request: object, bytes?: Uint8Array): Promise<Response> {
    const answer = (await this.call(method, request, bytes)) as string;
    try {
      return JSON.parse(answer) as Response;
    } catch (thrown) {
      throw new ApiError(INTERNAL_ERROR, `unreadable answer to ${method}: ${errorMessage(thrown)}`);
    }
  }

  /** A call answered with plain text (an export). */
  private async text(method: CoreMethod, request: object): Promise<string> {
    return (await this.call(method, request)) as string;
  }

  /** Imports a file from its raw bytes (`new Uint8Array(await file.arrayBuffer())`); the bytes are handed over. */
  importCsv(bytes: Uint8Array, request: ImportRequest): Promise<ImportResultDto> {
    return this.exchange('importCsv', request, bytes);
  }

  /** `[t_ms, value, t_ms, value, …]` for a chart, NaN at gaps. */
  async series(request: SeriesRequest): Promise<Float64Array> {
    return (await this.call('series', request)) as Float64Array;
  }

  flag(request: FlagRequest): Promise<EditStateDto> {
    return this.exchange('flag', request);
  }

  unflag(request: UnflagRequest): Promise<EditStateDto> {
    return this.exchange('unflag', request);
  }

  /** One op of any kind (flags, rules, scale and offset, delete, time shifts) as one undoable edit. */
  applyOp(request: ApplyOpRequest): Promise<EditStateDto> {
    return this.exchange('applyOp', request);
  }

  /** Takes back the dataset's last edit; the state is unchanged when there is none. */
  undo(request: DatasetRequest): Promise<EditStateDto> {
    return this.exchange('undo', request);
  }

  /** Reapplies the dataset's last undone edit; the state is unchanged when there is none. */
  redo(request: DatasetRequest): Promise<EditStateDto> {
    return this.exchange('redo', request);
  }

  coverage(request: CoverageRequest): Promise<CoverageDto> {
    return this.exchange('coverage', request);
  }

  histogram(request: HistogramRequest): Promise<HistogramDto> {
    return this.exchange('histogram', request);
  }

  windRose(request: WindRoseRequest): Promise<WindRoseDto> {
    return this.exchange('windRose', request);
  }

  diurnal(request: DiurnalRequest): Promise<DiurnalDto> {
    return this.exchange('diurnal', request);
  }

  /** Mean by calendar month, over every year of the record. */
  monthlyMeans(request: MonthlyMeansRequest): Promise<MonthlyMeansDto> {
    return this.exchange('monthlyMeans', request);
  }

  turbulence(request: TurbulenceRequest): Promise<TurbulenceDto> {
    return this.exchange('turbulence', request);
  }

  /** Configurar dataset › Adicionar coluna: whether an algebraic expression parses, and where it does not. */
  checkExpression(request: ExpressionCheckRequest): Promise<ExpressionCheckDto> {
    return this.exchange('checkExpression', request);
  }

  /** Analisar › Cisalhamento: speed, speed SD or direction shear by up to two divisions. */
  shearView(request: ShearViewRequest): Promise<ShearViewDto> {
    return this.exchange('shearView', request);
  }

  /** Analisar › Cisalhamento: the step-by-step α or veer rate in bins. */
  shearFrequency(request: ShearFrequencyRequest): Promise<ShearFrequencyDto> {
    return this.exchange('shearFrequency', request);
  }

  /** Analisar › Distorção da torre: one pair per year, per month and as a scatter. */
  distortionDetail(request: DistortionDetailRequest): Promise<DistortionDetailDto> {
    return this.exchange('distortionDetail', request);
  }

  /** Analisar › Distorção da torre: every anemometer pair by direction sector, with its factors. */
  towerDistortion(request: TowerDistortionRequest): Promise<TowerDistortionDto> {
    return this.exchange('towerDistortion', request);
  }

  /** Analisar › Distribuição de velocidade: the four Weibull fits side by side. */
  speedDistribution(request: SpeedDistributionRequest): Promise<SpeedDistributionDto> {
    return this.exchange('speedDistribution', request);
  }

  /** Flag › Flag por dispersão: how many steps a selection holds. */
  selectionCount(request: SelectionRequest): Promise<SelectionCountDto> {
    return this.exchange('selectionCount', request);
  }

  /** Flag › Sombra da torre: the anemometer pairs and their shaded sectors. */
  towerShading(request: TowerShadingRequest): Promise<TowerShadingDto> {
    return this.exchange('towerShading', request);
  }

  /** Any analysis by the name the core's dispatch knows it by; the answer as the plain JSON object. */
  analyze(request: AnalyzeRequest): Promise<unknown> {
    return this.exchange('analyze', request);
  }

  /** Applies a workflow to some datasets, in order. */
  runWorkflow(request: WorkflowRunRequest): Promise<WorkflowOutcomeDto[]> {
    return this.exchange('runWorkflow', request);
  }

  /** The long-term adjustment of a site column against a reference column of another dataset. */
  mcp(request: McpRequest): Promise<McpDto> {
    return this.exchange('mcp', request);
  }

  /** Revisar › Ajuste de longo prazo: the site against the reference shifted, hour by hour. */
  timeOffset(request: TimeOffsetRequest): Promise<TimeOffsetDto> {
    return this.exchange('timeOffset', request);
  }

  /** The site's long-term series by one method, as CSV text. */
  mcpSeries(request: McpSeriesRequest): Promise<string> {
    return this.text('mcpSeries', request);
  }

  extremes(request: ExtremesRequest): Promise<ExtremesDto> {
    return this.exchange('extremes', request);
  }

  /** Means per day, month or year, from the kept per-day summaries. */
  periodMeans(request: PeriodMeansRequest): Promise<PeriodMeansDto> {
    return this.exchange('periodMeans', request);
  }

  monthlyTable(request: MonthlyTableRequest): Promise<MonthlyTableDto> {
    return this.exchange('monthlyTable', request);
  }

  scatter(request: ScatterRequest): Promise<ScatterDto> {
    return this.exchange('scatter', request);
  }

  shear(request: ShearRequest): Promise<ShearDto> {
    return this.exchange('shear', request);
  }

  /** The dataset as CSV text. */
  exportCsv(request: ExportCsvRequest): Promise<string> {
    return this.text('exportCsv', request);
  }

  /** The dataset's metadata as pretty JSON text. */
  exportJson(request: DatasetRequest): Promise<string> {
    return this.text('exportJson', request);
  }

  /** A WAsP observed wind climate (`.tab`) as text; layout in docs/formatos/wasp-tab.md. */
  exportWaspTab(request: ExportWaspTabRequest): Promise<string> {
    return this.text('exportWaspTab', request);
  }

  async remove(request: DatasetRequest): Promise<void> {
    await this.call('remove', request);
  }

  /** Runs flag rules over the whole record as one undoable edit; every stretch found keeps its evidence. */
  flagByRules(request: FlagByRulesRequest): Promise<FlagByRulesDto> {
    return this.exchange('flagByRules', request);
  }

  /** What the rules would flag, nothing flagged (Testar). */
  testRules(request: FlagByRulesRequest): Promise<FlagByRulesDto> {
    return this.exchange('testRules', request);
  }

  /** What a project saves for the dataset: flags, segments and rule evidence. */
  projectState(request: DatasetRequest): Promise<RestoreDto> {
    return this.exchange('projectState', request);
  }

  /** Flagged steps per flag and column, and every flagged segment with its rule evidence. */
  flagStatistics(request: DatasetRequest): Promise<FlagStatisticsDto> {
    return this.exchange('flagStatistics', request);
  }

  /** The EPE file of a dataset and the name EPE asks for it. */
  exportEpe(request: ExportEpeRequest): Promise<EpeFileDto> {
    return this.exchange('exportEpe', request);
  }

  /** Exportar dados › Openwind: the original's three means of the speeds a file would export. */
  openwindMeans(request: ExportOpenwindRequest): Promise<OpenwindMeansDto> {
    return this.exchange('openwindMeans', request);
  }

  /** Exportar dados › Openwind: the met mast file (MM 2.1) of one height. */
  exportOpenwind(request: ExportOpenwindRequest): Promise<OpenwindFileDto> {
    return this.exchange('exportOpenwind', request);
  }

  /** Replaces the dataset's flags; segments, evidence follow their flag (Definir flags). */
  defineFlags(request: DefineFlagsRequest): Promise<EditStateDto> {
    return this.exchange('defineFlags', request);
  }

  /** Removes one flag over the whole record, from the given columns or all, as one undoable edit. */
  removeFlag(request: RemoveFlagRequest): Promise<EditStateDto> {
    return this.exchange('removeFlag', request);
  }
}

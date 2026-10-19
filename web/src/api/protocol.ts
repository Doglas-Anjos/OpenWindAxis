/**
 * Messages between the page and the core's Web Worker (api/worker.ts).
 * Requests and responses keep the wind-api JSON as text, so the worker never
 * needs the DTO types; bulk numbers travel as transferred buffers.
 */

/** Core methods the worker runs, named as on the WebAssembly `Session`. */
export type CoreMethod =
  | 'importCsv'
  | 'series'
  | 'flag'
  | 'unflag'
  | 'applyOp'
  | 'undo'
  | 'redo'
  | 'coverage'
  | 'histogram'
  | 'windRose'
  | 'diurnal'
  | 'monthlyMeans'
  | 'shear'
  | 'turbulence'
  | 'towerShading'
  | 'selectionCount'
  | 'speedDistribution'
  | 'towerDistortion'
  | 'shearView'
  | 'distortionDetail'
  | 'timeOffset'
  | 'checkExpression'
  | 'shearFrequency'
  | 'exportOpenwind'
  | 'openwindMeans'
  | 'testRules'
  | 'extremes'
  | 'mcp'
  | 'runWorkflow'
  | 'mcpSeries'
  | 'analyze'
  | 'periodMeans'
  | 'monthlyTable'
  | 'scatter'
  | 'exportCsv'
  | 'exportJson'
  | 'flagByRules'
  | 'projectState'
  | 'flagStatistics'
  | 'removeFlag'
  | 'defineFlags'
  | 'exportEpe'
  | 'exportWaspTab'
  | 'remove';

/** Page → worker: one core call. */
export interface WorkerCall {
  /** Matches the answer to its call. */
  callId: number;
  method: CoreMethod;
  /** The request DTO as JSON. */
  request: string;
  /** File bytes, only for `importCsv`; transferred, not copied. */
  bytes?: Uint8Array;
}

/** Worker → page: the answer to one call. */
export type WorkerAnswer =
  | { callId: number; result: string | Float64Array | null }
  /** `error` is an ErrorDto as JSON, or a plain message for a non-core failure. */
  | { callId: number; error: string };

/** Worker → page, once, before any answer: whether the core loaded. */
export type WorkerReady = { ready: true; apiVersion: number } | { ready: false; error: string };

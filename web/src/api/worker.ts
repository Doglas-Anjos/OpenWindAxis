/**
 * Hosts the WebAssembly core off the main thread, so imports and analyses
 * never freeze the page (ARQUITETURA P7). Calls run one at a time in arrival
 * order; series buffers go back transferred, without a copy.
 */
import init, { Session } from '../wasm/wind_wasm.js';
import type { CoreMethod, WorkerAnswer, WorkerCall, WorkerReady } from './protocol';
import { errorMessage } from '../format';

/** The worker's global scope, typed by the port API it shares with `Worker`. */
const workerScope = self as unknown as Worker;

let session: Session | null = null;

init()
  .then(() => {
    session = new Session();
    const ready: WorkerReady = { ready: true, apiVersion: Session.apiVersion() };
    workerScope.postMessage(ready);
  })
  .catch((thrown: unknown) => {
    const failed: WorkerReady = { ready: false, error: errorMessage(thrown) };
    workerScope.postMessage(failed);
  });

/** Runs one call on the session; `null` for calls without a result. */
function run(core: Session, method: CoreMethod, request: string, bytes: Uint8Array | undefined): string | Float64Array | null {
  switch (method) {
    case 'importCsv':
      return core.importCsv(bytes ?? new Uint8Array(), request);
    case 'series':
      return core.series(request);
    case 'remove':
      core.remove(request);
      return null;
    default:
      return core[method](request);
  }
}

workerScope.onmessage = (event: MessageEvent<WorkerCall>) => {
  const { callId, method, request, bytes } = event.data;
  let answer: WorkerAnswer;
  try {
    answer = { callId, result: run(session!, method, request, bytes) };
  } catch (thrown) {
    answer = { callId, error: errorMessage(thrown) };
  }
  const transfer = 'result' in answer && answer.result instanceof Float64Array ? [answer.result.buffer] : [];
  workerScope.postMessage(answer, transfer);
};

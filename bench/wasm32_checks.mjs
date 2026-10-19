// wasm32-only regressions native tests cannot see (usize is 32-bit there).
// Usage (after `npm run wasm` in web/): node bench/wasm32_checks.mjs web/src/wasm
// Decimation past ~2.15 M steps must keep time order and extremes.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

/** One-minute steps: 3 M of them is past the 32-bit overflow of steps × buckets. */
const STEPS = 3_000_000;
const STEP_MS = 60_000;
const SPIKE_STEP = 2_500_000;
const SPIKE_VALUE = 5000;
const PATTERN_PERIOD = 1000;
const OVERVIEW_BUCKETS = 2000;
const FIRST_MS = Date.UTC(2015, 0, 1);

const lines = ['time,ws'];
for (let step = 0; step < STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * STEP_MS).toISOString().slice(0, 16).replace('T', ' ');
  lines.push(`${stamp},${step === SPIKE_STEP ? SPIKE_VALUE : step % PATTERN_PERIOD}`);
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(new TextEncoder().encode(lines.join('\n')), send({ name: 'big' })));
console.log('steps', imported.dataset.stepCount);
const packed = session.series(
  send({ dataset: imported.dataset.id, column: 'ws', fromMs: FIRST_MS, toMs: FIRST_MS + STEPS * STEP_MS, buckets: OVERVIEW_BUCKETS }),
);
let ordered = true;
let spikeFound = false;
for (let index = 0; index + 3 < packed.length; index += 2) ordered &&= packed[index] <= packed[index + 2];
for (let index = 0; index < packed.length; index += 2) {
  spikeFound ||= packed[index + 1] === SPIKE_VALUE && packed[index] === FIRST_MS + SPIKE_STEP * STEP_MS;
}
const points = packed.length / 2;
console.log(`points=${points} ordered=${ordered} spike_found=${spikeFound}`);
process.exit(ordered && spikeFound && points <= 2 * OVERVIEW_BUCKETS ? 0 : 1);

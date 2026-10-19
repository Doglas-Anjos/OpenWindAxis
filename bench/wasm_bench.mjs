// WASM timings for the interaction path (docs/ARQUITETURA.md §4.4), through
// the same JSON contract the UI uses.
// Usage (after `npm run wasm` in web/): node bench/wasm_bench.mjs web/src/wasm
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const ROWS = 1_000_000;
const STEP_MS = 600_000;
const FIRST_MS = Date.UTC(2005, 0, 1);
const SPEED_HEIGHTS = [40, 50, 60, 70, 80, 90, 100, 110, 120, 130];
const DIRECTION_SENSORS = 5;
const TEMPERATURE_SENSORS = 5;
const EXISTING_SEGMENTS = 200;
const SEGMENT_SPACING = 4_000;
const SEGMENT_LENGTH = 500;
const DAY_OF_STEPS = 144;
const TOP_SPEED = 'WS130_Avg';
const FIRST_DIRECTION = 'Dir100';
const ROSE_SECTORS = 16;
const HISTOGRAM_BIN_WIDTH = 0.5;
const REPETITIONS = 5;
const SLOW_REPETITIONS = 3;

// xorshift32 on 32-bit integer ops: exact in JS, unlike an LCG on doubles.
let state = 2463534242;
const uniform = () => {
  state ^= state << 13;
  state ^= state >>> 17;
  state ^= state << 5;
  return (state >>> 0) / 4294967296;
};

const header = ['time'];
for (const height of SPEED_HEIGHTS) header.push(`WS${height}_Avg`, `WS${height}_SD`);
for (let sensor = 0; sensor < DIRECTION_SENSORS; sensor++) header.push(`Dir${100 + sensor}`);
for (let sensor = 0; sensor < TEMPERATURE_SENSORS; sensor++) header.push(`Temp${2 + sensor}m`);
const lines = [header.join(',')];
for (let row = 0; row < ROWS; row++) {
  const fields = [new Date(FIRST_MS + row * STEP_MS).toISOString().slice(0, 16).replace('T', ' ')];
  for (const _ of SPEED_HEIGHTS) fields.push((8 * Math.sqrt(-Math.log(1 - uniform()))).toFixed(2), uniform().toFixed(2));
  for (let sensor = 0; sensor < DIRECTION_SENSORS; sensor++) fields.push((360 * uniform()).toFixed(1));
  for (let sensor = 0; sensor < TEMPERATURE_SENSORS; sensor++) fields.push((25 + 5 * uniform()).toFixed(1));
  lines.push(fields.join(','));
}
// Bytes, as the browser reads them with file.arrayBuffer().
const fileBytes = new TextEncoder().encode(lines.join('\n'));

// Prints the best run and, when it differs, the first (cold) one: the first
// call also pays for growing WASM memory, which a real session pays once.
const time = (label, repetitions, run) => {
  let best = Infinity;
  let first;
  let output;
  for (let repetition = 0; repetition < repetitions; repetition++) {
    const started = performance.now();
    output = run(repetition);
    const elapsed = performance.now() - started;
    first ??= elapsed;
    best = Math.min(best, elapsed);
  }
  const cold = repetitions > 1 && first > 1.5 * best ? `   (first run ${first.toFixed(0)} ms)` : '';
  console.log(`${label.padEnd(44)} ${best.toFixed(2).padStart(9)} ms${cold}`);
  return output;
};

const session = new Session();
const send = (request) => JSON.stringify(request);
// Best of 3; from the second run on, each import replaces the first dataset,
// so memory holds one dataset as in a real session.
let datasetId;
const imported = JSON.parse(
  time('import 1M rows × 30 columns', SLOW_REPETITIONS, () => {
    const response = session.importCsv(fileBytes, send({ name: 'bench', replace: datasetId }));
    datasetId ??= JSON.parse(response).dataset.id;
    return response;
  }),
);
console.log(`  columns=${imported.dataset.columns.length} steps=${imported.dataset.stepCount}`);

const timeOf = (step) => FIRST_MS + step * STEP_MS;
const speedColumns = imported.dataset.columns.filter((column) => column.kind === 'wind_speed').map((column) => column.id);
const flagCount = imported.dataset.flags.length;
for (let segment = 0; segment < EXISTING_SEGMENTS; segment++) {
  const start = segment * SEGMENT_SPACING;
  const stretch = { fromMs: timeOf(start), toMs: timeOf(start + SEGMENT_LENGTH) };
  session.flag(send({ dataset: datasetId, columns: [speedColumns[segment % speedColumns.length]], flag: segment % flagCount, ...stretch }));
}
time('flag 1 column + summaries of 30 (JSON out)', REPETITIONS, (repetition) => {
  const start = 900_000 + repetition * 1_000;
  const stretch = { fromMs: timeOf(start), toMs: timeOf(start + DAY_OF_STEPS - 1) };
  return session.flag(send({ dataset: datasetId, columns: ['WS40_Avg'], flag: 0, ...stretch }));
});
time('coverage, 30 columns', REPETITIONS, () => session.coverage(send({ dataset: datasetId })));
time('wind rose, 16 sectors', REPETITIONS, () =>
  session.windRose(send({ dataset: datasetId, direction: FIRST_DIRECTION, speed: TOP_SPEED, sectors: ROSE_SECTORS })),
);
time('diurnal profile', REPETITIONS, () => session.diurnal(send({ dataset: datasetId, column: TOP_SPEED })));
time('histogram, no fit', REPETITIONS, () =>
  session.histogram(send({ dataset: datasetId, column: TOP_SPEED, binWidth: HISTOGRAM_BIN_WIDTH })),
);
time('histogram + Weibull MLE', SLOW_REPETITIONS, () =>
  session.histogram(send({ dataset: datasetId, column: TOP_SPEED, binWidth: HISTOGRAM_BIN_WIDTH, weibull: 'max_likelihood' })),
);
const speedMeans = SPEED_HEIGHTS.map((height) => `WS${height}_Avg`);
time('shear, 10 heights + 16 sectors', REPETITIONS, () =>
  session.shear(send({ dataset: datasetId, columns: speedMeans, direction: FIRST_DIRECTION, sectors: ROSE_SECTORS })),
);
time('extremes, monthly maxima', REPETITIONS, () =>
  session.extremes(send({ dataset: datasetId, column: TOP_SPEED, method: { kind: 'periodic_maxima', period: 'month' } })),
);
time('extremes, storms at the recommended threshold', SLOW_REPETITIONS, () =>
  session.extremes(send({ dataset: datasetId, column: TOP_SPEED, method: { kind: 'independent_storms' } })),
);

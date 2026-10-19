// Reproducible recipes across builds (ARQUITETURA C3): recipes made by the
// WASM core, as the browser makes them (SHA-256, configuration, state with a
// flag and a value edit, request, answer), replayed by the native `wind`
// binary. Every answer must come back number by number: the core is
// deterministic bit for bit between WASM and native.
// Usage (after `npm run wasm` in web/ and `cargo build --release -p wind-cli`):
//   node bench/recipe_check.mjs web/src/wasm target/release/wind samples/torre_exemplo.csv
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [wasmDirectory, binary, samplePath] = process.argv.slice(2);
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });
const { CORE_VERSION, RECIPE_FORMAT } = await import(pathToFileURL(join(wasmDirectory, '..', 'api', 'generated', 'version.ts')).href).catch(() => ({}));

const SPEED = 'Vel 100m Média';
const SPEED_80 = 'Vel 80m Média';
const DIRECTION = 'Dir 98m';
const HOUR_MS = 3_600_000;
const send = (request) => JSON.stringify(request);

const bytes = readFileSync(samplePath);
const session = new Session();
const imported = JSON.parse(session.importCsv(bytes, send({ name: basename(samplePath) })));
const dataset = imported.dataset.id;
const { startMs } = imported.dataset;
// State the recipe must carry: a flag and a value edit.
session.flag(send({ dataset, columns: [SPEED], flag: 0, fromMs: startMs, toMs: startMs + HOUR_MS }));
session.applyOp(send({ dataset, op: { op: 'scale_offset', columns: [SPEED_80], fromMs: startMs, toMs: startMs + 24 * HOUR_MS, scale: 1.02, offset: 0 } }));
const restore = JSON.parse(session.projectState(send({ dataset })));
const sha256 = createHash('sha256').update(bytes).digest('hex');

const analyses = [
  ['coverage', { dataset }],
  ['histogram', { dataset, column: SPEED, binWidth: 1, weibull: 'max_likelihood', confidence: true }],
  ['rose', { dataset, direction: DIRECTION, speed: SPEED, sectors: 16, calmThreshold: 2 }],
  ['diurnal', { dataset, column: SPEED }],
  ['monthly', { dataset, column: SPEED }],
  ['shear', { dataset, columns: [SPEED, SPEED_80], direction: DIRECTION, sectors: 12 }],
  ['turbulence', { dataset, speeds: [SPEED, SPEED_80], versus: { kind: 'speed' } }],
  ['extremes', { dataset, column: SPEED, method: { kind: 'independent_storms' }, confidence: true }],
  ['table', { dataset, column: SPEED }],
  ['scatter', { dataset, x: SPEED_80, y: SPEED }],
];
const folder = mkdtempSync(join(tmpdir(), 'recipes-'));
try {
  for (const [analysis, request] of analyses) {
    const result = JSON.parse(session.analyze(send({ dataset, analysis, request })));
    const recipe = {
      format: RECIPE_FORMAT ?? 1,
      coreVersion: CORE_VERSION ?? '0.1.0',
      source: { name: basename(samplePath), bytes: bytes.length, sha256 },
      config: {},
      restore,
      analysis,
      request,
      result,
    };
    const file = join(folder, `${analysis}.json`);
    writeFileSync(file, JSON.stringify(recipe));
    const report = JSON.parse(execFileSync(binary, ['replay', file, samplePath], { encoding: 'utf8' }));
    assert.ok(report.sourceMatches, `${analysis}: same file`);
    assert.ok(report.resultMatches, `${analysis}: ${report.differenceCount} differences, ${report.differences.join(' ')}`);
  }
} finally {
  rmSync(folder, { recursive: true, force: true });
}
console.log(`recipes: ${analyses.length} analyses made in WASM replay natively, number by number`);

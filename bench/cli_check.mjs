// The project format across languages: web/src/project.ts writes a project,
// the `wind` command line (Rust reader) opens it, and what it prints must be
// exactly what the browser's WASM session gives for the same sources,
// configurations and flags. A damaged copy must fail with an ErrorDto.
// Usage (after `npm run wasm` in web/ and `cargo build --release -p wind-cli`):
//   node bench/cli_check.mjs web target/release/wind
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const webDirectory = resolve(process.argv[2]);
const windBinary = resolve(process.argv[3]);
const esbuild = createRequire(join(webDirectory, 'package.json'))('esbuild');
const bundle = esbuild.buildSync({ entryPoints: [join(webDirectory, 'src/project.ts')], bundle: true, format: 'esm', write: false });
const project = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString('base64')}`);

const wasmDirectory = join(webDirectory, 'src/wasm');
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const TEN_MINUTES_MS = 600_000;
const DAY_STEPS = 144;
const FIRST_MS = Date.UTC(2014, 0, 1);
const CORRECTED_HEIGHT_M = 42;
const FLAGGED_STEPS = 100;
/** Speed cycles of the two sample towers, so their files differ. */
const PERIOD_A = 20;
const PERIOD_B = 13;
const INVALID_FLAG = 0;
/** Tower B's speeds at or above this are flagged by a rule, with evidence. */
const RULE_MAX_SPEED = PERIOD_B - 1;
const SHORTEST_STRETCH_STEPS = 1;
const EXIT_FAILURE = 1;

/** A one-day CSV whose speeds cycle with `period`. */
function csv(period) {
  const rows = ['t,ws,dir'];
  for (let step = 0; step < DAY_STEPS; step++) {
    const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
    rows.push(`${stamp},${step % period},${(step * 7) % 360}`);
  }
  return new TextEncoder().encode(rows.join('\n'));
}

// The browser: import both, flag the first by hand and the second by a rule,
// save a project with project.ts, as the UI does (flags, segments and evidence
// from projectState).
const session = new Session();
const send = (request) => JSON.stringify(request);
const config = { columns: [{ fileColumn: 1, heightM: CORRECTED_HEIGHT_M }] };
const sources = [new File([csv(PERIOD_A)], 'torre_a.csv'), new File([csv(PERIOD_B)], 'torre_b.csv')];
const imported = [];
for (const file of sources) {
  imported.push(JSON.parse(session.importCsv(new Uint8Array(await file.arrayBuffer()), send({ name: file.name, config }))));
}
const first = imported[0].dataset;
const stretch = { dataset: first.id, columns: ['ws'], fromMs: first.startMs, toMs: first.startMs + (FLAGGED_STEPS - 1) * TEN_MINUTES_MS };
const edited = JSON.parse(session.flag(send({ ...stretch, flag: INVALID_FLAG })));
const rule = {
  criterion: { kind: 'outside_range', min: null, max: RULE_MAX_SPEED },
  columns: ['ws'],
  flag: INVALID_FLAG,
  minSteps: SHORTEST_STRETCH_STEPS,
};
JSON.parse(session.flagByRules(send({ dataset: imported[1].dataset.id, rules: [rule] })));
const saved = sources.map((file, index) => ({
  file,
  config,
  restore: JSON.parse(session.projectState(send({ dataset: imported[index].dataset.id }))),
}));
assert.ok(saved[1].restore.evidence.length > 0, 'the rule left evidence to save');
const blob = project.writeProject(saved);

// What a fresh browser session shows after opening that project.
const reopened = new Session();
const expected = [];
for (const { file, config: savedConfig, restore } of await project.readProject(blob)) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  expected.push(JSON.parse(reopened.importCsv(bytes, send({ name: file.name, config: savedConfig, restore }))));
}
const evidenceAfter = JSON.parse(reopened.projectState(send({ dataset: expected[1].dataset.id }))).evidence;
assert.deepEqual(evidenceAfter, saved[1].restore.evidence, 'the browser restores the rule evidence');

const scratch = mkdtempSync(join(tmpdir(), 'wind-cli-check-'));
try {
  const projectPath = join(scratch, 'projeto.wproj');
  writeFileSync(projectPath, new Uint8Array(await blob.arrayBuffer()));
  const run = spawnSync(windBinary, ['project', projectPath], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  assert.deepEqual(JSON.parse(run.stdout), expected, 'the command line opens the project exactly as the browser does');
  assert.deepEqual(expected[0].dataset.segments, edited.segments);

  const damagedPath = join(scratch, 'danificado.wproj');
  writeFileSync(damagedPath, new Uint8Array(await blob.slice(0, blob.size - 1).arrayBuffer()));
  const damaged = spawnSync(windBinary, ['project', damagedPath], { encoding: 'utf8' });
  assert.equal(damaged.status, EXIT_FAILURE);
  assert.equal(damaged.stdout, '');
  assert.equal(JSON.parse(damaged.stderr).code, 'not_a_project');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

console.log('project across languages: ok');

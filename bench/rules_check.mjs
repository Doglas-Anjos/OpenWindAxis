// Flag rules through the real WASM core: a run with its evidence, a re-run
// that records nothing, undo and redo, unflagging that trims evidence, the
// evidence and the rules saved in a project file (web/src/project.ts) and
// restored, and refused input.
// Usage (after `npm run wasm` in web/): node bench/rules_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });
const webDirectory = resolve(wasmDirectory, '..', '..');
const esbuild = createRequire(join(webDirectory, 'package.json'))('esbuild');
const bundle = esbuild.buildSync({ entryPoints: [join(webDirectory, 'src/project.ts')], bundle: true, format: 'esm', write: false });
const project = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString('base64')}`);

const TEN_MINUTES_MS = 600_000;
const DAY_STEPS = 288;
const FIRST_MS = Date.UTC(2014, 0, 1);
/** Speeds cycle 5..11 m/s; temperatures 20..22 °C. */
const BASE_SPEED = 5;
const SPEED_PERIOD = 7;
const BASE_TEMPERATURE = 20;
const TEMPERATURE_PERIOD = 3;
/** A logger error code at one step, and a frozen anemometer. */
const SPIKE_STEP = 40;
const SPIKE_SPEED = 99;
const FLAT_START = 100;
const FLAT_STEPS = 12;
const FLAT_SPEED = 4;
/** The rules: speeds and temperatures within [0, 50); a flat of an hour or more. */
const RANGE_MIN = 0;
const RANGE_MAX = 50;
const STUCK_MIN_STEPS = 6;
const INVALID_FLAG = 0;
const ICING_FLAG = 2;
/** Steps unflagged in the middle of the flat. */
const UNFLAGGED_FROM = FLAT_START + 4;
const UNFLAGGED_STEPS = 3;

const rows = ['t,ws,temp'];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  const flat = step >= FLAT_START && step < FLAT_START + FLAT_STEPS;
  const speed = step === SPIKE_STEP ? SPIKE_SPEED : flat ? FLAT_SPEED : BASE_SPEED + (step % SPEED_PERIOD);
  rows.push(`${stamp},${speed},${BASE_TEMPERATURE + (step % TEMPERATURE_PERIOD)}`);
}
const source = new TextEncoder().encode(rows.join('\n'));
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(source, send({ name: 'torre' })));
const dataset = imported.dataset.id;
const validBefore = imported.summaries[0].valid;

const rules = [
  { criterion: { kind: 'outside_range', min: RANGE_MIN, max: RANGE_MAX }, columns: ['ws', 'temp'], flag: INVALID_FLAG, minSteps: 1 },
  { criterion: { kind: 'stuck', tolerance: 0 }, columns: ['ws'], flag: ICING_FLAG, minSteps: STUCK_MIN_STEPS },
];
const runRules = (rulesToRun = rules) => JSON.parse(session.flagByRules(send({ dataset, rules: rulesToRun })));

// A run: what each rule found, with the evidence of every stretch.
let answer = runRules();
const tallies = answer.tallies.map(({ rule, column, stretches, steps }) => [rule, column, stretches, steps]);
assert.deepEqual(tallies, [
  [0, 'ws', 1, 1],
  [0, 'temp', 0, 0],
  [1, 'ws', 1, FLAT_STEPS],
]);
const [spike, flat] = answer.found;
assert.deepEqual([spike.column, spike.flag, spike.startStep, spike.endStep, spike.statistic], ['ws', INVALID_FLAG, SPIKE_STEP, SPIKE_STEP + 1, SPIKE_SPEED]);
assert.deepEqual(spike.criterion, { kind: 'outside_range', min: RANGE_MIN, max: RANGE_MAX });
assert.deepEqual([flat.flag, flat.startStep, flat.endStep, flat.statistic, flat.statisticStep], [ICING_FLAG, FLAT_START, FLAT_START + FLAT_STEPS, FLAT_STEPS, FLAT_START]);
assert.equal(answer.state.summaries[0].valid, validBefore - 1 - FLAT_STEPS, 'summaries see the new flags');
assert.deepEqual([answer.state.canUndo, answer.state.canRedo], [true, false]);

// The flagged steps are now excluded, so a re-run finds nothing and records nothing.
answer = runRules();
assert.equal(answer.found.length, 0);
let state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.segments.length, 0, 'one undo takes the first run back');
assert.equal(state.canUndo, false, 'the empty re-run was not recorded');
assert.equal(JSON.parse(session.projectState(send({ dataset }))).evidence.length, 0, 'undo takes the evidence back too');
state = JSON.parse(session.redo(send({ dataset })));
assert.equal(state.segments.length, 2);
assert.equal(state.summaries[0].valid, validBefore - 1 - FLAT_STEPS);

// Unflagging part of the flat trims its evidence like its segment.
const startMs = imported.dataset.startMs;
const window = { fromMs: startMs + UNFLAGGED_FROM * TEN_MINUTES_MS, toMs: startMs + (UNFLAGGED_FROM + UNFLAGGED_STEPS - 1) * TEN_MINUTES_MS };
session.unflag(send({ dataset, columns: ['ws'], ...window }));
const trimmed = JSON.parse(session.projectState(send({ dataset }))).evidence.filter((record) => record.flag === ICING_FLAG);
assert.deepEqual(
  trimmed.map((record) => [record.startStep, record.endStep]),
  [
    [FLAT_START, UNFLAGGED_FROM],
    [UNFLAGGED_FROM + UNFLAGGED_STEPS, FLAT_START + FLAT_STEPS],
  ],
);
session.undo(send({ dataset }));

// Saved in a project file with the rules, read back and restored on a fresh
// import: the same evidence and the same rules.
const saved = JSON.parse(session.projectState(send({ dataset })));
assert.equal(saved.evidence.length, 2);
const file = new File([source], 'torre.csv');
const [read] = await project.readProject(project.writeProject([{ file, config: { columns: [] }, restore: saved, rules }]));
assert.deepEqual(read.rules, rules);
const reopened = JSON.parse(session.importCsv(new Uint8Array(await read.file.arrayBuffer()), send({ name: 'torre', restore: read.restore })));
assert.deepEqual(JSON.parse(session.projectState(send({ dataset: reopened.dataset.id }))), saved);
const damagedRule = { ...rules[0], criterion: { kind: 'qualquer' } };
await assert.rejects(project.readProject(project.writeProject([{ file, config: { columns: [] }, restore: saved, rules: [damagedRule] }])), /não é um projeto/);
const withoutEvidence = { flags: saved.flags, segments: saved.segments };
const older = JSON.parse(session.importCsv(source, send({ name: 'torre', restore: withoutEvidence })));
assert.equal(older.dataset.segments.length, 2, 'a project saved before flag rules still opens');

// Refused input, with the session untouched.
const notFlagged = { ...saved, evidence: [{ ...saved.evidence[0], startStep: 0, endStep: 1, statisticStep: 0 }] };
assert.throws(() => session.importCsv(source, send({ name: 'torre', restore: notFlagged })), /evidence_not_flagged/);
const refused = (rule, code) => assert.throws(() => runRules([{ ...rules[0], ...rule }]), code);
refused({ criterion: { kind: 'outside_range', min: RANGE_MAX, max: RANGE_MIN } }, /rule_range_invalid/);
refused({ criterion: { kind: 'stuck', tolerance: 0 }, minSteps: 1 }, /rule_min_steps_too_small/);
refused({ criterion: { kind: 'change_above', limit: -1 } }, /rule_threshold_invalid/);
refused({ columns: ['coluna_que_nao_existe'] }, /unknown_column/);
refused({ criterion: { kind: 'stuck', tolerance: 0, limit: 1 } }, /bad_request/);
assert.equal(JSON.parse(session.undo(send({ dataset }))).segments.length, 0, 'refused runs recorded nothing');

console.log('flag rules: ok');

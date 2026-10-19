// Value edits through the real WASM core: scale and offset (derived power
// density follows), undo, delete, a time shift of the whole record (per-day
// summaries move with it), and the edits surviving a project round trip and
// a re-import.
// Usage (after `npm run wasm` in web/): node bench/edits_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const TEN_MINUTES_MS = 600_000;
const HOUR_MS = 3_600_000;
const HOURS_PER_DAY = 24;
const DAY_STEPS = 288;
const SPEED_PERIOD = 20;
/** Speeds start at this, so every step has a defined TI and power density. */
const LOWEST_SPEED = 4;
const DELETED_STEPS = 10;
/** An id no dataset of the session has. */
const MISSING_DATASET = 999;
const SCALE = 2;
/** Power density goes with the cube of the speed. */
const POWER_DENSITY_FACTOR = SCALE ** 3;
const RELATIVE_TOLERANCE = 1e-9;
const FIRST_MS = Date.UTC(2014, 0, 1);
const SPEED = 'Vel 80m Média';
const DIRECTION = 'Dir 78m';
const POWER_DENSITY = `${SPEED} WPD`;

const rows = [`Data;${SPEED};Vel 80m DP;${DIRECTION}`];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  rows.push(`${stamp};${LOWEST_SPEED + (step % SPEED_PERIOD)};0.5;${(step * 7) % 360}`);
}
const source = new TextEncoder().encode(rows.join('\n'));
const session = new Session();
const send = (request) => JSON.stringify(request);
const close = (found, want) => Math.abs(found - want) <= RELATIVE_TOLERANCE * Math.abs(want);
const summaryOf = (summaries, column) => summaries.find((summary) => summary.column === column);

const imported = JSON.parse(session.importCsv(source, send({ name: 'torre' })));
const dataset = imported.dataset.id;
const { startMs } = imported.dataset;
const lastMs = startMs + (DAY_STEPS - 1) * TEN_MINUTES_MS;
const before = (column) => summaryOf(imported.summaries, column);
const apply = (op) => JSON.parse(session.applyOp(send({ dataset, op })));

// Scale: the speed doubles, its power density grows eightfold.
let state = apply({ op: 'scale_offset', columns: [SPEED], fromMs: startMs, toMs: lastMs, scale: SCALE, offset: 0 });
assert.equal(state.valuesChanged, true);
assert.ok(close(summaryOf(state.summaries, SPEED).mean, SCALE * before(SPEED).mean));
assert.ok(close(summaryOf(state.summaries, POWER_DENSITY).mean, POWER_DENSITY_FACTOR * before(POWER_DENSITY).mean), 'derived columns follow');
const firstPoint = session.series(send({ dataset, column: SPEED, fromMs: startMs, toMs: startMs, buckets: 1 }));
assert.equal(firstPoint[1], SCALE * LOWEST_SPEED, 'the chart reads the new values');

state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.valuesChanged, true, 'undoing an edit changes values too');
assert.ok(close(summaryOf(state.summaries, SPEED).mean, before(SPEED).mean));
state = JSON.parse(session.flag(send({ dataset, columns: [SPEED], flag: 0, fromMs: startMs, toMs: startMs })));
assert.equal(state.valuesChanged, false, 'a flag edit changes no values');

// Delete, then shift the whole record by an hour.
state = apply({ op: 'delete_data', columns: [DIRECTION], fromMs: startMs, toMs: startMs + (DELETED_STEPS - 1) * TEN_MINUTES_MS });
assert.equal(summaryOf(state.summaries, DIRECTION).valid, before(DIRECTION).valid - DELETED_STEPS);
const diurnalBefore = JSON.parse(session.diurnal(send({ dataset, column: SPEED }))).all;
state = apply({ op: 'shift_start', byMs: HOUR_MS });
assert.equal(state.startMs, startMs + HOUR_MS);
const diurnalAfter = JSON.parse(session.diurnal(send({ dataset, column: SPEED }))).all;
for (let hour = 0; hour < HOURS_PER_DAY; hour++) {
  assert.ok(close(diurnalAfter[hour], diurnalBefore[(hour + HOURS_PER_DAY - 1) % HOURS_PER_DAY]), `hour ${hour} holds the hour before`);
}

// The edits in effect travel with the project and with a re-import.
const saved = JSON.parse(session.projectState(send({ dataset })));
assert.deepEqual(saved.edits.map((edit) => edit.edit), ['delete', 'shift_start'], 'the undone scale is not saved');
const reopened = JSON.parse(session.importCsv(source, send({ name: 'torre', restore: saved })));
assert.equal(reopened.dataset.startMs, startMs + HOUR_MS);
assert.equal(summaryOf(reopened.summaries, DIRECTION).valid, before(DIRECTION).valid - DELETED_STEPS);
const reapplied = JSON.parse(session.importCsv(source, send({ name: 'torre', replace: dataset })));
assert.equal(reapplied.dataset.startMs, startMs + HOUR_MS);
assert.equal(summaryOf(reapplied.summaries, DIRECTION).valid, before(DIRECTION).valid - DELETED_STEPS);
assert.equal(reapplied.dataset.segments.length, 1, 'the flag stays on its step');

// A workflow naming a dataset that does not exist edits none; one named twice runs once.
const doubling = { format: 1, ops: [{ op: 'scale_offset', columns: [SPEED], fromMs: startMs, toMs: startMs + HOUR_MS, scale: SCALE, offset: 0 }], analyses: [] };
const stateBefore = session.projectState(send({ dataset }));
assert.throws(() => session.runWorkflow(send({ datasets: [dataset, MISSING_DATASET], workflow: doubling })), /no_such_dataset/);
assert.equal(session.projectState(send({ dataset })), stateBefore, 'the known dataset was not edited');
const [once] = JSON.parse(session.runWorkflow(send({ datasets: [dataset, dataset], workflow: doubling })));
assert.equal(JSON.parse(session.projectState(send({ dataset }))).edits.filter((edit) => edit.edit === 'scale_offset').length, 1, 'scaled once');
assert.equal(once.opsApplied, 1);

console.log('value edits: ok');

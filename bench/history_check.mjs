// Undo and redo through the real WASM core: flag, re-flag (records nothing),
// undo, an empty undo, redo, and a re-import that starts a new history.
// Usage (after `npm run wasm` in web/): node bench/history_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const TEN_MINUTES_MS = 600_000;
const DAY_STEPS = 288;
const SPEED_PERIOD = 20;
const FLAGGED_STEPS = 100;
const FIRST_MS = Date.UTC(2014, 0, 1);

const rows = ['t,ws'];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  rows.push(`${stamp},${step % SPEED_PERIOD}`);
}
const source = new TextEncoder().encode(rows.join('\n'));
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(source, send({ name: 'torre' })));
const dataset = imported.dataset.id;
const validBefore = imported.summaries[0].valid;
const startMs = imported.dataset.startMs;
const stretch = { dataset, columns: ['ws'], fromMs: startMs, toMs: startMs + (FLAGGED_STEPS - 1) * TEN_MINUTES_MS };

let state = JSON.parse(session.flag(send({ ...stretch, flag: 0 })));
assert.equal(state.segments.length, 1);
assert.deepEqual([state.canUndo, state.canRedo], [true, false]);
assert.equal(state.summaries[0].valid, validBefore - FLAGGED_STEPS);

session.flag(send({ ...stretch, flag: 0 }));
state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.segments.length, 0, 're-flagging the same stretch recorded nothing');
assert.deepEqual([state.canUndo, state.canRedo], [false, true]);
assert.equal(state.summaries[0].valid, validBefore);

state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.canRedo, true, 'an undo with nothing to undo changes nothing');

state = JSON.parse(session.redo(send({ dataset })));
assert.equal(state.summaries[0].valid, validBefore - FLAGGED_STEPS);
assert.equal(state.canRedo, false);

const reimported = JSON.parse(session.importCsv(source, send({ name: 'torre', replace: dataset })));
assert.equal(reimported.dataset.segments.length, 1, 'flags carried over');
state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.canUndo, false);
assert.equal(state.segments.length, 1, 'a re-import starts a new history');

console.log('undo/redo: ok');

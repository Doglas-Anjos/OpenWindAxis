// Flag statistics and removal by flag through the real WASM core: steps and
// shares per flag and column, segments with their rule evidence, removal from
// one column and from all of them, undo, and refused input.
// Usage (after `npm run wasm` in web/): node bench/flag_stats_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const TEN_MINUTES_MS = 600_000;
const DAY_STEPS = 288;
const FIRST_MS = Date.UTC(2014, 0, 1);
/** Speeds cycle 5..11 m/s with one logger error code; temperatures 20..22 °C. */
const BASE_SPEED = 5;
const SPEED_PERIOD = 7;
const BASE_TEMPERATURE = 20;
const TEMPERATURE_PERIOD = 3;
const SPIKE_STEP = 40;
const SPIKE_SPEED = 99;
const RANGE_MAX = 50;
/** Stretches flagged by hand: speed and temperature, invalid; temperature, icing. */
const MANUAL_SPEED_STEPS = 10;
const MANUAL_TEMPERATURE_STEPS = 6;
const ICED_TEMPERATURE_STEPS = 4;
const INVALID_FLAG = 0;
const TOWER_SHADOW_FLAG = 1;
const ICING_FLAG = 2;
const SPEED = 0;
const TEMPERATURE = 1;

const rows = ['t,ws,temp'];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  const speed = step === SPIKE_STEP ? SPIKE_SPEED : BASE_SPEED + (step % SPEED_PERIOD);
  rows.push(`${stamp},${speed},${BASE_TEMPERATURE + (step % TEMPERATURE_PERIOD)}`);
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(new TextEncoder().encode(rows.join('\n')), send({ name: 'torre' })));
const dataset = imported.dataset.id;
const startMs = imported.dataset.startMs;
const stretch = (steps) => ({ fromMs: startMs, toMs: startMs + (steps - 1) * TEN_MINUTES_MS });
const statistics = () => JSON.parse(session.flagStatistics(send({ dataset })));
/** Counts come per column, derived ones (the speed's WPD) last and always 0: they carry no flags. */
const DERIVED_COLUMNS = imported.dataset.columns.filter((column) => column.derivedFrom).length;
const perColumn = (...measured) => [...measured, ...Array(DERIVED_COLUMNS).fill(0)];

session.flag(send({ dataset, columns: ['ws'], flag: INVALID_FLAG, ...stretch(MANUAL_SPEED_STEPS) }));
session.flag(send({ dataset, columns: ['temp'], flag: INVALID_FLAG, ...stretch(MANUAL_TEMPERATURE_STEPS) }));
session.flag(send({ dataset, columns: ['temp'], flag: ICING_FLAG, ...stretch(ICED_TEMPERATURE_STEPS) }));
const rule = { criterion: { kind: 'outside_range', min: null, max: RANGE_MAX }, columns: ['ws'], flag: INVALID_FLAG, minSteps: 1 };
session.flagByRules(send({ dataset, rules: [rule] }));

// Steps and shares per flag and column; segments with the rule's evidence.
let counted = statistics();
assert.equal(counted.stepCount, DAY_STEPS);
assert.deepEqual(counted.steps[INVALID_FLAG], perColumn(MANUAL_SPEED_STEPS + 1, MANUAL_TEMPERATURE_STEPS));
assert.deepEqual(counted.steps[TOWER_SHADOW_FLAG], perColumn(0, 0));
assert.deepEqual(counted.steps[ICING_FLAG], perColumn(0, ICED_TEMPERATURE_STEPS));
assert.equal(counted.share[INVALID_FLAG][TEMPERATURE], MANUAL_TEMPERATURE_STEPS / DAY_STEPS);
const described = counted.segments.map(({ column, flag, startStep, endStep, evidence }) => [column, flag, startStep, endStep, evidence.length]);
assert.deepEqual(described, [
  ['ws', INVALID_FLAG, 0, MANUAL_SPEED_STEPS, 0],
  ['ws', INVALID_FLAG, SPIKE_STEP, SPIKE_STEP + 1, 1],
  ['temp', INVALID_FLAG, 0, MANUAL_TEMPERATURE_STEPS, 0],
  ['temp', ICING_FLAG, 0, ICED_TEMPERATURE_STEPS, 0],
]);
assert.equal(counted.segments[1].evidence[0].statistic, SPIKE_SPEED);

// Removing a flag from one column leaves the other column and the other flags.
let state = JSON.parse(session.removeFlag(send({ dataset, flag: INVALID_FLAG, columns: ['ws'] })));
assert.equal(state.canUndo, true);
assert.equal(state.summaries[SPEED].valid, DAY_STEPS, 'the speed is whole again');
counted = statistics();
assert.deepEqual(counted.steps[INVALID_FLAG], perColumn(0, MANUAL_TEMPERATURE_STEPS));
assert.equal(JSON.parse(session.projectState(send({ dataset }))).evidence.length, 0, 'the evidence went with its flag');

// From every column; a flag nobody carries records nothing.
session.removeFlag(send({ dataset, flag: INVALID_FLAG }));
assert.deepEqual(statistics().steps[INVALID_FLAG], perColumn(0, 0));
assert.deepEqual(statistics().steps[ICING_FLAG], perColumn(0, ICED_TEMPERATURE_STEPS));
session.removeFlag(send({ dataset, flag: TOWER_SHADOW_FLAG }));
session.undo(send({ dataset }));
state = JSON.parse(session.undo(send({ dataset })));
assert.equal(state.summaries[SPEED].valid, DAY_STEPS - MANUAL_SPEED_STEPS - 1, 'two undos bring both removals back');
assert.equal(statistics().segments[1].evidence.length, 1, 'and the evidence with them');

// Refused input.
assert.throws(() => session.removeFlag(send({ dataset, flag: 9 })), /no_such_flag/);
assert.throws(() => session.removeFlag(send({ dataset, flag: INVALID_FLAG, columns: ['coluna_que_nao_existe'] })), /unknown_column/);
assert.throws(() => session.removeFlag(send({ dataset, flag: INVALID_FLAG, column: 'ws' })), /bad_request/);
assert.throws(() => session.flagStatistics(send({ dataset: dataset + 1 })), /no_such_dataset/);

console.log('flag statistics: ok');

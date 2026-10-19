// "Filtrar por" through the real WASM core: every criterion narrows what the
// analyses read exactly as a count made here, step by step, says it should.
// Usage (after `npm run wasm` in web/): node bench/filter_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const HOUR_MS = 3_600_000;
const DAY_MS = 24 * HOUR_MS;
/** Hourly steps over one year from 2014-01-01T00:00Z. */
const STEPS = 8_760;
const FIRST_MS = Date.UTC(2014, 0, 1);
const SPEED_PERIOD = 17;
const DIRECTION_STEP_DEGREES = 37;
const DEGREES_PER_TURN = 360;
const INVALID_FLAG = 0;
const FLAGGED_STEPS = 500;
const BIN_WIDTH = 1;
/** June to August, 1-based. */
const JUNE = 6;
const AUGUST = 8;
/** 20:00 to 05:00, across midnight. */
const EVENING = 20;
const DAWN = 5;
const SECTORS = 12;
const SECTOR = 3;
const MIN_SPEED = 4;
const MAX_SPEED = 9;
/** 2014-03-10 to 2014-03-19, both days included. */
const RANGE_FIRST_DAY = Date.UTC(2014, 2, 10);
const RANGE_DAYS = 10;

const speedAt = (step) => step % SPEED_PERIOD;
const directionAt = (step) => (step * DIRECTION_STEP_DEGREES) % DEGREES_PER_TURN;
const instantAt = (step) => new Date(FIRST_MS + step * HOUR_MS);
const rows = ['t,ws,dir'];
for (let step = 0; step < STEPS; step++) {
  rows.push(`${instantAt(step).toISOString().slice(0, 16).replace('T', ' ')},${speedAt(step)},${directionAt(step)}`);
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(new TextEncoder().encode(rows.join('\n')), send({ name: 'torre' })));
const dataset = imported.dataset.id;
session.flag(send({ dataset, columns: ['ws'], flag: INVALID_FLAG, fromMs: FIRST_MS, toMs: FIRST_MS + (FLAGGED_STEPS - 1) * HOUR_MS }));

/** Samples the speed histogram reads under `filter`. */
const samples = (filter) => JSON.parse(session.histogram(send({ dataset, column: 'ws', binWidth: BIN_WIDTH, filter }))).samples;
/** Steps kept by `keep(step)`, counted here. */
const counted = (keep) => Array.from({ length: STEPS }, (_, step) => step).filter(keep).length;
const unflagged = (step) => step >= FLAGGED_STEPS;
const sectorOf = (direction) => Math.floor(((direction % DEGREES_PER_TURN) + DEGREES_PER_TURN / SECTORS / 2) / (DEGREES_PER_TURN / SECTORS)) % SECTORS;

assert.equal(samples(undefined), counted(unflagged), 'default flag filtering');
assert.equal(samples({}), counted(unflagged), 'an empty filter is default filtering');

const month = (step) => instantAt(step).getUTCMonth() + 1;
assert.equal(samples({ months: { first: JUNE, last: AUGUST } }), counted((step) => unflagged(step) && month(step) >= JUNE && month(step) <= AUGUST));

const hour = (step) => instantAt(step).getUTCHours();
assert.equal(samples({ hours: { start: EVENING, end: DAWN } }), counted((step) => unflagged(step) && (hour(step) >= EVENING || hour(step) < DAWN)));

const sector = { direction: 'dir', sectors: SECTORS, sector: SECTOR };
assert.equal(samples({ sector }), counted((step) => unflagged(step) && sectorOf(directionAt(step)) === SECTOR));

const value = { column: 'ws', min: MIN_SPEED, max: MAX_SPEED };
assert.equal(samples({ value }), counted((step) => unflagged(step) && speedAt(step) >= MIN_SPEED && speedAt(step) < MAX_SPEED));
assert.equal(samples({ value: { column: 'ws', min: MIN_SPEED, max: null } }), counted((step) => unflagged(step) && speedAt(step) >= MIN_SPEED));

const dateRange = { fromMs: RANGE_FIRST_DAY, toMs: RANGE_FIRST_DAY + RANGE_DAYS * DAY_MS };
const inRange = (step) => FIRST_MS + step * HOUR_MS >= dateRange.fromMs && FIRST_MS + step * HOUR_MS < dateRange.toMs;
assert.equal(samples({ dateRange }), counted(inRange));

// Flags: only flagged data, or every flag ignored.
assert.equal(samples({ flags: { keep: [true, true, true], keepUnflagged: false } }), FLAGGED_STEPS);
assert.equal(samples({ flags: { keep: [true, true, true], keepUnflagged: true } }), STEPS);

// Criteria combine with AND, and reach every analysis.
const summer = { months: { first: JUNE, last: AUGUST }, value };
const summerSteps = counted((step) => unflagged(step) && month(step) >= JUNE && month(step) <= AUGUST && speedAt(step) >= MIN_SPEED && speedAt(step) < MAX_SPEED);
assert.equal(samples(summer), summerSteps);
const rose = JSON.parse(session.windRose(send({ dataset, direction: 'dir', speed: 'ws', sectors: SECTORS, filter: summer })));
assert.equal(rose.samples, summerSteps, 'the rose reads the same steps');
const coverage = JSON.parse(session.coverage(send({ dataset, filter: { months: { first: JUNE, last: AUGUST } } })));
const summerShare = counted((step) => unflagged(step) && month(step) >= JUNE && month(step) <= AUGUST) / STEPS;
assert.ok(Math.abs(coverage.total[0] - summerShare) < 1e-12, `coverage ${coverage.total[0]} vs ${summerShare}`);
const diurnal = JSON.parse(session.diurnal(send({ dataset, column: 'ws', filter: { hours: { start: EVENING, end: DAWN } } })));
assert.equal(diurnal.all[12], null, 'no noon steps after a night-only filter');

// Bad criteria are refused with a code.
assert.throws(() => samples({ months: { first: 1, last: 13 } }), /filter_criterion_invalid/);
assert.throws(() => samples({ value: { column: 'nada', min: 1, max: 2 } }), /unknown_column/);
assert.throws(() => samples({ months: { first: 1, lsat: 2 } }), /bad_request/);

console.log('filter by: ok');

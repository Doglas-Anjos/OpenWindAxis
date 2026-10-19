// Method corrections through the real WASM core: shear by AP-MoMM unless
// AP-Mean is asked for, calms in the wind rose, and the measured air density
// in the energy figures.
// Usage (after `npm run wasm` in web/): node bench/methods_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const HOUR_MS = 3_600_000;
const FIRST_MS = Date.UTC(2014, 0, 1);
/** Hourly over 13 months: January twice, so the simple mean leans to it. */
const STEPS = (365 + 31) * 24;
const SECTORS = 4;
const CALM_THRESHOLD = 1.5;
const SITE_ELEVATION_M = 900;
const RHO_STD = 1.225;
/** Speeds by month at 100 m and 40 m: a seasonal profile. */
const topSpeed = (month) => 5 + month;
const bottomSpeed = (month) => 4 + 0.25 * month;
/** A calm hour every this many steps. */
const CALM_EVERY = 10;
const CALM_SPEED = 1;

const rows = ['t,WS100,WS40,Dir 98m,Temp 2m'];
for (let step = 0; step < STEPS; step++) {
  const instant = new Date(FIRST_MS + step * HOUR_MS);
  const month = instant.getUTCMonth();
  const calm = step % CALM_EVERY === 0;
  rows.push(
    [instant.toISOString().slice(0, 16).replace('T', ' '), calm ? CALM_SPEED : topSpeed(month), bottomSpeed(month), (step * 37) % 360, 10 + (month % 6)].join(','),
  );
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const config = { elevationM: SITE_ELEVATION_M, columns: [] };
const imported = JSON.parse(session.importCsv(new TextEncoder().encode(rows.join('\n')), send({ name: 'torre', config })));
const dataset = imported.dataset.id;

// Shear: AP-MoMM by default, and the record's seasonal bias moves AP-Mean.
const shear = (aggregation) => JSON.parse(session.shear(send({ dataset, columns: ['WS100', 'WS40'], sectors: SECTORS, aggregation })));
const byDefault = shear(undefined);
assert.equal(byDefault.aggregation, 'ap_momm');
assert.deepEqual(byDefault, shear('ap_momm'));
const simple = shear('ap_mean');
assert.equal(simple.aggregation, 'ap_mean');
assert.notEqual(simple.alpha, byDefault.alpha, 'a seasonally biased record: the two methods differ');

// Rose: calms leave the sectors; directions and calms add up to 100 %.
const rose = (calmThreshold) =>
  JSON.parse(session.windRose(send({ dataset, direction: 'Dir 98m', speed: 'WS100', sectors: SECTORS, calmThreshold })));
const withCalms = rose(CALM_THRESHOLD);
const expectedCalm = Math.ceil(STEPS / CALM_EVERY) / STEPS;
assert.ok(Math.abs(withCalms.calm - expectedCalm) < 1e-12, `calm ${withCalms.calm} vs ${expectedCalm}`);
const total = withCalms.frequency.reduce((sum, share) => sum + share, 0) + withCalms.calm;
assert.ok(Math.abs(total - 1) < 1e-12, `sectors + calm = ${total}`);
assert.equal(rose(undefined).calm, 0, 'no threshold, no calms');
assert.throws(() => rose(-1), /calm_threshold_invalid/);

// Energy: the derived air density, not 1.225, once there is temperature and elevation.
const histogram = JSON.parse(session.histogram(send({ dataset, column: 'WS100', binWidth: 1 })));
assert.equal(histogram.densityMeasured, true);
assert.ok(histogram.airDensityKgM3 < RHO_STD, `ρ ${histogram.airDensityKgM3} at ${SITE_ELEVATION_M} m is below sea level's`);

console.log('method corrections: ok');

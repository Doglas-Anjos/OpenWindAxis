// Confidence intervals through the real WASM core: the Weibull k and A
// intervals when asked for, the shear α interval always, both deterministic.
// Usage (after `npm run wasm` in web/): node bench/confidence_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const HOUR_MS = 3_600_000;
const FIRST_MS = Date.UTC(2014, 0, 1);
const DAYS = 200;
const STEPS = DAYS * 24;
const SHAPE = 2;
const SCALE = 8;
const ALPHA = 0.2;
const TOP_HEIGHT_M = 100;
const BOTTOM_HEIGHT_M = 40;
const TWO_DECIMALS = 100;
const SECTORS = 4;

/** xorshift-style fixed noise in [0, 1). */
let state = 12345;
const random = () => {
  state ^= state << 13;
  state ^= state >>> 17;
  state ^= state << 5;
  return (state >>> 0) / 2 ** 32;
};
const round = (speed) => Math.round(speed * TWO_DECIMALS) / TWO_DECIMALS;
const rows = [`t,WS${TOP_HEIGHT_M},WS${BOTTOM_HEIGHT_M}`];
for (let step = 0; step < STEPS; step++) {
  const speed = SCALE * (-Math.log(1 - random())) ** (1 / SHAPE);
  const lower = speed * (BOTTOM_HEIGHT_M / TOP_HEIGHT_M) ** ALPHA * (0.9 + 0.2 * random());
  rows.push(`${new Date(FIRST_MS + step * HOUR_MS).toISOString().slice(0, 16).replace('T', ' ')},${round(speed)},${round(lower)}`);
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const dataset = JSON.parse(session.importCsv(new TextEncoder().encode(rows.join('\n')), send({ name: 'torre' }))).dataset.id;

const histogram = (confidence) =>
  JSON.parse(session.histogram(send({ dataset, column: `WS${TOP_HEIGHT_M}`, binWidth: 1, weibull: 'max_likelihood', confidence }))).weibull;
const plain = histogram(undefined);
assert.equal(plain.kInterval, null, 'no interval unless asked');
const withInterval = histogram(true);
const contains = (interval, value) => interval.low < value && value < interval.high;
assert.ok(contains(withInterval.kInterval, withInterval.k), JSON.stringify(withInterval.kInterval));
assert.ok(contains(withInterval.aInterval, withInterval.a));
assert.ok(contains(withInterval.kInterval, SHAPE) && contains(withInterval.aInterval, SCALE), 'the truth inside');
assert.deepEqual(histogram(true).kInterval, withInterval.kInterval, 'deterministic');

const shear = JSON.parse(session.shear(send({ dataset, columns: [`WS${TOP_HEIGHT_M}`, `WS${BOTTOM_HEIGHT_M}`], sectors: SECTORS })));
assert.ok(contains(shear.alphaInterval, shear.alpha), JSON.stringify(shear.alphaInterval));
assert.ok(contains(shear.alphaInterval, ALPHA), `true α ${ALPHA} in ${JSON.stringify(shear.alphaInterval)}`);

console.log('confidence intervals: ok');

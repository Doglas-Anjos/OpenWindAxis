// WAsP .tab export through the real WASM core: the layout of
// docs/formatos/wasp-tab.md, frequencies that sum exactly, agreement with the
// wind rose, flags respected, and a refused request without a site location.
// Usage (after `npm run wasm` in web/): node bench/wasp_tab_check.mjs web/src/wasm
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const wasmDirectory = process.argv[2];
const { initSync, Session } = await import(pathToFileURL(join(wasmDirectory, 'wind_wasm.js')).href);
initSync({ module: readFileSync(join(wasmDirectory, 'wind_wasm_bg.wasm')) });

const TEN_MINUTES_MS = 600_000;
const DAY_STEPS = 144;
const FIRST_MS = Date.UTC(2014, 0, 1);
/** Morning wind from the north, afternoon wind from the south. */
const NORTH_DEG = 0;
const SOUTH_DEG = 180;
const SPEED_PERIOD = 15;
/** 1 m/s classes; every speed a quarter into its class, so none sits on an edge. */
const CLASS_WIDTH_M_S = 1;
const INSIDE_CLASS_M_S = 0.25;
const SECTORS = 4;
const NORTH_SECTOR = 0;
const SOUTH_SECTOR = 2;
const LATITUDE_DEG = -5.1234;
const LONGITUDE_DEG = -36.4567;
const HEIGHT_M = 80;
const INVALID_FLAG = 0;
const LINE_END = '\r\n';
/** Line 4 sums to 100.00 %, each sector's classes to 1000.000 ‰ (in units of the last decimal). */
const PERCENT_UNITS = 10_000;
const PER_MILLE_UNITS = 1_000_000;
const HEADER_LINES = 4;
const PERCENT = 100;
/** Half a unit of line 4's last decimal. */
const PERCENT_ROUNDING = 0.005;

const rows = ['t,ws,dir'];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  const direction = step < DAY_STEPS / 2 ? NORTH_DEG : SOUTH_DEG;
  rows.push(`${stamp},${(step % SPEED_PERIOD) * CLASS_WIDTH_M_S + INSIDE_CLASS_M_S},${direction}`);
}
const session = new Session();
const send = (request) => JSON.stringify(request);
const imported = JSON.parse(session.importCsv(new TextEncoder().encode(rows.join('\n')), send({ name: 'torre' })));
const dataset = imported.dataset.id;
const request = {
  dataset,
  speed: 'ws',
  direction: 'dir',
  sectors: SECTORS,
  binWidthMS: CLASS_WIDTH_M_S,
  latitudeDeg: LATITUDE_DEG,
  longitudeDeg: LONGITUDE_DEG,
  heightM: HEIGHT_M,
};

/** Each number of a line in units of its last decimal ("50.00" → 5000). */
const units = (line) => line.trim().split(/\s+/).map((number) => Number(number.replace('.', '')));

function parse(text) {
  const lines = text.split(LINE_END).filter((line) => line !== '');
  const table = lines.slice(HEADER_LINES).map(units);
  return { lines, sectorUnits: units(lines[3]), table };
}

let tab = parse(session.exportWaspTab(send(request)));
assert.equal(tab.lines[0], 'torre · ws · dir');
assert.equal(tab.lines[1], `${LATITUDE_DEG} ${LONGITUDE_DEG} ${HEIGHT_M}`);
assert.equal(tab.lines[2], `${SECTORS} 1.00 0.00`);
assert.equal(tab.sectorUnits.reduce((sum, value) => sum + value, 0), PERCENT_UNITS);
assert.deepEqual(tab.sectorUnits, [PERCENT_UNITS / 2, 0, PERCENT_UNITS / 2, 0]);
for (const sector of [NORTH_SECTOR, SOUTH_SECTOR]) {
  const column = tab.table.reduce((sum, row) => sum + row[1 + sector], 0);
  assert.equal(column, PER_MILLE_UNITS, `sector ${sector} sums to 1000 ‰`);
}
assert.equal(tab.table.length, SPEED_PERIOD, 'one class per whole speed');

// The same steps as the wind rose.
const rose = JSON.parse(session.windRose(send({ dataset, direction: 'dir', speed: 'ws', sectors: SECTORS })));
rose.frequency.forEach((share, sector) =>
  assert.ok(Math.abs(share * PERCENT - Number(tab.lines[3].trim().split(/\s+/)[sector])) <= PERCENT_ROUNDING),
);

// Flags are respected: excluding the morning leaves only the south.
const morningEndMs = FIRST_MS + (DAY_STEPS / 2 - 1) * TEN_MINUTES_MS;
session.flag(send({ dataset, columns: ['ws'], flag: INVALID_FLAG, fromMs: FIRST_MS, toMs: morningEndMs }));
tab = parse(session.exportWaspTab(send(request)));
assert.deepEqual(tab.sectorUnits, [0, 0, PERCENT_UNITS, 0]);

// No location, no file: WAsP needs the latitude.
const { latitudeDeg, ...withoutLatitude } = request;
assert.throws(() => session.exportWaspTab(send(withoutLatitude)), /site_location_unknown/);
assert.throws(() => session.exportWaspTab(send({ ...request, sectors: 0 })), /sectors_out_of_range/);

console.log('WAsP .tab: ok');

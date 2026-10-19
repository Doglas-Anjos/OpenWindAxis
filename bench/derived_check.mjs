// Derived columns through the real WASM core: TI, WPD and air density appear
// after the file's columns, follow their formulas (checked here with an
// independent JS implementation), inherit their sources' flags, refuse flags
// of their own, are rebuilt on re-import and stay out of the CSV unless asked.
// Usage (after `npm run wasm` in web/): node bench/derived_check.mjs web/src/wasm
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
const SITE_ELEVATION_M = 300;
const TEMPERATURE_HEIGHT_M = 80;
const PRESSURE_HEIGHT_M = 2;
const INVALID_FLAG = 0;
/** Steps flagged on the SD column, then on the temperature column. */
const DEVIATION_FLAGGED = [0, 10];
const TEMPERATURE_FLAGGED = [20, 30];
/** A step without temperature: no density and no WPD there. */
const TEMPERATURE_GAP_STEP = 50;
/** Relative tolerance between the core and this file's formulas (libm vs Math.pow). */
const TOLERANCE = 1e-12;

// ISO 2533 standard atmosphere and the IEC 61400-12-1 moist-air density (at
// the 50 % assumed without a humidity sensor), written out here as an
// independent check of crates/wind-core/src/derived.rs.
const DRY_AIR_GAS_CONSTANT = 287.05287;
const WATER_VAPOUR_GAS_CONSTANT = 461.5;
const DEFAULT_HUMIDITY_SHARE = 0.5;
/** IEC 61400-12-1 vapour pressure, Pa: a·exp(b·T). */
const vapourPressure = (kelvin) => 2.05e-5 * Math.exp(0.0631846 * kelvin);
const moistAir = (pressurePa, kelvin) =>
  (pressurePa / DRY_AIR_GAS_CONSTANT -
    DEFAULT_HUMIDITY_SHARE * vapourPressure(kelvin) * (1 / DRY_AIR_GAS_CONSTANT - 1 / WATER_VAPOUR_GAS_CONSTANT)) /
  kelvin;
const SEA_LEVEL_TEMPERATURE_K = 288.15;
const SEA_LEVEL_PRESSURE_PA = 101_325;
const LAPSE_RATE_K_M = 0.0065;
const GRAVITY_M_S2 = 9.80665;
const CELSIUS_ZERO_K = 273.15;
const PASCALS_PER_KILOPASCAL = 1000;
const TI_MIN_SPEED_M_S = 0.01;
const standardPressure = (elevationM) =>
  SEA_LEVEL_PRESSURE_PA * (1 - (LAPSE_RATE_K_M * elevationM) / SEA_LEVEL_TEMPERATURE_K) ** (GRAVITY_M_S2 / (DRY_AIR_GAS_CONSTANT * LAPSE_RATE_K_M));

const IDS = { speed: 'WS80', deviation: 'WS80_SD', temperature: 'Temp 80m', pressure: 'Pres 2m', direction: 'Dir 78m' };
const DERIVED = { density: 'Densidade do ar', turbulence: 'WS80 TI', power: 'WS80 WPD' };

// One day: speeds 0–14 m/s, temperatures around 25 °C, pressure in kPa (the
// import must notice), a few gaps.
const rows = [`t,${Object.values(IDS).join(',')}`];
const inputs = [];
for (let step = 0; step < DAY_STEPS; step++) {
  const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
  const speed = step % 15;
  const deviation = (0.1 + (step % 7) * 0.05).toFixed(2);
  const temperature = step === TEMPERATURE_GAP_STEP ? '' : 20 + (step % 11);
  const pressure = (97 + (step % 5) * 0.1).toFixed(1);
  inputs.push({ speed, deviation: Number(deviation), temperature: temperature === '' ? NaN : temperature, pressure: Number(pressure) });
  rows.push(`${stamp},${speed},${deviation},${temperature},${pressure},${(step * 7) % 360}`);
}
const source = new TextEncoder().encode(rows.join('\n'));
const session = new Session();
const send = (request) => JSON.stringify(request);
const config = { elevationM: SITE_ELEVATION_M, columns: [] };
const imported = JSON.parse(session.importCsv(source, send({ name: 'torre', config })));
const dataset = imported.dataset.id;
const columns = imported.dataset.columns;
const byId = new Map(columns.map((column) => [column.id, column]));
const position = (id) => columns.findIndex((column) => column.id === id);

// Layout and metadata.
assert.deepEqual(columns.map((column) => column.id), [...Object.values(IDS), ...Object.values(DERIVED)]);
assert.equal(byId.get(IDS.pressure).units, 'kPa', 'pressure units told by magnitude');
assert.deepEqual(byId.get(DERIVED.density).derivedFrom, [IDS.temperature, IDS.pressure]);
assert.deepEqual(byId.get(DERIVED.turbulence).derivedFrom, [IDS.speed, IDS.deviation]);
assert.deepEqual(byId.get(DERIVED.power).derivedFrom, [IDS.speed, IDS.temperature, IDS.pressure]);
assert.equal(byId.get(IDS.speed).derivedFrom, undefined, 'measured columns carry no derivedFrom');
assert.deepEqual([byId.get(DERIVED.power).kind, byId.get(DERIVED.power).units], ['power_density', 'W/m²']);
assert.equal(imported.summaries.length, columns.length, 'summaries cover the derived columns');

// Values, step by step.
const series = (column) => {
  const packed = session.series(send({ dataset, column, fromMs: FIRST_MS, toMs: FIRST_MS + DAY_STEPS * TEN_MINUTES_MS, buckets: 0 }));
  return Array.from({ length: packed.length / 2 }, (_, point) => packed[2 * point + 1]);
};
const near = (value, want) => (Number.isNaN(want) ? Number.isNaN(value) : Math.abs(value - want) <= TOLERANCE * Math.abs(want));
const adjustmentPa =
  standardPressure(SITE_ELEVATION_M + TEMPERATURE_HEIGHT_M) - standardPressure(SITE_ELEVATION_M + PRESSURE_HEIGHT_M);
const [density, turbulence, power] = [series(DERIVED.density), series(DERIVED.turbulence), series(DERIVED.power)];
inputs.forEach(({ speed, deviation, temperature, pressure }, step) => {
  const wantDensity = moistAir(pressure * PASCALS_PER_KILOPASCAL + adjustmentPa, temperature + CELSIUS_ZERO_K);
  // The original's rule (the default): σ/U above 0.01 m/s, 0 at it or below.
  const wantTurbulence = speed > TI_MIN_SPEED_M_S ? deviation / speed : 0;
  const wantPower = 0.5 * wantDensity * speed ** 3; // ½·ρ·U³
  assert.ok(near(density[step], wantDensity), `density at ${step}: ${density[step]} vs ${wantDensity}`);
  assert.ok(near(turbulence[step], wantTurbulence), `TI at ${step}: ${turbulence[step]} vs ${wantTurbulence}`);
  assert.ok(near(power[step], wantPower), `WPD at ${step}: ${power[step]} vs ${wantPower}`);
});

// Flags: inherited from the sources, refused on a derived column.
const valid = (state, id) => state.summaries[position(id)].valid;
const stepsMs = ([first, end]) => ({ fromMs: FIRST_MS + first * TEN_MINUTES_MS, toMs: FIRST_MS + (end - 1) * TEN_MINUTES_MS });
assert.throws(
  () => session.flag(send({ dataset, columns: [DERIVED.power], flag: INVALID_FLAG, ...stepsMs(DEVIATION_FLAGGED) })),
  /derived_column_not_flaggable/,
);
const before = imported;
const validTurbulenceFlagged = turbulence.slice(...DEVIATION_FLAGGED).filter((value) => !Number.isNaN(value)).length;
let state = JSON.parse(session.flag(send({ dataset, columns: [IDS.deviation], flag: INVALID_FLAG, ...stepsMs(DEVIATION_FLAGGED) })));
assert.equal(valid(state, DERIVED.turbulence), valid(before, DERIVED.turbulence) - validTurbulenceFlagged, 'TI follows its SD');
assert.equal(valid(state, DERIVED.power), valid(before, DERIVED.power), 'WPD does not use the SD');
const temperatureSteps = TEMPERATURE_FLAGGED[1] - TEMPERATURE_FLAGGED[0];
state = JSON.parse(session.flag(send({ dataset, columns: [IDS.temperature], flag: INVALID_FLAG, ...stepsMs(TEMPERATURE_FLAGGED) })));
assert.equal(valid(state, DERIVED.density), valid(before, DERIVED.density) - temperatureSteps, 'density follows the temperature');
assert.equal(valid(state, DERIVED.power), valid(before, DERIVED.power) - temperatureSteps, 'WPD follows it through the density');
state = JSON.parse(session.undo(send({ dataset })));
assert.equal(valid(state, DERIVED.power), valid(before, DERIVED.power), 'undo restores the derived summaries');

// CSV export: derived columns only when asked.
const header = (request) => session.exportCsv(send({ dataset, delimiter: ',', ...request })).split('\n')[0].split(',');
assert.deepEqual(header({}), ['Timestamp', ...Object.values(IDS)]);
assert.deepEqual(header({ includeDerived: true }), ['Timestamp', ...Object.values(IDS), ...Object.values(DERIVED)]);

// Re-import without the pressure column and without the elevation: no air
// density, and WPD falls back to the standard density.
const PRESSURE_FILE_COLUMN = 4;
const withoutPressure = { columns: [{ fileColumn: PRESSURE_FILE_COLUMN, skip: true }] };
const reimported = JSON.parse(session.importCsv(source, send({ name: 'torre', config: withoutPressure, replace: dataset })));
const reimportedIds = reimported.dataset.columns.map((column) => column.id);
assert.ok(!reimportedIds.includes(DERIVED.density), 'no density without pressure or elevation');
assert.deepEqual(reimported.dataset.columns.find((column) => column.id === DERIVED.power).derivedFrom, [IDS.speed]);
assert.equal(reimported.dataset.segments.length, 1, 'the SD flag carried over (the temperature one was undone)');

console.log('derived columns: ok');

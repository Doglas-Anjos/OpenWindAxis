// Project files end to end: web/src/project.ts writes and reads them, and the
// real WASM core re-imports each dataset with its saved configuration and flags.
// Usage (after `npm run wasm` in web/): node bench/project_check.mjs web
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const webDirectory = resolve(process.argv[2]);
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

/** A one-day CSV whose speeds cycle with `period`. */
function csv(period) {
  const rows = ['t,ws,dir'];
  for (let step = 0; step < DAY_STEPS; step++) {
    const stamp = new Date(FIRST_MS + step * TEN_MINUTES_MS).toISOString().slice(0, 16).replace('T', ' ');
    rows.push(`${stamp},${step % period},${(step * 7) % 360}`);
  }
  return new TextEncoder().encode(rows.join('\n'));
}

const session = new Session();
const send = (request) => JSON.stringify(request);
const config = { columns: [{ fileColumn: 1, heightM: CORRECTED_HEIGHT_M }] };
const sources = [new File([csv(PERIOD_A)], 'torre_a.csv'), new File([csv(PERIOD_B)], 'torre_b.csv')];

// Import both, flag the first one, save.
const saved = [];
for (const file of sources) {
  const result = JSON.parse(session.importCsv(new Uint8Array(await file.arrayBuffer()), send({ name: file.name, config })));
  saved.push({ file, config, result });
}
const first = saved[0].result.dataset;
const stretch = { dataset: first.id, columns: ['ws'], fromMs: first.startMs, toMs: first.startMs + (FLAGGED_STEPS - 1) * TEN_MINUTES_MS };
const edited = JSON.parse(session.flag(send({ ...stretch, flag: INVALID_FLAG })));
// The first dataset shows its columns by type with systematic names; the second keeps none.
const display = { naming: 'systematic', heightDecimals: 1, order: 'type_then_height', highestFirst: false, hidden: [{ kind: 'wind_speed', subtype: 'std_dev' }], colors: 'by_type' };
const datasets = saved.map(({ file, result }, index) => ({
  file,
  config,
  restore: { flags: result.dataset.flags, segments: index === 0 ? edited.segments : result.dataset.segments },
  display: index === 0 ? display : undefined,
}));
const blob = project.writeProject(datasets);

// Read it back: same sources, byte for byte, and the same state after import.
const reopened = await project.readProject(blob);
assert.equal(reopened.length, sources.length);
assert.deepEqual(reopened.map((dataset) => dataset.display), [display, undefined], 'the column display settings come back');
for (const [index, { file, config: readConfig, restore }] of reopened.entries()) {
  assert.equal(file.name, sources[index].name);
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), new Uint8Array(await sources[index].arrayBuffer()));
  const result = JSON.parse(session.importCsv(new Uint8Array(await file.arrayBuffer()), send({ name: file.name, config: readConfig, restore })));
  assert.deepEqual(result.dataset.segments, datasets[index].restore.segments);
  assert.equal(result.dataset.columns[0].heightM, CORRECTED_HEIGHT_M, 'the saved configuration is applied');
}
const reflagged = JSON.parse(session.importCsv(csv(PERIOD_A), send({ name: 'a', config, restore: datasets[0].restore })));
assert.deepEqual(reflagged.summaries[0].valid, edited.summaries[0].valid, 'summaries see the restored flags');

// Damaged files are refused with a message, never half-read. The layout is
// written out here on purpose, as a check independent of project.ts.
const MAGIC = 'WPRJ';
const LENGTH_BYTES = 4;
const HEADER_BYTES = MAGIC.length + LENGTH_BYTES;
const LITTLE_ENDIAN = true;
const refused = async (damaged, pattern) => assert.rejects(project.readProject(damaged), pattern);
await refused(new Blob(['nada disso']), /não é um projeto/);
await refused(blob.slice(0, blob.size - 1), /não é um projeto/);
await refused(new Blob([blob, 'lixo no fim']), /não é um projeto/);
const manifestBytes = new DataView(await blob.slice(0, HEADER_BYTES).arrayBuffer()).getUint32(MAGIC.length, LITTLE_ENDIAN);
const manifest = JSON.parse(await blob.slice(HEADER_BYTES, HEADER_BYTES + manifestBytes).text());
const sourcesPart = blob.slice(HEADER_BYTES + manifestBytes);
const withManifest = (changed) => {
  const json = new TextEncoder().encode(JSON.stringify(changed));
  const length = new DataView(new ArrayBuffer(LENGTH_BYTES));
  length.setUint32(0, json.length, LITTLE_ENDIAN);
  return new Blob([MAGIC, length, json, sourcesPart]);
};
assert.equal((await project.readProject(withManifest(manifest))).length, sources.length, 'the rewrite itself is sound');
const damagedDisplay = manifest.datasets.map((dataset, index) => (index === 0 ? { ...dataset, display: { ...display, order: 'alphabetical' } } : dataset));
const [shownAsRead] = await project.readProject(withManifest({ ...manifest, datasets: damagedDisplay }));
assert.equal(shownAsRead.display, undefined, 'damaged display settings fall back to the columns as read');
await refused(withManifest({ ...manifest, format: manifest.format + 1 }), /versão mais nova/);
await refused(withManifest(null), /não é um projeto/);
const withoutFlags = manifest.datasets.map(({ restore, ...rest }) => rest);
await refused(withManifest({ ...manifest, datasets: withoutFlags }), /não é um projeto/);

// A restore that does not fit fails before the session changes.
const MISSING_COLUMN = 'coluna_que_nao_existe';
const misplaced = { ...datasets[0].restore, segments: [{ column: MISSING_COLUMN, flag: INVALID_FLAG, startStep: 0, endStep: 5 }] };
const flagsOf = (dataset) => JSON.parse(session.exportJson(send({ dataset }))).segments;
const before = flagsOf(first.id);
assert.throws(
  () => session.importCsv(csv(PERIOD_A), send({ name: 'a', config, restore: misplaced, replace: first.id })),
  /unknown_column/,
);
assert.deepEqual(flagsOf(first.id), before, 'the replaced dataset is untouched');
assert.throws(() => session.importCsv(csv(PERIOD_A), send({ name: 'a', config, restore: misplaced })), /unknown_column/);
const next = JSON.parse(session.importCsv(csv(PERIOD_A), send({ name: 'a', config })));
assert.equal(next.dataset.id, reflagged.dataset.id + 1, 'the failed import took no id');

// Appended files (Arquivo › Anexar dados): one buffer, the first file then the
// appended ones, each sized; flags follow their times when an earlier day is
// appended later, and the project keeps every file.
const DAY_MS = DAY_STEPS * TEN_MINUTES_MS;
/** A one-day CSV starting `days` after the first day. */
function day(days, period = PERIOD_A) {
  const shifted = new TextDecoder().decode(csv(period)).replace(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}/gm, (stamp) => {
    const time = Date.parse(`${stamp.replace(' ', 'T')}:00Z`) + days * DAY_MS;
    return new Date(time).toISOString().slice(0, 16).replace('T', ' ');
  });
  return new TextEncoder().encode(shifted);
}
const joined = (...parts) => {
  const bytes = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  parts.reduce((offset, part) => (bytes.set(part, offset), offset + part.length), 0);
  return bytes;
};
const [dayOne, dayTwo, dayZero] = [day(1), day(2), day(0)];
const daily = JSON.parse(
  session.importCsv(joined(dayOne, dayTwo), send({ name: 'dia1.csv', config, appended: [{ name: 'dia2.csv', bytes: dayTwo.length }] })),
);
assert.equal(daily.dataset.stepCount, 2 * DAY_STEPS, 'two days in one dataset');
assert.deepEqual(daily.appended.map((status) => [status.name, status.steps, status.error]), [['dia2.csv', DAY_STEPS, null]]);
const FLAGGED_FROM = DAY_STEPS + 6;
const flaggedDay = { dataset: daily.dataset.id, columns: ['ws'], fromMs: daily.dataset.startMs + FLAGGED_FROM * TEN_MINUTES_MS, toMs: daily.dataset.startMs + (FLAGGED_FROM + 9) * TEN_MINUTES_MS };
session.flag(send({ ...flaggedDay, flag: INVALID_FLAG }));
// An earlier day appended later: the record starts a day sooner and the flag keeps its time.
const sources3 = [{ name: 'dia2.csv', bytes: dayTwo.length }, { name: 'dia0.csv', bytes: dayZero.length }];
const longer = JSON.parse(session.importCsv(joined(dayOne, dayTwo, dayZero), send({ name: 'dia1.csv', config, appended: sources3, replace: daily.dataset.id })));
assert.equal(longer.dataset.stepCount, 3 * DAY_STEPS);
assert.equal(longer.dataset.startMs, daily.dataset.startMs - DAY_MS);
assert.deepEqual(
  longer.dataset.segments.map((segment) => [segment.startStep, segment.endStep]),
  [[FLAGGED_FROM + DAY_STEPS, FLAGGED_FROM + DAY_STEPS + 10]],
  'the flag moved a day along the grid, staying on its time',
);
const appendedFiles = [new File([dayTwo], 'dia2.csv'), new File([dayZero], 'dia0.csv')];
const withAppended = project.writeProject([
  { file: new File([dayOne], 'dia1.csv'), appended: appendedFiles, config, restore: JSON.parse(session.projectState(send({ dataset: daily.dataset.id }))) },
]);
const [reread] = await project.readProject(withAppended);
assert.deepEqual(reread.appended.map((file) => file.name), ['dia2.csv', 'dia0.csv'], 'the appended files come back in order');
assert.deepEqual(new Uint8Array(await reread.appended[1].arrayBuffer()), dayZero);
const sizes = reread.appended.map((file) => ({ name: file.name, bytes: file.size }));
const bytes = joined(...(await Promise.all([reread.file, ...reread.appended].map(async (file) => new Uint8Array(await file.arrayBuffer())))));
const reopenedDaily = JSON.parse(session.importCsv(bytes, send({ name: reread.file.name, config: reread.config, restore: reread.restore, appended: sizes })));
assert.equal(reopenedDaily.dataset.stepCount, 3 * DAY_STEPS);
assert.deepEqual(reopenedDaily.dataset.segments, longer.dataset.segments, 'the reopened project has the same flags');
await refused(withAppended.slice(0, withAppended.size - 1), /não é um projeto/);

console.log('project round trip: ok');

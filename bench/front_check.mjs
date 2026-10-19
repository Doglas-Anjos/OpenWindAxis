// The browser's pure modules, without a browser: number formatting
// (format.ts) and the column display settings (display.ts): names, order,
// hidden columns, colours, and the check of saved settings.
// Usage: node bench/front_check.mjs web
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const webDirectory = resolve(process.argv[2]);
const esbuild = createRequire(join(webDirectory, 'package.json'))('esbuild');
const bundle = esbuild.buildSync({
  stdin: { contents: "export * from './src/format'; export * from './src/display';", resolveDir: webDirectory, loader: 'ts' },
  bundle: true,
  format: 'esm',
  write: false,
});
const front = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString('base64')}`);

// Numbers: a gap (NaN, as raw series values mark one) reads as no value.
assert.equal(front.formatNumber(Number.NaN), front.NO_VALUE, 'a gap is no value, not "NaN"');
assert.equal(front.formatNumber(null), front.NO_VALUE);
assert.equal(front.formatNumber(1234.5), '1.234,5', 'pt-BR notation');

// A tower as read: two speeds with an SD, a direction, a derived TI.
const column = (id, kind, subtype, heightM, extra = {}) => ({ id, kind, subtype, heightM, units: '', parent: null, ...extra });
const dataset = {
  id: 0,
  columns: [
    column('WS80', 'wind_speed', 'mean', 80),
    column('WD78', 'wind_direction', 'mean', 78),
    column('WS100', 'wind_speed', 'mean', 100),
    column('WS100SD', 'wind_speed', 'std_dev', 100, { parent: 'WS100' }),
    column('WS100 TI', 'turbulence_intensity', 'mean', 100, { derivedFrom: ['WS100', 'WS100SD'] }),
  ],
};
const ids = (view) => view.shown().map((shown) => shown.id);

const asRead = front.columnView(dataset, front.DEFAULT_DISPLAY);
assert.deepEqual(ids(asRead), ['WS80', 'WD78', 'WS100', 'WS100SD', 'WS100 TI'], 'the file order');
assert.equal(asRead.name(dataset.columns[0]), 'WS80', 'the file id');
assert.equal(asRead.color(dataset.columns[0]), undefined, "the chart's palette");

const systematic = front.columnView(dataset, {
  ...front.DEFAULT_DISPLAY,
  naming: 'systematic',
  heightDecimals: 1,
  order: 'type_then_height',
  highestFirst: true,
  hidden: [{ kind: 'wind_speed', subtype: 'std_dev' }],
  colors: 'by_type',
});
assert.deepEqual(ids(systematic), ['WS100', 'WS80', 'WD78', 'WS100 TI'], 'by type, the highest first, the SD hidden');
assert.equal(systematic.name(dataset.columns[2]), 'Vel 100.0m Média');
assert.equal(systematic.name(dataset.columns[4]), 'WS100 TI', 'a derived column keeps its id');
assert.equal(systematic.visible(dataset.columns[3]), false);
const lightness = (color) => Number(/(\d+(?:\.\d+)?)%\)$/.exec(color)[1]);
assert.ok(lightness(systematic.color(dataset.columns[2])) < lightness(systematic.color(dataset.columns[0])), 'the higher sensor is darker');

const byHeight = front.columnView(dataset, { ...front.DEFAULT_DISPLAY, order: 'height_then_type', highestFirst: false });
assert.deepEqual(ids(byHeight), ['WD78', 'WS80', 'WS100', 'WS100SD', 'WS100 TI'], 'the lowest first, then by type and statistic');

// Two sensors that would share a systematic name keep their ids after it.
const twins = { id: 1, columns: [column('A', 'wind_speed', 'mean', 80), column('B', 'wind_speed', 'mean', 80)] };
const twinView = front.columnView(twins, { ...front.DEFAULT_DISPLAY, naming: 'systematic' });
assert.deepEqual(twins.columns.map((twin) => twinView.name(twin)), ['Vel 80m Média (A)', 'Vel 80m Média (B)']);

// Saved settings: only well-formed ones are drawn with.
assert.equal(front.isDisplay(front.DEFAULT_DISPLAY), true);
assert.equal(front.isDisplay({ ...front.DEFAULT_DISPLAY, order: 'alphabetical' }), false);
assert.equal(front.isDisplay({ ...front.DEFAULT_DISPLAY, heightDecimals: 20 }), false, 'toFixed would throw past its range');
assert.equal(front.isDisplay(null), false);

console.log('front modules: ok');

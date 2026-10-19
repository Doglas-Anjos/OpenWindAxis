// The real UI end to end, without the browser extension: drives a headless
// Chrome (with its own temporary profile) over the DevTools protocol through
// the main window as a user would — menus, toolbar, Workbook Explorer,
// analysis and flag windows, dialogs — importing the sample, opening every
// analysis, filtering, flagging by hand and by rule, undoing, configuring the
// dataset, saving and reopening the workbook. Saves screenshots plus every
// console error.
// Usage (with `npm run dev` serving the app):
//   node bench/ui_check.mjs "C:/Program Files/Google/Chrome/Application/chrome.exe" http://localhost:5199/ <abs path>/samples/torre_exemplo.csv <out dir>
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [chromePath, appUrl, samplePath, outDir] = process.argv.slice(2);
const PORT = 9333;
/** Waits that let the worker answer and React draw. */
const SETTLE_MS = 1500;
const SHORT_SETTLE_MS = 300;
const LONG_SETTLE_MS = 3000;
const SCREENSHOT_SETTLE_MS = 600;
const POLL_MS = 250;
const STARTUP_POLLS = 50;
const STARTUP_POLL_MS = 200;
const DOWNLOAD_POLLS = 40;
/** Polls for the session to reach the browser's store (it is saved 1.5 s after a change). */
const SESSION_POLLS = 120;
/** The help pages, and polls for their tab to load. */
const HELP_PATH = '/ajuda/index.html';
const HELP_TITLE = 'Ajuda do OpenWindAxis';
const HELP_POLLS = 40;
const DEFAULT_TIMEOUT_MS = 60_000;
const IMPORT_TIMEOUT_MS = 120_000;
const VIEWPORT = { width: 1600, height: 900 };
/** The summary graphs: coverage, shear, rose, monthly means, diurnal. */
const SUMMARY_GRAPHS = 5;
/** June to August, 1-based, for the month filter. */
const JUNE = '6';
const AUGUST = '8';
/** The rule flags speeds at or above this, m/s; the calm threshold tried, m/s. */
const RULE_MAX_SPEED = '20';
const CALM_THRESHOLD = '2';
/** Scale applied to the rule's column by Revisar › Escala e offset. */
const SPEED_SCALE = '2';
/** The long-term reference built from the sample: its preamble, the logger's
 *  missing code, the day/month/year hour prefix and where the year sits in it. */
const SAMPLE_PREAMBLE_LINES = 3;
const MISSING_CODE = -999;
const HOUR_PREFIX = 13;
const YEAR_FROM = 6;
const YEAR_TO = 10;
/** Scale of each reference year, oldest first; the last is the sample's year. */
const REFERENCE_YEARS = [1.04, 0.97, 1.02, 0.99, 1.0];
/** The sample's direction field, and how far the reference's vane is turned from it, °. */
const SAMPLE_DIRECTION_FIELD = 6;
const REFERENCE_TURN_DEG = 10;
/** The vertical extrapolation the check asks for, and the column it makes. */
const EXTRAPOLATION_HEIGHT = '120';
/** Rows of the Flag manual values table the check drags over, from the first. */
const TABLE_DRAG_FROM = 10;
const TABLE_DRAG_TO = 15;
/** The flag Definir flags adds, and the one it deletes. */
const NEW_FLAG_NAME = 'Suspeito';
const DELETED_FLAG = 'Gelo';
/** The dataset's height in the explorer with the lowest speed as its primary. */
const PRIMARY_LOWEST_HEIGHT = '80 m';
/** The constant displacement height the check sets, m. */
const DISPLACEMENT_M = '10';
/** A boom bearing on the first speed sensor, and the direction it is shaded from. */
const BOOM_DEG = 90;
const BOOM_SHADED_DEG = 270;
/** Half the original's 30° shaded sector. */
const BOOM_HALF_WIDTH_DEG = 15;
/** The calculated column the check adds: the average of the two speeds. */
const CALCULATED_COLUMN = 'Vel média 80-100';
/** And an algebraic expression: 100 m minus 80 m. */
const EXPRESSION_COLUMN = 'Diferença 100-80';
/** And a solar variable: the sun's altitude. */
const SOLAR_COLUMN = 'Altura do sol';
/** The north sector's displacement height in the by-sector check, m. */
const NORTH_DISPLACEMENT_M = '30';
/** A column hidden by the column display check, and a systematic name it shows. */
const HIDDEN_COLUMN = 'Vel 100m DP';
const SYSTEMATIC_NAME = 'Vel 100m Média';
/** Datasets open (and visible) when the "<todos visíveis>" check runs. */
const ALL_VISIBLE_DATASETS = 3;
/** Hours in each generated reanalysis file. */
const REANALYSIS_HOURS = 72;
/** Ten-minute steps in each generated logger file. */
const LOGGER_STEPS = 36;
/** The map's fine zoom, levels, and how close the measured tile scale must come to it. */
const FINE_ZOOM_STEP = 0.25;
/** Fields of an Openwind data row: five of time, five of the height, and L. */
const OPENWIND_FIELDS = 11;
/** The mean the scaled Openwind export aims at, m/s, and how near the file must come (3-decimal rounding). */
const OPENWIND_TARGET_MEAN = 10;
const OPENWIND_MEAN_TOLERANCE = 0.001;
/** Mouse moves of a drag on a chart: enough for ECharts to see a brush. */
const DRAG_MOVES = 10;
const SCALE_TOLERANCE = 0.02;
/** The EPE station number the export check types. */
const EPE_STATION = '1234567';
/** Fields of an EPE record split on "|": 22 channels and the empty one after the closing pipe. */
const EPE_FIELDS_WITH_END = 23;
/** The EPE file's extension: a download still in progress has another. */
const EPE_EXTENSION = '.txt';
const EXTRAPOLATED_COLUMN = 'Vel 120 m extrapolada';
/** The column the clock-shift rule searches. */
const TEMPERATURE_COLUMN = 'Temp 2m';
/** The column the rule searches. */
const RULE_COLUMN = 'Vel 100m Média';
/** How much of a window's text to print. */
const PRINT_CHARS = 700;
/** The analysis windows, as the Analisar menu names them. */
const ANALYSES = ['Cobertura de dados', 'Histograma', 'Rosa dos ventos', 'Perfil diurno', 'Cisalhamento do vento'];
mkdirSync(outDir, { recursive: true });
const chrome = spawn(chromePath, [
  '--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${join(outDir, 'profile')}`,
  `--window-size=${VIEWPORT.width},${VIEWPORT.height}`, '--no-first-run', '--no-default-browser-check', 'about:blank',
], { stdio: 'ignore' });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let targets;
for (let attempt = 0; attempt < STARTUP_POLLS; attempt++) {
  try { targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json(); break; } catch { await sleep(STARTUP_POLL_MS); }
}
const page = targets.find((target) => target.type === 'page');
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve) => socket.addEventListener('open', resolve));
let nextId = 1;
const pending = new Map();
const problems = [];
socket.addEventListener('message', (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  } else if (message.method === 'Runtime.exceptionThrown') {
    problems.push(`exception: ${message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text}`);
  } else if (message.method === 'Runtime.consoleAPICalled' && ['error', 'warning'].includes(message.params.type)) {
    problems.push(`console.${message.params.type}: ${message.params.args.map((arg) => arg.value ?? arg.description).join(' ')}`);
  } else if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') {
    problems.push(`log: ${message.params.entry.text} ${message.params.entry.url ?? ''}`);
  }
});
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, (message) => (message.error ? reject(new Error(`${method}: ${message.error.message}`)) : resolve(message.result)));
    socket.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => {
  const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(`${expression}: ${result.exceptionDetails.exception?.description}`);
  return result.result.value;
};
const waitFor = async (expression, label, timeoutMs = DEFAULT_TIMEOUT_MS) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await evaluate(`!!(${expression})`)) return;
    await sleep(POLL_MS);
  }
  throw new Error(`timed out waiting for ${label}`);
};
const shot = async (name) => {
  await sleep(SCREENSHOT_SETTLE_MS);
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(data, 'base64'));
};
/** The window on top in the right pane: the other open windows stay mounted, hidden. */
const ACTIVE = `document.querySelector('.window:not(.behind)')`;
/** Clicks the first element under `scope` matching `selector` whose text is `text`. */
const clickText = (text, selector = 'button, [role=tab], summary, label', scope = 'document') =>
  evaluate(`(() => { const el = [...${scope}.querySelectorAll(${JSON.stringify(selector)})].find(e => e.textContent.trim() === ${JSON.stringify(text)}); if (!el || el.disabled) return false; el.click(); return true; })()`);
/** Picks `item` from the menu `menu` of the menu bar. */
const pickMenu = async (menu, item) => {
  if (!(await clickText(menu, '.menu > button'))) throw new Error(`no menu ${menu}`);
  await sleep(SHORT_SETTLE_MS);
  const picked = await evaluate(`(() => { const el = [...document.querySelectorAll('.dropdown button')].find(b => b.querySelector('.label')?.textContent === ${JSON.stringify(item)}); if (!el || el.disabled) return false; el.click(); return true; })()`);
  if (!picked) throw new Error(`no enabled item ${menu} › ${item}`);
  await sleep(SETTLE_MS);
};
const noteErrors = () => evaluate(`[...document.querySelectorAll('.note.err')].map(e => e.textContent.trim()).filter(Boolean)`);
/** Sets a React-controlled select or input and fires the event React listens to. */
const setValue = (selector, value) =>
  evaluate(`(() => { const el = ${selector}; const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); return true; })()`);
/** The field of the open dialog whose label starts with `label`. */
const modalSelect = (label) => `[...document.querySelectorAll('.modal label')].find(l => l.innerText.startsWith(${JSON.stringify(label)})).querySelector('select, input')`;
const analysisFacts = () => evaluate(`${ACTIVE}.querySelector('.facts')?.innerText.replace(/\\n/g, ' ')`);
const flat = (text) => (text ?? '').replace(/\n/g, ' | ');
const datasetRows = `document.querySelectorAll('.datasets tbody tr').length`;

await send('Page.enable');
await send('Runtime.enable');
await send('Log.enable');
await send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: 1, mobile: false });
// The test profile lives on between runs: start from an empty store, not the last run's session.
await send('Storage.clearDataForOrigin', { origin: new URL(appUrl).origin, storageTypes: 'indexeddb' });
await send('Page.navigate', { url: appUrl });
await waitFor(`document.querySelector('.menubar')`, 'main window');
await waitFor(`!document.querySelector('.statusbar').innerText.includes('Carregando')`, 'core loaded');
console.log(`title: ${await evaluate('document.title')}`);
await shot('01-vazio');

// Add the sample through the explorer's add-dataset input.
const { root } = await send('DOM.getDocument', { depth: -1 });
const { nodeId } = await send('DOM.querySelector', { nodeId: root.nodeId, selector: '.minibar input[type=file]' });
const started = Date.now();
await send('DOM.setFileInputFiles', { nodeId, files: [samplePath] });
await waitFor(`${datasetRows} === 1`, 'dataset row', IMPORT_TIMEOUT_MS);
console.log(`import shown after ${Date.now() - started} ms`);
await waitFor(`document.querySelectorAll('.summary .chart canvas').length >= ${SUMMARY_GRAPHS}`, 'summary graphs');
console.log(`explorer row: ${flat(await evaluate(`document.querySelector('.datasets tbody tr').innerText`))}`);
console.log(`map pins: ${await evaluate(`document.querySelectorAll('.sitemap .leaflet-marker-icon').length`)}`);
await shot('02-workbook');

// The dataset list folds away, giving the map the pane; the map's small +/− zoom a quarter level.
const mapHeight = `document.querySelector('.sitemap').clientHeight`;
const unfoldedHeight = await evaluate(mapHeight);
await evaluate(`document.querySelector('.minibar .fold').click()`);
await sleep(SHORT_SETTLE_MS);
const foldedHeight = await evaluate(mapHeight);
const listHidden = await evaluate(`document.querySelector('.datasets').hidden`);
await shot('02b-lista-recolhida');
await evaluate(`document.querySelector('.minibar .fold').click()`);
await sleep(SHORT_SETTLE_MS);
console.log(`explorer fold: map ${unfoldedHeight} px -> ${foldedHeight} px, list hidden ${listHidden}`);
if (!listHidden || !(foldedHeight > unfoldedHeight)) problems.push('explorer fold: the list did not fold');
/** Scale of the tiles on screen against their own level: 1 at a whole level, 2^¼ a fine step further in. */
const tileScale = `(() => { const pane = [...document.querySelectorAll('.sitemap .leaflet-tile-container')].find(c => c.querySelector('img')); const scale = /scale\\(([\\d.]+)\\)/.exec(pane?.style.transform ?? ''); return scale ? Number(scale[1]) : 1; })()`;
const scaleBefore = await evaluate(tileScale);
await evaluate(`document.querySelector('.sitemap .fine-zoom a').click()`);
await sleep(SETTLE_MS);
const scaleAfter = await evaluate(tileScale);
console.log(`fine zoom: tiles scaled ${scaleBefore.toFixed(3)} -> ${scaleAfter.toFixed(3)} (a quarter level is ×${(2 ** FINE_ZOOM_STEP).toFixed(3)})`);
if (Math.abs(scaleAfter / scaleBefore - 2 ** FINE_ZOOM_STEP) > SCALE_TOLERANCE && Math.abs(scaleAfter * 2 / scaleBefore - 2 ** FINE_ZOOM_STEP) > SCALE_TOLERANCE) {
  problems.push(`fine zoom: scale ${scaleBefore} -> ${scaleAfter}`);
}
await shot('02c-zoom-fino');
await evaluate(`document.querySelector('.sitemap .fine-zoom a:last-child').click()`);
await sleep(SETTLE_MS);

await pickMenu('Analisar', 'Série temporal');
await waitFor(`${ACTIVE}.querySelector('.chart canvas')`, 'time series chart');
await shot('03-serie-temporal');
await evaluate(`[...${ACTIVE}.querySelectorAll('.ts-display label')].find(l => l.innerText.includes('Médias diárias')).querySelector('input').click()`);
await waitFor(`${ACTIVE}.querySelector('.ts-chart canvas')`, 'daily means chart');
await shot('03b-medias-diarias');
await evaluate(`[...${ACTIVE}.querySelectorAll('.ts-display label')].find(l => l.innerText.includes('Dados medidos')).querySelector('input').click()`);

for (const [index, name] of ANALYSES.entries()) {
  await pickMenu('Analisar', name);
  const errors = await noteErrors();
  if (errors.length) problems.push(`${name}: ${errors.join(' | ')}`);
  await shot(`0${4 + index}-${name.toLowerCase().replace(/ /g, '-')}`);
}

// Histogram: a real month filter (June to August), then the confidence interval.
await pickMenu('Analisar', 'Histograma');
console.log(`histogram: ${await analysisFacts()}`);
const monthRow = `[...${ACTIVE}.querySelectorAll('.filterby .criterion')].find(r => r.innerText.startsWith('Mês'))`;
await evaluate(`${monthRow}.querySelector('input[type=checkbox]').click()`);
await sleep(SHORT_SETTLE_MS);
await setValue(`${monthRow}.querySelectorAll('select')[0]`, JUNE);
await sleep(SHORT_SETTLE_MS);
await setValue(`${monthRow}.querySelectorAll('select')[1]`, AUGUST);
await sleep(SETTLE_MS);
console.log(`histogram, June–August: ${await analysisFacts()}`);
await evaluate(`${monthRow}.querySelector('input[type=checkbox]').click()`);
await evaluate(`[...${ACTIVE}.querySelectorAll('label.check')].find(l => l.innerText.includes('Intervalo de confiança')).querySelector('input').click()`);
await sleep(LONG_SETTLE_MS);
console.log(`histogram with interval: ${await analysisFacts()}`);
await shot('09-histograma-ic');
if (!(await clickText('Tabela', '.format button', ACTIVE))) problems.push('histogram: no table format');
await sleep(SHORT_SETTLE_MS);
console.log(`histogram table rows: ${await evaluate(`${ACTIVE}.querySelectorAll('table tbody tr').length`)}`);
await shot('10-histograma-tabela');

// The histogram's recipe, saved by the window and replayed by the native binary when it is built.
const downloads = join(outDir, 'downloads');
// The checks find files by extension: a reused folder would hand them the last run's.
rmSync(downloads, { recursive: true, force: true });
mkdirSync(downloads, { recursive: true });
await send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });
if (!(await clickText('Exportar receita', 'button', ACTIVE))) problems.push('recipe: export disabled');
let recipeFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !recipeFile; attempt++) {
  await sleep(POLL_MS);
  recipeFile = readdirSync(downloads).find((name) => name.endsWith('_receita.json'));
}
if (!recipeFile) throw new Error('no recipe downloaded');
const recipe = JSON.parse(readFileSync(join(downloads, recipeFile), 'utf8'));
console.log(`recipe ${recipeFile}: ${recipe.analysis}, sha256 ${recipe.source.sha256.slice(0, 12)}…, core ${recipe.coreVersion}`);
const windBinary = ['wind.exe', 'wind'].map((name) => join(dirname(fileURLToPath(import.meta.url)), '..', 'target', 'release', name)).find(existsSync);
if (windBinary) {
  const report = JSON.parse(execFileSync(windBinary, ['replay', join(downloads, recipeFile), samplePath], { encoding: 'utf8' }));
  console.log(`recipe replayed natively: file ${report.sourceMatches}, result ${report.resultMatches}`);
}

await pickMenu('Analisar', 'Cisalhamento do vento');
console.log(`shear: ${await analysisFacts()}`);
// The other displays: α by month and hour (a line per month), the SD shear, the step-by-step frequency;
// the direction shear needs two vanes, which the sample lacks.
const shearSelect = (label) => `[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith(${JSON.stringify(label)})).querySelector('select')`;
await setValue(shearSelect('Subdividir por'), 'month_hour');
await waitFor(`${ACTIVE}.querySelector('.analysis canvas') && !${ACTIVE}.querySelector('.note.err')`, 'shear by month and hour');
await shot('10b-cisalhamento-mes-hora');
await setValue(shearSelect('Exibir'), 'speed_sd');
await setValue(shearSelect('Subdividir por'), 'none');
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.startsWith('α')`, 'the SD shear');
const sdShear = await analysisFacts();
console.log(`speed SD shear: ${sdShear}`);
await setValue(shearSelect('Exibir'), 'freq_speed');
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.startsWith('Passos')`, 'the shear frequency');
console.log(`shear frequency: ${await analysisFacts()}`);
await shot('10c-cisalhamento-frequencia');
await setValue(shearSelect('Exibir'), 'direction');
await waitFor(`${ACTIVE}.querySelector('.note.err')`, 'the direction shear note');
const veerNote = await evaluate(`${ACTIVE}.querySelector('.note.err').innerText`);
console.log(`direction shear: ${veerNote}`);
if (!veerNote.includes('dois sensores de direção')) problems.push(`direction shear: ${veerNote}`);
await setValue(shearSelect('Exibir'), 'speed');
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.startsWith('Expoente')`, 'the speed profile again');
await clickText('Tabela', '.format button', ACTIVE);
await waitFor(`${ACTIVE}.querySelector('.heightfits')`, 'the height pair fits');
console.log(`height fits: ${flat(await evaluate(`${ACTIVE}.querySelector('.heightfits tbody').innerText`))}`);
await clickText('Gráfico', '.format button', ACTIVE);
await pickMenu('Analisar', 'Rosa dos ventos');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Calmaria abaixo de')).querySelector('input')`, CALM_THRESHOLD);
await sleep(SETTLE_MS);
console.log(`rose with calms below 2 m/s: ${await evaluate(`[...${ACTIVE}.querySelectorAll('.hint')].map(h => h.innerText).find(t => t.includes('passos com dado')) ?? ''`)}`);
await shot('11-rosa-calmaria');

// Distribuição de velocidade: the four Weibull fits side by side, each with its R².
await pickMenu('Analisar', 'Distribuição de velocidade');
await waitFor(`${ACTIVE}.querySelector('table.distribution tbody tr')`, 'the four fits');
await sleep(SETTLE_MS);
const fitRows = await evaluate(`[...${ACTIVE}.querySelectorAll('table.distribution tbody tr')].map(r => r.innerText.replace(/\t/g, ' | '))`);
console.log(`distribution: ${fitRows.join(' || ')}`);
const rSquares = fitRows.map((row) => Number(row.split(' | ').at(-1).replace(',', '.')));
if (fitRows.length !== 4 || !rSquares.every((r2) => r2 > 0 && r2 <= 1)) problems.push(`distribution: ${fitRows.join(' || ')}`);
await shot('10b-distribuicao');

// Distorção da torre: the sample has one anemometer per height, so no pair (the 200 real days have four).
await pickMenu('Analisar', 'Distorção da torre');
await waitFor(`${ACTIVE}.querySelector('.analysis-output .note')`, 'tower distortion');
const distortionNote = await evaluate(`${ACTIVE}.querySelector('.analysis-output .note').innerText`);
console.log(`tower distortion: ${distortionNote}`);
if (!distortionNote.startsWith('Nenhum par')) problems.push(`tower distortion: ${distortionNote}`);

// Turbulence: TI by speed with the IEC curves, then by height as a table.
await pickMenu('Analisar', 'Turbulência');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'turbulence facts');
console.log(`turbulence: ${await analysisFacts()}`);
await shot('11b-turbulencia');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Exibir a turbulência')).querySelector('select')`, 'height');
await sleep(SETTLE_MS);
await clickText('Tabela', '.format button', ACTIVE);
await sleep(SHORT_SETTLE_MS);
console.log(`turbulence by height: ${flat(await evaluate(`${ACTIVE}.querySelector('table')?.innerText`))}`);
await shot('11c-turbulencia-altura');

// Extreme winds: monthly maxima with the interval, then independent storms.
await pickMenu('Analisar', 'Ventos extremos');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'extreme wind facts');
await evaluate(`[...${ACTIVE}.querySelectorAll('label.check')].find(l => l.innerText.includes('Intervalo de confiança')).querySelector('input').click()`);
await sleep(LONG_SETTLE_MS);
console.log(`extremes, monthly maxima: ${await analysisFacts()}`);
await shot('11d-ventos-extremos');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Método')).querySelector('select')`, 'independent_storms');
await sleep(LONG_SETTLE_MS);
console.log(`extremes, independent storms: ${await analysisFacts()}`);

// Scatter of the two speeds, and the monthly means table.
await pickMenu('Analisar', 'Dispersão');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'scatter facts');
console.log(`scatter: ${await analysisFacts()}`);
await shot('11e-dispersao');
await pickMenu('Analisar', 'Tabelas');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'monthly table facts');
console.log(`monthly table: ${await analysisFacts()}`);
await clickText('Tabela', '.format button', ACTIVE);
await shot('11f-tabela-medias-mensais');

// Flag the visible segment by hand, undo, check redo.
await pickMenu('Flag', 'Flag manual…');
await waitFor(`${ACTIVE}.querySelector('.flag-controls')`, 'flag controls');
const buttonEnabled = (text) => evaluate(`!${`[...${ACTIVE}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`}.disabled`);
if (!(await clickText('Aplicar flag ao segmento', 'button', ACTIVE))) problems.push('flag manual: apply disabled');
await sleep(SETTLE_MS);
const undoAfterFlag = await buttonEnabled('Desfazer');
await clickText('Desfazer', 'button', ACTIVE);
await sleep(SETTLE_MS);
console.log(`undo enabled after flag: ${undoAfterFlag}; redo enabled after undo: ${await buttonEnabled('Refazer')}`);
await shot('12-flag-manual');

// The values table: drag over six rows, flag them, see the tinted cells, undo.
await waitFor(`${ACTIVE}.querySelectorAll('.values-pane tbody td').length > 0`, 'values table');
const valueRows = `[...${ACTIVE}.querySelectorAll('.values-pane tbody tr')].filter(r => r.cells.length)`;
const tintedRows = `${valueRows}.filter(r => [...r.cells].some(c => c.style.background)).length`;
const tintedBefore = await evaluate(tintedRows);
// One event per task, as a real drag: React commits the drag between them.
for (const [row, type] of [[TABLE_DRAG_FROM, 'mousedown'], [TABLE_DRAG_TO, 'mouseover'], [TABLE_DRAG_TO, 'mouseup']]) {
  await evaluate(`${valueRows}[${row}].dispatchEvent(new MouseEvent('${type}', { bubbles: true }))`);
  await sleep(SHORT_SETTLE_MS);
}
await sleep(SETTLE_MS);
const tableSegment = await evaluate(`[...${ACTIVE}.querySelectorAll('.flag-controls .segment input')].map(i => i.value).join(' a ')`);
await clickText('Aplicar flag ao segmento', 'button', ACTIVE);
await sleep(SETTLE_MS);
const tintedAfter = await evaluate(tintedRows);
console.log(`values table: header ${flat(await evaluate(`${ACTIVE}.querySelector('.values-pane thead').innerText`))}; dragged ${tableSegment}; tinted rows ${tintedBefore} → ${tintedAfter}`);
if (tintedAfter - tintedBefore !== TABLE_DRAG_TO - TABLE_DRAG_FROM + 1) problems.push(`values table: ${tintedAfter - tintedBefore} rows tinted by the flag`);
await shot('12b-flag-manual-tabela');
await clickText('Desfazer', 'button', ACTIVE);
await sleep(SETTLE_MS);

// A value edit from Revisar: double the 100 m speed, read the histogram, undo.
await pickMenu('Revisar', 'Escala e offset…');
await waitFor(`document.querySelector('.modal')`, 'scale and offset dialog');
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.trim() === ${JSON.stringify(RULE_COLUMN)}).querySelector('input').click()`);
await setValue(`[...document.querySelectorAll('.modal label')].find(l => l.innerText.startsWith('Escala')).querySelector('input')`, SPEED_SCALE);
await sleep(SHORT_SETTLE_MS);
await shot('12b-escala-e-offset');
if (!(await clickText('OK', '.modal button'))) problems.push('scale and offset: OK disabled');
await sleep(SETTLE_MS);
await clickText('Histograma', '.windowtab span');
await sleep(LONG_SETTLE_MS);
console.log(`histogram, speed doubled: ${await analysisFacts()}`);
await pickMenu('Editar', 'Desfazer');
await sleep(LONG_SETTLE_MS);
console.log(`histogram, undone: ${await analysisFacts()}`);

// A flag rule: speeds at or above 20 m/s at 100 m, flag "Inválido".
await pickMenu('Flag', 'Flag por regras…');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.trim().startsWith('Máximo')).querySelector('input')`, RULE_MAX_SPEED);
await evaluate(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.trim() === ${JSON.stringify(RULE_COLUMN)}).querySelector('input').click()`);
await sleep(SHORT_SETTLE_MS);
await clickText('Adicionar regra', 'button', ACTIVE);
await sleep(SHORT_SETTLE_MS);
// Testar first: what the rule would flag, with nothing flagged (Desfazer stays off).
await clickText('Testar todas', 'button', ACTIVE);
await sleep(LONG_SETTLE_MS);
const testHeading = await evaluate(`${ACTIVE}.querySelector('.runsummary h4')?.innerText ?? ''`);
const undoAfterTest = await evaluate(`[...document.querySelectorAll('.toolbar button')].find(b => b.title.startsWith('Desfazer'))?.disabled`);
console.log(`rules test: ${testHeading} | ${flat(await evaluate(`${ACTIVE}.querySelector('.runsummary tbody')?.innerText ?? ''`))}`);
if (!testHeading.startsWith('Teste') || undoAfterTest !== true) problems.push(`rules test: ${testHeading}, undo ${undoAfterTest}`);
await clickText('Executar todas', 'button', ACTIVE);
await sleep(LONG_SETTLE_MS);
console.log(`rules window: ${flat(await evaluate(`${ACTIVE}.innerText`)).slice(0, PRINT_CHARS)}`);
await shot('13-flag-regras');

// A window test: the temperature's weekly cycle against the record's (the sample keeps its clock).
const ruleLabel = (text) => `[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.trim() === ${JSON.stringify(text)}).querySelector('input')`;
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.trim().startsWith('Teste')).querySelector('select')`, 'clock_shift');
await sleep(SHORT_SETTLE_MS);
await evaluate(`${ruleLabel(RULE_COLUMN)}.click()`);
await evaluate(`${ruleLabel(TEMPERATURE_COLUMN)}.click()`);
await sleep(SHORT_SETTLE_MS);
if (!(await clickText('Adicionar regra', 'button', ACTIVE))) problems.push('clock shift rule: not added');
await sleep(SHORT_SETTLE_MS);
await clickText('Executar todas', 'button', ACTIVE);
await sleep(LONG_SETTLE_MS);
console.log(`rules after the clock shift: ${flat(await evaluate(`[...${ACTIVE}.querySelectorAll('table')][0]?.innerText`))}`);

// Tower shading, laid out as the original's: the sample has one anemometer per height, so the
// detected pattern has no pair (a mast with twin anemometers has); by the boom, one typed in the window at 90°
// shades the first speed sensor with the wind from 255° to 285°, and the rule says so.
await pickMenu('Flag', 'Sombra da torre…');
await waitFor(`${ACTIVE}.querySelector('.shading-window .note')`, 'tower shading');
const shadingNote = await evaluate(`${ACTIVE}.querySelector('.shading-window .note').innerText`);
console.log(`tower shading: ${shadingNote}`);
if (!shadingNote.startsWith('Nenhum par')) problems.push(`tower shading: ${shadingNote}`);
await evaluate(`[...${ACTIVE}.querySelectorAll('.shading-basis label.check')].find(l => l.innerText.trim() === 'orientação da lança').querySelector('input').click()`);
await waitFor(`${ACTIVE}.querySelector('.shading-details')`, 'the boom details');
await setValue(`[...${ACTIVE}.querySelectorAll('.shading-details label')].find(l => l.innerText.startsWith('Orientação da lança')).querySelector('input')`, String(BOOM_DEG));
await waitFor(`${ACTIVE}.querySelector('.shading-rules .rule-lines li:not(.hint)')`, 'the boom rule');
const boomLine = flat(await evaluate(`${ACTIVE}.querySelector('.shading-details .hint').innerText + ' | ' + ${ACTIVE}.querySelector('.shading-rules .rule-lines').innerText`));
console.log(`tower shading by the boom: ${boomLine}`);
if (!boomLine.includes(`${BOOM_SHADED_DEG - BOOM_HALF_WIDTH_DEG}° a ${BOOM_SHADED_DEG + BOOM_HALF_WIDTH_DEG}°`)) problems.push(`tower shading by the boom: ${boomLine}`);
await shot('14b-sombra-pela-lanca');
await setValue(`[...${ACTIVE}.querySelectorAll('.shading-details label')].find(l => l.innerText.startsWith('Orientação da lança')).querySelector('input')`, '');
await sleep(SHORT_SETTLE_MS);
// A calculated column, the average of the two speeds (read with default filtering), added in Configurar dataset.
await pickMenu('Revisar', 'Configurar dataset…');
await waitFor(`document.querySelector('.modal')`, 'configure dialog');
await clickText('Colunas', '.dialog-tabs button');
await setValue(`document.querySelector('.modal .calculated-form input')`, CALCULATED_COLUMN);
await evaluate(`[...document.querySelectorAll('.modal .calculated-form label.check')].filter(l => ['Vel 100m Média', 'Vel 80m Média'].includes(l.innerText.trim())).forEach(l => l.querySelector('input').click())`);
await sleep(SHORT_SETTLE_MS);
if (!(await clickText('Adicionar coluna', '.modal button'))) problems.push('calculated column: not added');
await sleep(SHORT_SETTLE_MS);
// And the difference 100 m − 80 m as an expression; a typo first shows where it fails.
const formField = (label) => `[...document.querySelectorAll('.modal .calculated-form > label')].find(l => l.firstChild.textContent.trim() === ${JSON.stringify(label)}).querySelector('select, input, textarea')`;
await setValue(formField('Nome'), EXPRESSION_COLUMN);
await setValue(formField('Classe'), 'expression');
await setValue(formField('Expressão'), 'X = A +;');
await waitFor(`document.querySelector('.modal .calculated-form .note.err')`, 'the expression error');
console.log(`expression error: ${await evaluate(`document.querySelector('.modal .calculated-form .note.err').innerText`)}`);
await setValue(formField('Expressão'), 'X = A - B;');
await setValue(formField('A'), 'Vel 100m Média');
await setValue(formField('B'), 'Vel 80m Média');
await waitFor(`!document.querySelector('.modal .calculated-form .note.err') && [...document.querySelectorAll('.modal button')].some(b => b.textContent.trim() === 'Adicionar coluna' && !b.disabled)`, 'the expression accepted');
await clickText('Adicionar coluna', '.modal button');
await sleep(SHORT_SETTLE_MS);
// And the sun's altitude, from the site the sample carries.
await setValue(formField('Nome'), SOLAR_COLUMN);
await setValue(formField('Classe'), 'solar');
await setValue(formField('Variável'), 'altitude');
await clickText('Adicionar coluna', '.modal button');
await sleep(SHORT_SETTLE_MS);
console.log(`calculated: ${flat(await evaluate(`[...document.querySelectorAll('.modal .calculated .extrapolation span')].map(s => s.innerText).join(' / ')`))}`);
await shot('16a-coluna-calculada');
await clickText('OK', '.modal button');
await waitFor(`!document.querySelector('.modal') && !document.querySelector('.statusbar').innerText.includes('Reaplicando')`, 'the calculated column', IMPORT_TIMEOUT_MS);
await pickMenu('Analisar', 'Histograma');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'the histogram', IMPORT_TIMEOUT_MS);
/** The histogram of `column`: the facts once they differ from those shown before. */
const histogramOf = async (column) => {
  const before = await analysisFacts();
  await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Coluna')).querySelector('select')`, column);
  await waitFor(`(() => { const facts = ${ACTIVE}.querySelector('.facts')?.innerText.replace(/\\n/g, ' '); return facts && facts !== ${JSON.stringify(before)}; })()`, `the histogram of ${column}`, IMPORT_TIMEOUT_MS);
  return analysisFacts();
};
const calculatedFacts = await histogramOf(CALCULATED_COLUMN);
console.log(`histogram of the calculated average: ${calculatedFacts}`);
// Where 100 m is flagged the average takes 80 m alone: it has data wherever 80 m has.
const countOf = (facts) => Number(facts.match(/^N ([\d.]+)/)[1].replace(/\./g, ''));
const speedFacts = await histogramOf('Vel 80m Média');
console.log(`histogram of 80 m: ${speedFacts}`);
if (countOf(calculatedFacts) < countOf(speedFacts)) problems.push(`calculated column: ${calculatedFacts} vs 80 m ${speedFacts}`);
const expressionFacts = await histogramOf(EXPRESSION_COLUMN);
console.log(`histogram of the expression: ${expressionFacts}`);
const solarFacts = await histogramOf(SOLAR_COLUMN);
console.log(`histogram of the sun's altitude: ${solarFacts}`);
if (!(countOf(solarFacts) > 0)) problems.push(`solar column: ${solarFacts}`);

// Flag por dispersão: drag a rectangle over the high speeds, flag what it holds, undo.
await pickMenu('Flag', 'Flag por dispersão…');
await waitFor(`${ACTIVE}.querySelector('.flagscatter canvas')`, 'the flag scatter');
await sleep(SETTLE_MS * 2);
const [plotLeft, plotTop, plotWidth, plotHeight] = await evaluate(`(() => { const r = ${ACTIVE}.querySelector('.flagscatter .chart').getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
const dragFrom = [plotLeft + plotWidth * 0.75, plotTop + plotHeight * 0.08];
const dragTo = [plotLeft + plotWidth * 0.96, plotTop + plotHeight * 0.4];
await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: dragFrom[0], y: dragFrom[1] });
await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: dragFrom[0], y: dragFrom[1], button: 'left', clickCount: 1 });
for (let step = 1; step <= DRAG_MOVES; step++) {
  const along = step / DRAG_MOVES;
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: dragFrom[0] + (dragTo[0] - dragFrom[0]) * along, y: dragFrom[1] + (dragTo[1] - dragFrom[1]) * along, button: 'left', buttons: 1 });
}
await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: dragTo[0], y: dragTo[1], button: 'left', clickCount: 1 });
await sleep(SETTLE_MS * 2);
const scatterSelected = await evaluate(`${ACTIVE}.querySelector('.selectioncount b').innerText`);
console.log(`flag por dispersão: ${scatterSelected} passos selecionados`);
if (!(Number(scatterSelected.replace(/\./g, '')) > 0)) problems.push('flag scatter: the rectangle selected nothing');
await shot('15b-flag-por-dispersao');
await clickText('Aplicar flag à seleção', 'button', ACTIVE);
await sleep(SETTLE_MS * 2);
if ((await evaluate(`${ACTIVE}.querySelector('.selectioncount b').innerText`)) !== '0') problems.push('flag scatter: the flagged points still show');
await pickMenu('Editar', 'Desfazer');
await sleep(SETTLE_MS);

await pickMenu('Flag', 'Inspecionar e remover flags…');
await sleep(SETTLE_MS);
const inspectText = () => evaluate(`[...document.querySelectorAll('.window')].find(w => w.querySelector('details.statistics'))?.innerText`);
console.log(`statistics: ${flat(await inspectText()).slice(0, PRINT_CHARS)}`);
await shot('14-inspecionar-flags');

// Configure the dataset: the dialog opens on a double click and shows the site.
await evaluate(`document.querySelector('.datasets tbody tr').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
await waitFor(`document.querySelector('.modal')`, 'configure dialog');
console.log(`site in the dialog: ${await evaluate(`[...document.querySelectorAll('.modal .properties input')].slice(1, 3).map(i => i.value || i.placeholder).join(' / ')`)}`);
await shot('15-configurar');
await clickText('Colunas', '.dialog-tabs button');
await shot('16-configurar-colunas');
// Colunas primárias: the lowest speed as primary moves the dataset's height (80 m); then back to the default.
await clickText('Colunas primárias', '.dialog-tabs button');
console.log(`primary columns: ${flat(await evaluate(`document.querySelector('.modal .primary-columns tbody').innerText`))}`);
const speedPrimary = `document.querySelector('.modal select[aria-label="Coluna primária de Velocidade"]')`;
await setValue(speedPrimary, 'lowest');
await shot('16b-configurar-colunas-primarias');
await clickText('OK', '.modal button');
const datasetHeight = `(() => { const position = [...document.querySelectorAll('.datasets thead th')].findIndex(th => th.textContent.trim() === 'Altura'); return document.querySelectorAll('.datasets tbody tr')[0].cells[position].textContent; })()`;
await waitFor(`${datasetHeight} === ${JSON.stringify(PRIMARY_LOWEST_HEIGHT)}`, 'the lowest speed as primary', IMPORT_TIMEOUT_MS);
console.log(`dataset height with the lowest speed as primary: ${await evaluate(datasetHeight)}`);
await pickMenu('Revisar', 'Configurar dataset…');
await clickText('Colunas primárias', '.dialog-tabs button');
await setValue(speedPrimary, 'default');
await clickText('OK', '.modal button');
await waitFor(`${datasetHeight} !== ${JSON.stringify(PRIMARY_LOWEST_HEIGHT)}`, 'the default primary speed again', IMPORT_TIMEOUT_MS);
await sleep(SETTLE_MS);

// Altura de deslocamento: a constant 10 m reaches the shear window (effective heights), then none again.
const setDisplacement = async (mode, heightM) => {
  await pickMenu('Revisar', 'Configurar dataset…');
  await clickText('Altura de deslocamento', '.dialog-tabs button');
  await evaluate(`[...document.querySelectorAll('.modal .displacement label.check')].find(l => l.innerText.trim() === ${JSON.stringify(mode)}).querySelector('input').click()`);
  await sleep(SHORT_SETTLE_MS);
  if (heightM !== undefined) await setValue(`[...document.querySelectorAll('.modal .displacement label')].find(l => l.innerText.startsWith('Altura (m)')).querySelector('input')`, heightM);
};
await setDisplacement('Constante', DISPLACEMENT_M);
await shot('16c-configurar-deslocamento');
await clickText('OK', '.modal button');
await waitFor(`!document.querySelector('.statusbar').innerText.includes('Reaplicando')`, 'displacement re-read', IMPORT_TIMEOUT_MS);
await pickMenu('Analisar', 'Cisalhamento do vento');
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.includes('deslocamento')`, 'the displacement in the shear window', IMPORT_TIMEOUT_MS);
console.log(`shear with ${DISPLACEMENT_M} m of displacement: ${await analysisFacts()}`);
// By sector: north 30 m and the rest 10 m; Calcular weighs them by how often each sector blows.
await setDisplacement('Por setor da direção');
await setValue(`document.querySelector('.modal .displacement .grid-inputs input')`, NORTH_DISPLACEMENT_M);
await clickText('Calcular', '.modal button');
await sleep(SETTLE_MS);
console.log(`displacement backup weighted by sector: ${await evaluate(`[...document.querySelectorAll('.modal .displacement label')].find(l => l.innerText.startsWith('Reserva')).querySelector('input').value`)} m`);
await shot('16d-deslocamento-por-setor');
await evaluate(`[...document.querySelectorAll('.modal .displacement label.check')].find(l => l.innerText.trim() === 'Nenhuma').querySelector('input').click()`);
await sleep(SHORT_SETTLE_MS);
await clickText('OK', '.modal button');
await waitFor(`!${ACTIVE}.querySelector('.facts')?.innerText.includes('deslocamento')`, 'no displacement again', IMPORT_TIMEOUT_MS);
console.log(`shear without displacement: ${await analysisFacts()}`);

// Exibição de colunas: systematic names, by type then height, speed SDs hidden, colours by type;
// the time series legend and an analysis list follow it. Then back to the columns as read.
await pickMenu('Revisar', 'Configurar dataset…');
await clickText('Exibição de colunas', '.dialog-tabs button');
await evaluate(`[...document.querySelectorAll('.modal .column-display label.check')].find(l => l.innerText.startsWith('Sistemáticos')).querySelector('input').click()`);
await setValue(modalSelect('Ordem'), 'type_then_height');
await evaluate(`document.querySelector('.modal input[aria-label="Mostrar Velocidade, Desvio padrão"]').click()`);
await evaluate(`[...document.querySelectorAll('.modal .column-display label.check')].find(l => l.innerText.startsWith('Uma cor por tipo')).querySelector('input').click()`);
await sleep(SHORT_SETTLE_MS);
console.log(`column display preview: ${flat(await evaluate(`document.querySelector('.modal .column-preview').innerText`))}`);
await shot('16e-exibicao-de-colunas');
await clickText('OK', '.modal button');
await clickText('Série temporal', '.windowtab span');
await sleep(SETTLE_MS);
const legend = flat(await evaluate(`${ACTIVE}.querySelector('.ts-legend').innerText`));
console.log(`time series legend: ${legend}`);
if (legend.includes(HIDDEN_COLUMN) || !legend.includes(SYSTEMATIC_NAME)) problems.push(`column display: legend ${legend}`);
await shot('16f-serie-com-exibicao');
await pickMenu('Analisar', 'Histograma');
const histogramChoices = flat(await evaluate(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Coluna')).querySelector('select').innerText`));
console.log(`histogram columns: ${histogramChoices}`);
if (histogramChoices.includes(HIDDEN_COLUMN)) problems.push('column display: a hidden column in the histogram list');
await pickMenu('Revisar', 'Configurar dataset…');
await clickText('Exibição de colunas', '.dialog-tabs button');
await clickText('Como no arquivo', '.modal .column-display button');
await clickText('OK', '.modal button');
await sleep(SHORT_SETTLE_MS);
// Back to the flag inspection, the window the steps below read.
await clickText('Inspecionar e remover flags', '.windowtab span');

// Save the workbook (a download), then open it again: its dataset joins the open one.
await pickMenu('Arquivo', 'Salvar workbook');
let projectFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !projectFile; attempt++) {
  await sleep(POLL_MS);
  projectFile = readdirSync(downloads).find((name) => name.endsWith('.wproj'));
}
if (!projectFile) throw new Error('no workbook downloaded');
console.log(`saved ${projectFile}: ${statSync(join(downloads, projectFile)).size} bytes; title: ${await evaluate('document.title')}`);
const { root: again } = await send('DOM.getDocument', { depth: -1 });
const { nodeId: projectInput } = await send('DOM.querySelector', { nodeId: again.nodeId, selector: 'input[type=file][accept=".wproj"]' });
await send('DOM.setFileInputFiles', { nodeId: projectInput, files: [join(downloads, projectFile)] });
await waitFor(`${datasetRows} === 2`, 'second dataset from the workbook', IMPORT_TIMEOUT_MS);
await sleep(LONG_SETTLE_MS);
await evaluate(`document.querySelectorAll('.datasets tbody tr')[1].click()`);
await sleep(SETTLE_MS);
console.log(`reopened statistics: ${flat(await inspectText()).slice(0, PRINT_CHARS)}`);
await pickMenu('Flag', 'Flag por regras…');
console.log(`reopened rules: ${flat(await evaluate(`${ACTIVE}.innerText`)).slice(0, PRINT_CHARS)}`);
await clickText('Gráficos-resumo', '.windowtab span');
await shot('17-workbook-reaberto');

// Long-term adjustment: a five-year hourly reference made from the sample
// (2023 as 0.8 · the hourly mean + 0.3, earlier years the same hours scaled
// a little), imported as another dataset.
const sampleLines = readFileSync(samplePath, 'utf8').split(/\r?\n/).slice(SAMPLE_PREAMBLE_LINES + 1).filter(Boolean);
const hourly = new Map();
const hourDirection = new Map();
for (const line of sampleLines) {
  const fields = line.split(';');
  const [stamp, speed] = fields;
  const value = Number(speed.replace(',', '.'));
  if (Number.isFinite(value) && value > MISSING_CODE) hourly.set(stamp.slice(0, HOUR_PREFIX), [...(hourly.get(stamp.slice(0, HOUR_PREFIX)) ?? []), value]);
  // The hour's first direction, turned: the long-term window should find the turn back.
  const direction = Number(fields[SAMPLE_DIRECTION_FIELD].replace(',', '.'));
  if (!hourDirection.has(stamp.slice(0, HOUR_PREFIX)) && Number.isFinite(direction) && direction > MISSING_CODE) {
    hourDirection.set(stamp.slice(0, HOUR_PREFIX), (direction - REFERENCE_TURN_DEG + 360) % 360);
  }
}
const referenceRows = ['Data/Hora;Vel 50m Média;Dir 50m'];
for (const [yearOffset, factor] of REFERENCE_YEARS.entries()) {
  for (const [hour, values] of hourly) {
    const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
    const year = Number(hour.slice(YEAR_FROM, YEAR_TO)) - (REFERENCE_YEARS.length - 1 - yearOffset);
    if (hour.startsWith('29/02')) continue;
    const direction = hourDirection.has(hour) ? hourDirection.get(hour).toFixed(1).replace('.', ',') : '';
    referenceRows.push(`${hour.slice(0, YEAR_FROM)}${year}${hour.slice(YEAR_TO)}:00;${(factor * (0.8 * mean + 0.3)).toFixed(2).replace('.', ',')};${direction}`);
  }
}
const referencePath = join(outDir, 'referencia_longo_prazo.csv');
writeFileSync(referencePath, referenceRows.join('\n'));
const { root: withReference } = await send('DOM.getDocument', { depth: -1 });
const { nodeId: addInput } = await send('DOM.querySelector', { nodeId: withReference.nodeId, selector: '.minibar input[type=file]' });
await send('DOM.setFileInputFiles', { nodeId: addInput, files: [referencePath] });
await waitFor(`${datasetRows} === 3`, 'reference dataset', IMPORT_TIMEOUT_MS);
await pickMenu('Revisar', 'Ajuste de longo prazo…');
// Importing selected the reference: the site is the tower, the first dataset.
const longTermSelect = (label) => `[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith(${JSON.stringify(label)})).querySelector('select')`;
await setValue(longTermSelect('Local'), await evaluate(`${longTermSelect('Local')}.options[0].value`));
await sleep(SHORT_SETTLE_MS);
await setValue(longTermSelect('Referência'), await evaluate(`[...${longTermSelect('Referência')}.options].find(o => o.textContent.includes('referencia')).value`));
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'long-term facts', IMPORT_TIMEOUT_MS);
await sleep(SETTLE_MS);
console.log(`long term: ${await analysisFacts()}`);
await setValue(longTermSelect('Setores'), '12');
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.includes('setores')`, 'sector fits', IMPORT_TIMEOUT_MS);
await sleep(SETTLE_MS);
console.log(`long term, by sector: ${await analysisFacts()}`);
await shot('18-ajuste-longo-prazo');
await clickText('Tabela', '.format button', ACTIVE);
await sleep(SHORT_SETTLE_MS);
console.log(`long term, recommendation: ${await evaluate(`${ACTIVE}.querySelector('.tablewrap .hint')?.innerText ?? ''`)}`);
await evaluate(`[...${ACTIVE}.querySelectorAll('label.check')].find(l => l.innerText.includes('Intervalo de confiança')).querySelector('input').click()`);
await waitFor(`${ACTIVE}.querySelector('.tablewrap table')?.innerText.includes('–')`, 'long-term intervals', IMPORT_TIMEOUT_MS);
console.log(`long term, methods: ${flat(await evaluate(`${ACTIVE}.querySelector('.tablewrap table').innerText`))}`);
await shot('18b-ajuste-longo-prazo-tabela');
// The original's options: four groups of months crossed with the 12 sectors, a 3 m/s cutoff (its own group), lines through 0.
await setValue(longTermSelect('Divisões anuais'), '4');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Velocidade de corte')).querySelector('input')`, '3');
await evaluate(`[...${ACTIVE}.querySelectorAll('label.check')].find(l => l.innerText.includes('Intercepto zero')).querySelector('input').click()`);
await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText.includes('52 subdivisões')`, 'the subdivided fits', IMPORT_TIMEOUT_MS);
const subdivided = flat(await evaluate(`${ACTIVE}.querySelector('.tablewrap table').innerText`));
console.log(`long term, divided: ${await analysisFacts()} | ${subdivided.slice(0, PRINT_CHARS)}`);
if (!subdivided.includes('de 52')) problems.push(`long term options: ${subdivided.slice(0, PRINT_CHARS)}`);
const mtsRow = subdivided.split(' | ').find((row) => row.startsWith('Matrix Time Series')) ?? '';
console.log(`long term, MTS: ${mtsRow}`);
if (!mtsRow) problems.push('long term: no matrix time series');
// The optimal time offset: the reference was built from the tower's own hours, so no shift.
await clickText('Gráfico', '.format button', ACTIVE);
await clickText('Deslocamento de tempo ótimo', 'button', ACTIVE);
await waitFor(`${ACTIVE}.querySelector('.time-offset table')`, 'the time offset analysis', IMPORT_TIMEOUT_MS);
const offsetText = flat(await evaluate(`${ACTIVE}.querySelector('.time-offset').innerText`));
console.log(`time offset: ${offsetText}`);
if (!offsetText.includes('máxima sem deslocamento')) problems.push(`time offset: ${offsetText}`);
await shot('18c-deslocamento-de-tempo');

// Vertical extrapolation of the tower to 120 m, from 80 and 100 m; then its histogram.
await evaluate(`document.querySelectorAll('.datasets tbody tr')[0].click()`);
await sleep(SHORT_SETTLE_MS);
// Along each step's line of best fit through both speeds, α held in the original's −0.25 to 1.25,
// the new columns flagged (Inválido, the first flag): the histogram then has nothing until Desfazer.
await pickMenu('Revisar', 'Extrapolação vertical…');
await setValue(modalSelect('Alturas de destino'), EXTRAPOLATION_HEIGHT);
await setValue(modalSelect('Extrapolar a partir de'), 'best_fit');
const modalCheck = (start) => `[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.trim().startsWith(${JSON.stringify(start)})).querySelector('input')`;
await evaluate(`${modalCheck('Restringir α')}.click()`);
await sleep(SHORT_SETTLE_MS);
if (!(await clickText('Adicionar', '.modal button'))) problems.push('extrapolation: not added');
await sleep(SHORT_SETTLE_MS);
const extrapolationLine = flat(await evaluate(`document.querySelector('.modal .extrapolation span').innerText`));
console.log(`extrapolation: ${extrapolationLine}`);
if (!extrapolationLine.includes('reta de melhor ajuste') || !extrapolationLine.includes('α entre')) problems.push(`extrapolation: ${extrapolationLine}`);
await evaluate(`${modalCheck('Marcar as novas colunas')}.click()`);
await clickText('OK', '.modal button');
await waitFor(`!document.querySelector('.statusbar').innerText.includes('Reaplicando')`, 'extrapolation re-read', IMPORT_TIMEOUT_MS);
await sleep(SETTLE_MS);
await pickMenu('Analisar', 'Histograma');
await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Coluna')).querySelector('select')`, EXTRAPOLATED_COLUMN);
await waitFor(`${ACTIVE}.querySelector('.note.err')`, 'the flagged extrapolation');
console.log(`histogram of the flagged extrapolation: ${await evaluate(`${ACTIVE}.querySelector('.note.err').innerText`)}`);
await pickMenu('Editar', 'Desfazer');
await waitFor(`${ACTIVE}.querySelector('.facts')`, 'the extrapolation unflagged', IMPORT_TIMEOUT_MS);
console.log(`histogram at 120 m, extrapolated: ${await analysisFacts()}`);
const extrapolatedIds = await evaluate(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.startsWith('Coluna')).querySelector('select').innerText`);
if (!extrapolatedIds.includes(`${EXTRAPOLATED_COLUMN} DP`)) problems.push('extrapolation: no synthesized SD');
await shot('19-extrapolacao-120m');

// Reconstruct the 100 m speed's gaps from 80 m; the filled steps carry the flag "Reconstruído".
await pickMenu('Revisar', 'Reconstruir lacunas…');
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.trim() === 'Vel 100m Média').querySelector('input').click()`);
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.includes('De outra altura')).querySelector('input').click()`);
await sleep(SHORT_SETTLE_MS);
await setValue(modalSelect('Velocidade de origem'), 'Vel 80m Média');
await sleep(SHORT_SETTLE_MS);
if (!(await clickText('OK', '.modal button'))) problems.push('reconstruction: OK disabled');
await sleep(LONG_SETTLE_MS);
await pickMenu('Flag', 'Inspecionar e remover flags…');
await sleep(SETTLE_MS);
const reconstructedRow = await evaluate(`[...${ACTIVE}.querySelectorAll('tr')].find(r => r.innerText.startsWith('Vel 100m Média'))?.innerText ?? ''`);
console.log(`after reconstruction, 100 m flags: ${flat(reconstructedRow)} | header: ${flat(await evaluate(`${ACTIVE}.querySelector('thead')?.innerText ?? ''`))}`);
await shot('20-reconstrucao');

// A workflow from the tower (its edits, its rules, a histogram and the turbulence), saved and
// run on the other datasets: the reopened copy takes it all, the reference stops at the
// reconstruction (it has no 100 m column). The saved file runs on the CSV with `wind run`.
await pickMenu('Fluxos', 'Gerenciar fluxos…');
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.includes('Turbulência')).querySelector('input').click()`);
await sleep(SHORT_SETTLE_MS);
await clickText('Salvar fluxo…', '.modal button');
let workflowFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !workflowFile; attempt++) {
  await sleep(POLL_MS);
  workflowFile = readdirSync(downloads).find((name) => name.endsWith('.fluxo.json'));
}
if (!workflowFile) throw new Error('no workflow downloaded');
const savedWorkflow = JSON.parse(readFileSync(join(downloads, workflowFile), 'utf8'));
console.log(`workflow: ${savedWorkflow.ops.map((op) => op.op).join(', ')} | ${savedWorkflow.analyses.map((step) => step.analysis).join(', ')}`);
await clickText('Executar nos datasets marcados', '.modal button');
await waitFor(`document.querySelector('.modal .workflow-outcomes')`, 'workflow outcomes', IMPORT_TIMEOUT_MS);
console.log(`workflow outcomes: ${flat(await evaluate(`document.querySelector('.modal .workflow-outcomes tbody').innerText`))}`);
await shot('21-fluxos');
await clickText('Fechar', '.modal button');
if (windBinary) {
  const outcomes = JSON.parse(execFileSync(windBinary, ['run', samplePath, '--workflow', join(downloads, workflowFile)], { encoding: 'utf8' }));
  const [outcome] = outcomes;
  console.log(`wind run: ${outcome.opsApplied}/${savedWorkflow.ops.length} edits, ${outcome.opError?.code ?? 'no error'}; ${outcome.results.map((step) => `${step.analysis} ${step.error ? step.error.code : 'ok'}`).join(', ')}`);
  if (outcome.opError || outcome.results.some((step) => step.error)) problems.push('wind run: the workflow did not run clean on the CSV');
}

// Definir flags on the tower: add "Suspeito" (kept in analyses), delete "Gelo"; the rules keep their flag.
await pickMenu('Flag', 'Definir flags…');
await waitFor(`document.querySelector('.modal .define-flags')`, 'define flags dialog');
await clickText('Adicionar', '.modal button');
await sleep(SHORT_SETTLE_MS);
await setValue(modalSelect('Nome'), NEW_FLAG_NAME);
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.includes('Incluir nos cálculos')).querySelector('input').click()`);
await evaluate(`[...document.querySelectorAll('.modal .define-flags tbody tr')].find(r => r.innerText.includes(${JSON.stringify(DELETED_FLAG)})).click()`);
await sleep(SHORT_SETTLE_MS);
await clickText('Excluir', '.modal button');
await sleep(SHORT_SETTLE_MS);
console.log(`define flags: ${flat(await evaluate(`document.querySelector('.modal .define-flags tbody').innerText`))} | ${flat(await evaluate(`document.querySelector('.modal .note.warn')?.innerText ?? 'no warning'`))}`);
await shot('22-definir-flags');
await clickText('OK', '.modal button');
// The core redefines the flags and the summaries are rebuilt before the dialog closes.
await waitFor(`!document.querySelector('.modal .define-flags')`, 'define flags to close').catch(() => {});
if (await evaluate(`!!document.querySelector('.modal .define-flags')`)) {
  problems.push(`define flags: the dialog did not close: ${await evaluate(`[...document.querySelectorAll('.modal .note, .errorbar')].map(n => n.innerText).join(' | ')`)}`);
}
await pickMenu('Flag', 'Inspecionar e remover flags…');
await sleep(SETTLE_MS);
const flagHeader = flat(await evaluate(`${ACTIVE}.querySelector('thead')?.innerText ?? ''`));
console.log(`flags after: ${flagHeader}`);
if (flagHeader.includes(DELETED_FLAG) || !flagHeader.includes(NEW_FLAG_NAME)) problems.push(`define flags: header ${flagHeader}`);
await pickMenu('Flag', 'Flag por regras…');
console.log(`rules after: ${flat(await evaluate(`${ACTIVE}.querySelector('tbody')?.innerText ?? ''`))}`);

// "<todos visíveis>": the histogram, the rose (as a table) and the shear of the three datasets at once;
// the reference, with one height, stays out of the shear.
const showAllVisible = async (analysis) => {
  await pickMenu('Analisar', analysis);
  await setValue(`${ACTIVE}.querySelector('.dataset-select select')`, 'all');
  await waitFor(`${ACTIVE}.querySelector('.analysis-output canvas, .analysis-output .note')`, `${analysis} of every visible dataset`, IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
};
await showAllVisible('Histograma');
console.log(`histogram, all visible: ${await analysisFacts()}`);
await shot('23-histograma-todos-visiveis');
await showAllVisible('Rosa dos ventos');
await clickText('Tabela', '.format button', ACTIVE);
await sleep(SHORT_SETTLE_MS);
const roseColumns = await evaluate(`${ACTIVE}.querySelectorAll('.multi-dataset thead th').length - 1`);
console.log(`rose table, all visible: ${roseColumns} datasets | ${flat(await evaluate(`${ACTIVE}.querySelector('.multi-dataset tbody tr')?.innerText ?? ''`))}`);
if (roseColumns !== ALL_VISIBLE_DATASETS) problems.push(`all visible: the rose table has ${roseColumns} datasets`);
await showAllVisible('Cisalhamento do vento');
console.log(`shear, all visible: ${flat(await evaluate(`${ACTIVE}.querySelector('.analysis-output').innerText`)).slice(0, PRINT_CHARS)}`);
await shot('23b-cisalhamento-todos-visiveis');

// Reanalysis files as downloaded: NASA POWER (header block, split time, WS50M) and ERA5 (u/v
// components at 100 m). Each opens with its wind recognised: 50 m and 100 m in the explorer.
const hours = Array.from({ length: REANALYSIS_HOURS }, (_, hour) => hour);
const powerRows = hours.map((hour) => `2023,1,${1 + Math.floor(hour / 24)},${hour % 24},${(6 + Math.sin(hour / 5)).toFixed(2)},${(90 + hour) % 360}`);
const powerPath = join(outDir, 'POWER_Point_Hourly.csv');
writeFileSync(powerPath, ['-BEGIN HEADER-', 'Location: Latitude  -5.1234   Longitude -36.4567', '-END HEADER-', 'YEAR,MO,DY,HR,WS50M,WD50M', ...powerRows].join('\n'));
const eraRows = hours.map((hour) => `2023-01-${String(1 + Math.floor(hour / 24)).padStart(2, '0')} ${String(hour % 24).padStart(2, '0')}:00:00,${(5 * Math.cos(hour / 7)).toFixed(3)},${(5 * Math.sin(hour / 7)).toFixed(3)}`);
const eraPath = join(outDir, 'era5_timeseries.csv');
writeFileSync(eraPath, ['valid_time,u100,v100', ...eraRows].join('\n'));
const { root: withReanalysis } = await send('DOM.getDocument', { depth: -1 });
const { nodeId: reanalysisInput } = await send('DOM.querySelector', { nodeId: withReanalysis.nodeId, selector: '.minibar input[type=file]' });
const datasetsBefore = await evaluate(datasetRows);
// Logger files in their makers' layouts: Ammonit Meteo-40 (statistics named Sensor;Evaluation;Statistic,
// heights in the ini part) and NRG SymphoniePRO (tab separated, a Sensor History before the table).
const loggerSteps = Array.from({ length: LOGGER_STEPS }, (_, step) => step);
const stamp10 = (step) => `2023-01-01 ${String(Math.floor(step / 6)).padStart(2, '0')}:${String((step % 6) * 10).padStart(2, '0')}:00`;
const ammonitPath = join(outDir, 'D401234_ammonit.csv');
writeFileSync(ammonitPath, [
  'Date/time,Thies80;wind_speed;Avg,Thies80;wind_speed;StdDev,Vane78;wind_direction;Avg,addr',
  ...loggerSteps.map((step) => `${stamp10(step)},${(6 + Math.sin(step / 4)).toFixed(2)},0.60,${(90 + step) % 360},${1024 + step}`),
  '', '[System]', 'timezone=UTC-03:00', 'latitude=-5.1234', 'longitude=-36.4567', '[Thies80;wind_speed]', 'sensor_height=80', '[Vane78;wind_direction]', 'sensor_height=78',
].join('\n'));
const symproPath = join(outDir, '001234_sympro_meas.txt');
writeFileSync(symproPath, [
  'SymphoniePRO Logger data file ----- Measurements', '', 'Site Properties', 'Latitude:\t-5.1234', 'Longitude:\t-36.4567', 'Time Zone:\tUTC-03:00', 'Sensor History',
  'Channel:\t1', 'Description:\tNRG Class1 Anem', 'Height:\t80.00', 'Channel:\t20', 'Description:\tNRG T60 Temperature', 'Height:\t2.00', 'Data',
  'Timestamp\tCh1_Anem_80.00m_N_Avg_m/s\tCh20_Analog_2.00m_N_Avg_C',
  ...loggerSteps.map((step) => `${stamp10(step)}\t${(6 + Math.cos(step / 4)).toFixed(2)}\t${(27 + Math.sin(step / 9)).toFixed(1)}`),
].join('\n'));
await send('DOM.setFileInputFiles', { nodeId: reanalysisInput, files: [powerPath, eraPath, ammonitPath, symproPath] });
await waitFor(`${datasetRows} === ${datasetsBefore + 4}`, 'the reanalysis and logger datasets', IMPORT_TIMEOUT_MS);
await sleep(SETTLE_MS);
const rowOf = (name) => `[...document.querySelectorAll('.datasets tbody tr')].find(r => r.innerText.includes(${JSON.stringify(name)}))`;
const heightOf = async (name) =>
  evaluate(`(() => { const position = [...document.querySelectorAll('.datasets thead th')].findIndex(th => th.textContent.trim() === 'Altura'); return ${rowOf(name)}.cells[position].textContent; })()`);
const [powerHeight, eraHeight] = [await heightOf('POWER_Point_Hourly'), await heightOf('era5_timeseries')];
console.log(`reanalysis: NASA POWER at ${powerHeight}, ERA5 at ${eraHeight}`);
if (powerHeight !== '50 m' || eraHeight !== '100 m') problems.push(`reanalysis: heights ${powerHeight} and ${eraHeight}`);
const zoneOf = async (name) =>
  evaluate(`(() => { const position = [...document.querySelectorAll('.datasets thead th')].findIndex(th => th.textContent.trim() === 'Fuso'); return ${rowOf(name)}.cells[position].textContent; })()`);
const loggers = { ammonit: [await heightOf('D401234_ammonit'), await zoneOf('D401234_ammonit')], sympro: [await heightOf('001234_sympro'), await zoneOf('001234_sympro')] };
console.log(`loggers: Ammonit at ${loggers.ammonit.join(', ')}; NRG at ${loggers.sympro.join(', ')}`);
if (loggers.ammonit.join() !== '80 m,UTC-03' || loggers.sympro.join() !== '80 m,UTC-03') problems.push(`loggers: ${JSON.stringify(loggers)}`);
await evaluate(`${rowOf('era5_timeseries')}.click()`);
await pickMenu('Revisar', 'Configurar dataset…');
await clickText('Colunas', '.dialog-tabs button');
console.log(`ERA5 columns: ${flat(await evaluate(`[...document.querySelectorAll('.modal .columns-table tbody td.id')].map(td => td.innerText).join(' | ')`))}`);
await shot('24-reanalise');
await clickText('Cancelar', '.modal button');

// The EPE file of the tower: named as EPE asks, the station on line 1, the error codes on line 3+.
await evaluate(`${rowOf('torre_exemplo')}.click()`);
await pickMenu('Arquivo', 'Exportar dados…');
await setValue(`[...document.querySelectorAll('.modal .epe label')].find(l => l.innerText.startsWith('Código da estação')).querySelector('input')`, EPE_STATION);
await shot('25-exportar-epe');
await clickText('Baixar arquivo EPE', '.modal button');
let epeFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !epeFile; attempt++) {
  await sleep(POLL_MS);
  epeFile = readdirSync(downloads).find((name) => name.startsWith(`${EPE_STATION}_`) && name.endsWith(EPE_EXTENSION));
}
if (!epeFile) throw new Error('no EPE file downloaded');
const epeLines = readFileSync(join(downloads, epeFile), 'utf8').split(/\r\n/);
const records = epeLines.indexOf('dados') + 1;
console.log(`EPE: ${epeFile} | ${epeLines.slice(0, 3).join(' | ')} | ${epeLines[records]}`);
const wellFormed = epeLines[0] === `Estação ${EPE_STATION}` && records > 0 && epeLines.includes('fimdados') && epeLines[records].split('|').length === EPE_FIELDS_WITH_END;
if (!wellFormed) problems.push(`EPE: ${epeLines.slice(0, 3).join(' / ')} / ${epeLines[records]}`);
// The Openwind met mast file of the same tower: MM 2.1, the header line, one row per step.
await clickText('Exportar Openwind', '.modal button');
let openwindFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !openwindFile; attempt++) {
  await sleep(POLL_MS);
  openwindFile = readdirSync(downloads).find((name) => name.endsWith('_openwind.csv'));
}
if (!openwindFile) throw new Error('no Openwind file downloaded');
const openwindLines = readFileSync(join(downloads, openwindFile), 'utf8').replace(/^﻿/, '').split('\n');
console.log(`Openwind: ${openwindFile} | ${openwindLines.slice(0, 9).join(' | ')}`);
if (openwindLines[0] !== 'MM,2.1' || !openwindLines[7].startsWith('Year,Month,Day,Hour,Minute,') || openwindLines[8].split(',').length !== OPENWIND_FIELDS) {
  problems.push(`Openwind: ${openwindLines.slice(0, 9).join(' / ')}`);
}
// The original's three means, then the speeds scaled to a 10 m/s mean: the file's speeds average 10.
console.log(`Openwind means: ${flat(await evaluate(`document.querySelector('.modal .openwind-means')?.innerText ?? ''`))}`);
await evaluate(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.startsWith('Escalar as velocidades')).querySelector('input[type=checkbox]').click()`);
await setValue(`[...document.querySelectorAll('.modal label.check')].find(l => l.innerText.startsWith('Escalar as velocidades')).querySelector('input[inputmode=decimal]')`, String(OPENWIND_TARGET_MEAN));
await sleep(SHORT_SETTLE_MS);
const { unlinkSync } = await import('node:fs');
unlinkSync(join(downloads, openwindFile));
await clickText('Exportar Openwind', '.modal button');
let scaledFile;
for (let attempt = 0; attempt < DOWNLOAD_POLLS && !scaledFile; attempt++) {
  await sleep(POLL_MS);
  scaledFile = readdirSync(downloads).find((name) => name.endsWith('_openwind.csv'));
}
const scaledSpeeds = readFileSync(join(downloads, scaledFile), 'utf8').split('\n').slice(8).filter(Boolean).map((line) => Number(line.split(',')[5]));
const scaledMean = scaledSpeeds.reduce((sum, speed) => sum + speed, 0) / scaledSpeeds.length;
console.log(`Openwind scaled to ${OPENWIND_TARGET_MEAN} m/s: mean ${scaledMean.toFixed(3)} over ${scaledSpeeds.length} steps`);
if (Math.abs(scaledMean - OPENWIND_TARGET_MEAN) > OPENWIND_MEAN_TOLERANCE) problems.push(`Openwind scaling: mean ${scaledMean}`);
await clickText('Fechar', '.modal button');

// A reload (F5) brings the workbook back from the browser's store; Arquivo lists what was opened.
const rowsBefore = await evaluate(datasetRows);
const storedSession = `(async () => {
  const database = await new Promise((resolve, reject) => { const opening = indexedDB.open('openwindaxis'); opening.onsuccess = () => resolve(opening.result); opening.onerror = () => reject(opening.error); });
  const session = await new Promise((resolve) => { const reading = database.transaction('workbooks').objectStore('workbooks').get('session'); reading.onsuccess = () => resolve(reading.result); reading.onerror = () => resolve(undefined); });
  database.close();
  return session?.datasets.length ?? 0;
})()`;
let stored = 0;
for (let attempt = 0; attempt < SESSION_POLLS && stored !== rowsBefore; attempt++) {
  await sleep(POLL_MS);
  stored = await evaluate(storedSession);
}
if (stored !== rowsBefore) problems.push(`session: ${stored} of ${rowsBefore} datasets stored`);
await send('Page.reload');
await waitFor(`document.querySelector('.menubar')`, 'main window after the reload');
await waitFor(`${datasetRows} === ${rowsBefore}`, 'the workbook restored after the reload', IMPORT_TIMEOUT_MS);
console.log(`reload: ${rowsBefore} datasets back: ${flat(await evaluate(`[...document.querySelectorAll('.datasets tbody tr')].map(r => r.cells[2]?.innerText).join(', ')`))}`);
await clickText('Arquivo', '.menu > button');
await sleep(SHORT_SETTLE_MS);
const recentItems = await evaluate(`[...document.querySelectorAll('.dropdown button')].map(b => b.querySelector('.label')?.textContent ?? '').filter(label => /^\\d+\\. /.test(label))`);
console.log(`recent: ${recentItems.join(' | ')}`);
if (!recentItems.some((label) => label.endsWith('.wproj'))) problems.push(`recent: no workbook in ${recentItems.join(' | ')}`);
// Opening the recent workbook adds its dataset, as Abrir workbook does.
const recentWorkbook = recentItems.find((label) => label.endsWith('workbook.wproj'));
await evaluate(`[...document.querySelectorAll('.dropdown button')].find(b => b.querySelector('.label')?.textContent === ${JSON.stringify(recentWorkbook)}).click()`);
await waitFor(`${datasetRows} === ${rowsBefore + 1}`, 'the recent workbook opened', IMPORT_TIMEOUT_MS);
console.log(`recent ${recentWorkbook} opened: ${await evaluate(datasetRows)} datasets`);

// Ajuda › Conteúdo da ajuda opens the help pages (web/public/ajuda) in a tab of their own;
// the click counts as the user's, or the browser blocks the new tab as a popup.
await clickText('Ajuda', '.menu > button');
await sleep(SHORT_SETTLE_MS);
await send('Runtime.evaluate', { expression: `[...document.querySelectorAll('.dropdown button')].find(b => b.querySelector('.label')?.textContent === 'Conteúdo da ajuda').click()`, userGesture: true });
let helpTab;
for (let attempt = 0; attempt < HELP_POLLS && !helpTab?.title.startsWith(HELP_TITLE); attempt++) {
  await sleep(POLL_MS);
  helpTab = (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()).find((target) => target.type === 'page' && target.url.includes(HELP_PATH));
}
console.log(`help: ${helpTab ? `${helpTab.title} (${helpTab.url})` : 'no tab'}`);
if (!helpTab?.title.startsWith(HELP_TITLE)) problems.push(`help: ${helpTab ? helpTab.title : 'the menu opened no tab'}`);
if (helpTab) await fetch(`http://127.0.0.1:${PORT}/json/close/${helpTab.id}`);

const errors = await noteErrors();
if (errors.length) problems.push(`notes: ${errors.join(' | ')}`);
console.log('--- problems ---');
console.log(problems.length ? problems.join('\n') : 'none');
socket.close();
chrome.kill();
process.exit(0);

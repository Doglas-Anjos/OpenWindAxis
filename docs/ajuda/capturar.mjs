// Captures the screens and numbers of the help (Ajuda › Conteúdo da ajuda, built by
// montar.mjs) from a raw logger folder, as a user would go through the app: open the
// folder as a dataset, configure it, build and run the quality-control rules, open
// every analysis window, export, flag the tower shading, measure what leaving it
// unflagged does to the mean, and only then compare the twins day by day.
// The data: an Ammonit Meteo-40 logger's daily CSV files (a 100 m mast with twin
// anemometers at 100, 80, 60 and 40 m, three vanes, temperature, humidity, pressure).
// Usage (dev server running): node docs/ajuda/capturar.mjs <chrome.exe> <app url> <logger folder>
// Writes web/public/ajuda/img/*.webp and docs/ajuda/fatos.json.
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [chromePath, appUrl, loggerFolder] = process.argv.slice(2);
const here = dirname(fileURLToPath(import.meta.url));
const imageDir = join(here, '..', '..', 'web', 'public', 'ajuda', 'img');
const factsPath = join(here, 'fatos.json');
const profileDir = join(tmpdir(), 'openwindaxis-ajuda-profile');
/** The screenshots' format and quality: WebP keeps the help light. */
const IMAGE_FORMAT = 'webp';
const IMAGE_QUALITY = 82;
const PORT = 9335;
const SETTLE_MS = 1500;
const SHORT_SETTLE_MS = 300;
const RENDER_SETTLE_MS = 1200;
const POLL_MS = 250;
const STARTUP_POLLS = 50;
const STARTUP_POLL_MS = 200;
const DEFAULT_TIMEOUT_MS = 60_000;
const IMPORT_TIMEOUT_MS = 300_000;
const VIEWPORT = { width: 1600, height: 900 };
/** Sharper screenshots: twice the CSS pixels. */
const DEVICE_SCALE = 2;
/** Room around a clipped element, CSS px. */
const CLIP_MARGIN = 6;

/** The logger's columns, as the Ammonit import names them. */
const SPEEDS = {
  a100: 'wind_speed 100m Avg (Anemómetro 1)',
  b100: 'wind_speed 100m Avg (Anemómetro 2)',
  a80: 'wind_speed 80m Avg (Anemómetro 3)',
  b80: 'wind_speed 80m Avg (Anemómetro 4)',
  a60: 'wind_speed 60m Avg (Anemómetro 5)',
  b60: 'wind_speed 60m Avg (Anemómetro 6)',
  a40: 'wind_speed 40m Avg (Anemómetro 7)',
  b40: 'wind_speed 40m Avg (Anemómetro 8)',
};
const VANES = { top: 'wind_direction 96m Avg (Veleta 1)', middle: 'wind_direction 77m Avg (Veleta 2)', low: 'wind_direction 57m Avg (Veleta 3)' };
const TEMPERATURE = 'temperature 97.5m Avg (Temp-Hum)';
const HUMIDITY = 'humidity 97.5m Avg (Temp-Hum)';
const PRESSURE = 'air_pressure 97m Avg (Barómetro)';
/** The twins: each B anemometer is compared day by day with the A at its height. */
const TWINS = [
  [SPEEDS.b100, SPEEDS.a100],
  [SPEEDS.b80, SPEEDS.a80],
  [SPEEDS.b60, SPEEDS.a60],
  [SPEEDS.b40, SPEEDS.a40],
];
/** The QC rules, as the help explains them (NREL tables 9.1–9.3, MEASNET annex B). */
const INVALID_FLAG = '0';
/** A 10-minute mean below this is a stalled anemometer, not a calm, m/s. */
const STALLED_BELOW_M_S = '0.01';
const SPEED_MAX_M_S = '60';
/** The same speed for 3 h (18 steps of 10 min) is a stuck sensor; a vane for 1 h. */
const STUCK_SPEED_STEPS = '18';
const STUCK_VANE_STEPS = '6';
/** A day whose ratio to the twin departs this much from the record's, %. */
const TWIN_TOLERANCE_PERCENT = '3';
const TEMPERATURE_RANGE = ['-30', '50'];
/** The upper bound is left out of a rule's range: just past 100 % keeps saturated air valid. */
const HUMIDITY_RANGE = ['1', '100.000001'];
const PRESSURE_RANGE = ['500', '1100'];
/** The boom of the help's example: pointing north, shaded with the wind from the south. */
const EXAMPLE_BOOM_DEG = 0;
const EXTRAPOLATION_HEIGHT = '120';
/** The speed from which the distortion factors are read, m/s (the TI's and the extrapolation's floor). */
const MIN_DISTORTION_SPEED = '4';
/** The 100 m speed as the two anemometers make it when both are kept: their mean. */
const AVERAGE_COLUMN = 'Vel 100 m média dos dois';
/** The twin rules of the 80, 60 and 40 m pairs (4 to 7 are the four pairs'), removed until the shading is flagged. */
const LOWER_TWIN_RULES = [7, 6, 5];
/** The 80 m pair's rules read the top vane: the 77 m one stuck from April to July. */
const PAIR_80M = '1';

// Every run replaces the images: one that a step no longer takes would linger otherwise.
rmSync(imageDir, { recursive: true, force: true });
mkdirSync(imageDir, { recursive: true });
const chrome = spawn(chromePath, [
  '--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profileDir}`,
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
  if (result.exceptionDetails) throw new Error(`${expression.slice(0, 200)}: ${result.exceptionDetails.exception?.description}`);
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
/** The window on top in the right pane: the other open windows stay mounted, hidden. */
const ACTIVE = `document.querySelector('.window:not(.behind)')`;
const MODAL = `document.querySelector('.modal')`;
/** Saves a screenshot of the element `selector` evaluates to (the whole page without one). */
const shot = async (name, selector) => {
  await sleep(RENDER_SETTLE_MS);
  let clip;
  if (selector) {
    const [x, y, width, height] = await evaluate(`(() => { const r = (${selector}).getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
    const left = Math.max(0, x - CLIP_MARGIN), top = Math.max(0, y - CLIP_MARGIN);
    clip = { x: left, y: top, width: Math.min(VIEWPORT.width - left, width + 2 * CLIP_MARGIN), height: Math.min(VIEWPORT.height - top, height + 2 * CLIP_MARGIN), scale: 1 };
  }
  const { data } = await send('Page.captureScreenshot', { format: IMAGE_FORMAT, quality: IMAGE_QUALITY, ...(clip && { clip }) });
  writeFileSync(join(imageDir, `${name}.${IMAGE_FORMAT}`), Buffer.from(data, 'base64'));
  console.log(`  imagem ${name}`);
};
const clickText = (text, selector = 'button, [role=tab], summary, label', scope = 'document') =>
  evaluate(`(() => { const el = [...${scope}.querySelectorAll(${JSON.stringify(selector)})].find(e => e.textContent.trim() === ${JSON.stringify(text)}); if (!el || el.disabled) return false; el.click(); return true; })()`);
const pickMenu = async (menu, item) => {
  if (!(await clickText(menu, '.menu > button'))) throw new Error(`no menu ${menu}`);
  await sleep(SHORT_SETTLE_MS);
  const picked = await evaluate(`(() => { const el = [...document.querySelectorAll('.dropdown button')].find(b => b.querySelector('.label')?.textContent === ${JSON.stringify(item)}); if (!el || el.disabled) return false; el.click(); return true; })()`);
  if (!picked) throw new Error(`no enabled item ${menu} › ${item}`);
  await sleep(SETTLE_MS);
};
const setValue = (selector, value) =>
  evaluate(`(() => { const el = ${selector}; const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); return true; })()`);
const labelled = (scope, label, field = 'select') => `[...${scope}.querySelectorAll('label')].find(l => l.innerText.trim().startsWith(${JSON.stringify(label)})).querySelector(${JSON.stringify(field)})`;
const text = (selector) => evaluate(`(${selector})?.innerText ?? ''`);
const facts = () => text(`${ACTIVE}.querySelector('.facts')`);
const datasetRows = `document.querySelectorAll('.datasets tbody tr').length`;
const rowOf = (name) => `[...document.querySelectorAll('.datasets tbody tr')].find(r => r.innerText.includes(${JSON.stringify(name)}))`;
const notReapplying = `!document.querySelector('.statusbar').innerText.includes('Reaplicando')`;

/** What the guide quotes, by step. */
const found = {};
/** Runs one step of the tour; a failure is noted and the tour goes on. */
const step = async (name, work) => {
  console.log(name);
  try {
    await work();
  } catch (thrown) {
    problems.push(`${name}: ${thrown.message}`);
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape' }).catch(() => {});
    await clickText('Cancelar', '.modal button').catch(() => {});
    await clickText('Fechar', '.modal button').catch(() => {});
  }
};
/** Opens an analysis window and waits for its numbers and chart. */
const analysis = async (item, ready = `${ACTIVE}.querySelector('.facts')`) => {
  await pickMenu('Analisar', item);
  await waitFor(ready, item, IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
};

await send('Page.enable');
await send('Runtime.enable');
await send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: DEVICE_SCALE, mobile: false });
await send('Storage.clearDataForOrigin', { origin: new URL(appUrl).origin, storageTypes: 'indexeddb' });
await send('Page.navigate', { url: appUrl });
await waitFor(`document.querySelector('.menubar')`, 'main window');
await waitFor(`!document.querySelector('.statusbar').innerText.includes('Carregando')`, 'core loaded');

/** Checks exactly `wanted` among the labelled checkboxes under `scope` (labels are column ids). */
const checkOnly = (scope, wanted) =>
  evaluate(`(() => { const wanted = new Set(${JSON.stringify(wanted)});
    for (const label of ${scope}.querySelectorAll('label.check')) {
      const box = label.querySelector('input[type=checkbox]');
      if (box && box.checked !== wanted.has(label.innerText.trim())) box.click();
    } return true; })()`);
/** Adds one rule through the window's form: the test, its fields by label, the duration and the columns. */
async function addRule({ kind, fields = {}, reference, minSteps = '1', columns }) {
  const form = `${ACTIVE}.querySelector('.ruleform')`;
  await setValue(labelled(form, 'Teste'), kind);
  await sleep(SHORT_SETTLE_MS);
  for (const [label, value] of Object.entries(fields)) await setValue(labelled(form, label, 'input'), value);
  if (reference) await setValue(labelled(form, 'Anemômetro de referência'), reference);
  await setValue(labelled(form, 'Duração mínima', 'input'), minSteps);
  await setValue(labelled(form, 'Flag'), INVALID_FLAG);
  await sleep(SHORT_SETTLE_MS);
  await checkOnly(`${form}.querySelector('fieldset.checks')`, columns);
  await sleep(SHORT_SETTLE_MS);
  if (!(await clickText('Adicionar regra', 'button', ACTIVE))) throw new Error(`rule ${kind} not added`);
  await sleep(SHORT_SETTLE_MS);
}
const measuredRow = `[...${ACTIVE}.querySelectorAll('table.distribution tr')].map(r => r.innerText.replace(/\\t/g, ' | ')).find(r => r.startsWith('Dados medidos'))`;
const sectorTable = `[...${ACTIVE}.querySelectorAll('.tablewrap table')].find(t => t.innerText.includes('Mediana'))`;

await step('1. abrir a pasta do logger', async () => {
  const { root } = await send('DOM.getDocument', { depth: -1 });
  const { nodeIds } = await send('DOM.querySelectorAll', { nodeId: root.nodeId, selector: 'input[webkitdirectory]' });
  const started = Date.now();
  // Arquivo › Abrir pasta como dataset… is the last folder input.
  await send('DOM.setFileInputFiles', { nodeId: nodeIds.at(-1), files: [loggerFolder] });
  await waitFor(`${datasetRows} === 1`, 'the dataset', IMPORT_TIMEOUT_MS);
  found.openSeconds = (Date.now() - started) / 1000;
  await sleep(SETTLE_MS * 2);
  found.dataset = await evaluate(`(() => { const heads = [...document.querySelectorAll('.datasets thead th')].map(th => th.textContent.trim()); const row = document.querySelector('.datasets tbody tr'); return Object.fromEntries([...row.cells].map((c, i) => [heads[i] || String(i), c.innerText.trim()])); })()`);
  await evaluate(`document.querySelector('.datasets tbody tr').click()`);
  await sleep(SETTLE_MS);
  await shot('02-workbook');
  await shot('02b-barra-de-ferramentas', `document.querySelector('.toolbar')`);
  await clickText('Arquivo', '.menu > button');
  await sleep(SHORT_SETTLE_MS);
  found.fileMenu = await evaluate(`[...document.querySelectorAll('.dropdown button')].map(b => b.querySelector('.label')?.textContent).filter(Boolean)`);
  const [right, bottom] = await evaluate(`(() => { const r = document.querySelector('.dropdown').getBoundingClientRect(); return [r.right, r.bottom]; })()`);
  await sleep(RENDER_SETTLE_MS);
  const { data } = await send('Page.captureScreenshot', { format: IMAGE_FORMAT, quality: IMAGE_QUALITY, clip: { x: 0, y: 0, width: right + 2 * CLIP_MARGIN, height: bottom + 2 * CLIP_MARGIN, scale: 1 } });
  writeFileSync(join(imageDir, `03-menu-arquivo.${IMAGE_FORMAT}`), Buffer.from(data, 'base64'));
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape' });
  await evaluate(`document.body.click()`);
  await sleep(SHORT_SETTLE_MS);
});

await step('2. configurar o dataset', async () => {
  await pickMenu('Revisar', 'Configurar dataset…');
  await waitFor(MODAL, 'configure dialog');
  await sleep(SETTLE_MS);
  found.basic = await text(MODAL);
  await shot('04-configurar-propriedades', MODAL);
  await clickText('Colunas', '.dialog-tabs button');
  await sleep(SETTLE_MS);
  await shot('05-configurar-colunas', MODAL);
  await clickText('Colunas primárias', '.dialog-tabs button');
  await sleep(SHORT_SETTLE_MS);
  found.primary = await text(`document.querySelector('.modal .primary-columns')`);
  await shot('06-configurar-colunas-primarias', MODAL);
  await clickText('Cancelar', '.modal button');
  await sleep(SHORT_SETTLE_MS);
});

await step('3. escala e offset', async () => {
  await pickMenu('Revisar', 'Escala e offset…');
  await waitFor(MODAL, 'scale and offset dialog');
  await shot('07-escala-e-offset', MODAL);
  await clickText('Cancelar', '.modal button');
  await sleep(SHORT_SETTLE_MS);
});

await step('4. gráficos-resumo, série e cobertura', async () => {
  await pickMenu('Exibir', 'Gráficos-resumo');
  await waitFor(`document.querySelectorAll('.summary .chart canvas').length > 0`, 'summary graphs', IMPORT_TIMEOUT_MS);
  await shot('08-graficos-resumo', ACTIVE);
  await analysis('Série temporal', `${ACTIVE}.querySelector('.chart canvas')`);
  await shot('09-serie-temporal', ACTIVE);
  await evaluate(`[...${ACTIVE}.querySelectorAll('.ts-display label')].find(l => l.innerText.includes('Médias diárias')).querySelector('input').click()`);
  await waitFor(`${ACTIVE}.querySelector('.ts-chart canvas')`, 'daily means');
  await shot('10-medias-diarias', ACTIVE);
  await evaluate(`[...${ACTIVE}.querySelectorAll('.ts-display label')].find(l => l.innerText.includes('Dados medidos')).querySelector('input').click()`);
  await analysis('Cobertura de dados', `${ACTIVE}.querySelector('canvas, table')`);
  await shot('11-cobertura', ACTIVE);
});

await step('5. as duas falhas vistas na dispersão', async () => {
  await analysis('Dispersão');
  await setValue(labelled(ACTIVE, 'Eixo x'), SPEEDS.a100);
  await setValue(labelled(ACTIVE, 'Eixo y'), SPEEDS.b100);
  await sleep(SETTLE_MS * 2);
  found.scatterSpeedsBeforeQc = await facts();
  await shot('12-dispersao-100m-antes', ACTIVE);
  await setValue(labelled(ACTIVE, 'Eixo x'), VANES.top);
  await setValue(labelled(ACTIVE, 'Eixo y'), VANES.middle);
  await waitFor(`${ACTIVE}.querySelector('.facts')?.innerText !== ${JSON.stringify(found.scatterSpeedsBeforeQc)}`, 'the vanes scatter', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 2);
  found.scatterVanes = await facts();
  await shot('13-dispersao-cata-ventos', ACTIVE);
});

await step('6. controle de qualidade por regras', async () => {
  await analysis('Histograma');
  found.histogramBeforeQc = await facts();
  await pickMenu('Flag', 'Flag por regras…');
  await waitFor(`${ACTIVE}.querySelector('.ruleform')`, 'the rule form');
  const speeds = Object.values(SPEEDS);
  await addRule({ kind: 'outside_range', fields: { Mínimo: STALLED_BELOW_M_S, Máximo: SPEED_MAX_M_S }, columns: speeds });
  await addRule({ kind: 'stuck', fields: { 'Tolerância por passo': '0' }, minSteps: STUCK_SPEED_STEPS, columns: speeds });
  await addRule({ kind: 'stuck', fields: { 'Tolerância por passo': '0' }, minSteps: STUCK_VANE_STEPS, columns: Object.values(VANES) });
  for (const [twin, reference] of TWINS) await addRule({ kind: 'twin_drift', fields: { 'Tolerância (%)': TWIN_TOLERANCE_PERCENT }, reference, columns: [twin] });
  await addRule({ kind: 'outside_range', fields: { Mínimo: TEMPERATURE_RANGE[0], Máximo: TEMPERATURE_RANGE[1] }, columns: [TEMPERATURE] });
  await addRule({ kind: 'outside_range', fields: { Mínimo: HUMIDITY_RANGE[0], Máximo: HUMIDITY_RANGE[1] }, columns: [HUMIDITY] });
  await addRule({ kind: 'outside_range', fields: { Mínimo: PRESSURE_RANGE[0], Máximo: PRESSURE_RANGE[1] }, columns: [PRESSURE] });
  found.rules = await text(`${ACTIVE}.querySelector('tbody')`);
  await clickText('Testar todas', 'button', ACTIVE);
  await waitFor(`${ACTIVE}.querySelector('.runsummary h4')?.innerText.startsWith('Teste')`, 'the rules test', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.rulesTest = await text(`${ACTIVE}.querySelector('.runsummary')`);
  // The lower pairs' twin rules wait for the tower shading (chapter 8): the last first, so the numbers hold.
  for (const position of LOWER_TWIN_RULES) {
    await evaluate(`${ACTIVE}.querySelector('[aria-label="Excluir a regra ${position}"]').click()`);
    await sleep(SHORT_SETTLE_MS);
  }
  found.rulesRunList = await text(`${ACTIVE}.querySelector('tbody')`);
  await clickText('Executar todas', 'button', ACTIVE);
  await waitFor(`${ACTIVE}.querySelector('.runsummary h4')?.innerText.startsWith('Última')`, 'the rules run', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.rulesRun = await text(`${ACTIVE}.querySelector('.runsummary')`);
  await shot('14-flag-por-regras', ACTIVE);
  await pickMenu('Flag', 'Inspecionar e remover flags…');
  await sleep(SETTLE_MS * 2);
  found.inspect = await text(ACTIVE);
  await shot('15-inspecionar-flags', ACTIVE);
});

await step('7. flag manual e por dispersão', async () => {
  await pickMenu('Flag', 'Flag manual…');
  await waitFor(`${ACTIVE}.querySelector('.flag-controls')`, 'flag controls');
  await sleep(SETTLE_MS);
  await shot('16-flag-manual', ACTIVE);
  await pickMenu('Flag', 'Flag por dispersão…');
  await waitFor(`${ACTIVE}.querySelector('.flagscatter canvas')`, 'flag scatter', IMPORT_TIMEOUT_MS);
  await setValue(labelled(ACTIVE, 'Eixo x'), SPEEDS.a100);
  await setValue(labelled(ACTIVE, 'Eixo y'), SPEEDS.b100);
  await sleep(SETTLE_MS * 2);
  await shot('17-flag-por-dispersao', ACTIVE);
});

await step('8. análises do dia a dia', async () => {
  await analysis('Histograma');
  found.histogram = await facts();
  await shot('18-histograma', ACTIVE);
  await analysis('Rosa dos ventos', `${ACTIVE}.querySelector('canvas')`);
  found.rose = await evaluate(`[...${ACTIVE}.querySelectorAll('.facts, .hint')].map(h => h.innerText).join(' | ')`);
  await shot('19-rosa-dos-ventos', ACTIVE);
  await analysis('Perfil diurno', `${ACTIVE}.querySelector('canvas')`);
  await shot('20-perfil-diurno', ACTIVE);
  await analysis('Dispersão');
  await setValue(labelled(ACTIVE, 'Eixo x'), SPEEDS.a100);
  await setValue(labelled(ACTIVE, 'Eixo y'), SPEEDS.b100);
  await sleep(SETTLE_MS * 2);
  found.scatter = await facts();
  await shot('21-dispersao-100m-depois', ACTIVE);
  await analysis('Tabelas');
  found.tables = await facts();
  await shot('22-tabelas', ACTIVE);
});

await step('9. distorção da torre', async () => {
  await analysis('Distorção da torre', `${ACTIVE}.querySelector('.analysis-output canvas')`);
  found.distortion = await text(`${ACTIVE}.querySelector('.analysis-output table')`);
  const valueRow = `[...${ACTIVE}.querySelectorAll('.filterby .criterion')].find(r => r.innerText.trim().startsWith('Coluna de dados'))`;
  await evaluate(`${valueRow}.querySelector('input[type=checkbox]').click()`);
  await setValue(`${valueRow}.querySelector('select')`, SPEEDS.a100);
  await setValue(`${valueRow}.querySelector('input[aria-label=Mínimo]')`, MIN_DISTORTION_SPEED);
  await waitFor(`${ACTIVE}.querySelector('.analysis-output table')?.innerText !== ${JSON.stringify(found.distortion)}`, 'the distortion above 4 m/s', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 2);
  found.distortionAbove4 = await text(`${ACTIVE}.querySelector('.analysis-output table')`);
  await shot('23-distorcao-da-torre', ACTIVE);
  await evaluate(`${valueRow}.querySelector('input[type=checkbox]').click()`);
  await sleep(SETTLE_MS);
});

await step('10. cisalhamento', async () => {
  await analysis('Cisalhamento do vento');
  found.shear = await facts();
  await shot('24-cisalhamento', ACTIVE);
  await clickText('Tabela', '.format button', ACTIVE);
  await waitFor(`${ACTIVE}.querySelector('.heightfits')`, 'the height pair fits', IMPORT_TIMEOUT_MS);
  found.heightFits = await text(`${ACTIVE}.querySelector('.heightfits')`);
  await clickText('Gráfico', '.format button', ACTIVE);
  await sleep(SHORT_SETTLE_MS);
});

await step('11. distribuição de velocidade', async () => {
  await analysis('Distribuição de velocidade', `${ACTIVE}.querySelector('table.distribution tbody tr')`);
  found.distribution = await evaluate(`[...${ACTIVE}.querySelectorAll('table.distribution tr')].map(r => r.innerText.replace(/\\t/g, ' | '))`);
  await shot('25-distribuicao', ACTIVE);
});

await step('12. turbulência', async () => {
  await analysis('Turbulência');
  found.turbulence = await facts();
  await shot('26-turbulencia', ACTIVE);
});

await step('13. ventos extremos', async () => {
  await pickMenu('Analisar', 'Ventos extremos');
  await waitFor(`${ACTIVE}.querySelector('.facts, .note')`, 'extremes', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.extremes = await text(`${ACTIVE}.querySelector('.facts') ?? ${ACTIVE}.querySelector('.note')`);
  await shot('27-ventos-extremos', ACTIVE);
});

await step('14. extrapolação vertical', async () => {
  await pickMenu('Revisar', 'Extrapolação vertical…');
  await waitFor(MODAL, 'extrapolation dialog');
  await setValue(labelled(MODAL, 'Alturas de destino', 'select, input'), EXTRAPOLATION_HEIGHT);
  await sleep(SETTLE_MS);
  await shot('28-extrapolacao-vertical', MODAL);
  await clickText('Cancelar', '.modal button');
  await sleep(SHORT_SETTLE_MS);
});

await step('15. ajuste de longo prazo', async () => {
  await pickMenu('Revisar', 'Ajuste de longo prazo…');
  await waitFor(`${ACTIVE}.querySelector('.summary-empty, .facts')`, 'the long-term window', IMPORT_TIMEOUT_MS);
  found.longTerm = await text(`${ACTIVE}.querySelector('.summary-empty') ?? ${ACTIVE}.querySelector('.facts')`);
});

await step('16. exportar para o Openwind', async () => {
  await pickMenu('Arquivo', 'Exportar dados…');
  await waitFor(MODAL, 'export dialog');
  await sleep(SETTLE_MS);
  found.openwindMeans = await text(`document.querySelector('.modal .openwind-means')`);
  await evaluate(`[...document.querySelectorAll('.modal fieldset')].find(f => f.querySelector('legend')?.innerText.trim() === 'Openwind').scrollIntoView({ block: 'start' })`);
  await shot('30-exportar-openwind', MODAL);
  await clickText('Fechar', '.modal button');
  await sleep(SHORT_SETTLE_MS);
});

await step('17. sombra da torre', async () => {
  await pickMenu('Flag', 'Sombra da torre…');
  await waitFor(`${ACTIVE}.querySelector('.shading-plots canvas')`, 'the pair plots', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 3);
  found.shadingPattern = await text(`${ACTIVE}.querySelector('.shading-window')`);
  found.shadingPairs = await evaluate(`[...${ACTIVE}.querySelectorAll('.shading-list option')].map(o => o.textContent)`);
  await shot('31-sombra-padrao-detectado', ACTIVE);
  await evaluate(`[...${ACTIVE}.querySelectorAll('.shading-basis label.check')].find(l => l.innerText.trim() === 'orientação da lança').querySelector('input').click()`);
  await waitFor(`${ACTIVE}.querySelector('.shading-details')`, 'the boom details');
  const boom = `[...${ACTIVE}.querySelectorAll('.shading-details label')].find(l => l.innerText.startsWith('Orientação da lança')).querySelector('input')`;
  await setValue(boom, String(EXAMPLE_BOOM_DEG));
  await waitFor(`${ACTIVE}.querySelector('.shading-rules .rule-lines li:not(.hint)')`, 'the boom rule', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 2);
  found.shadingBoom = await text(`${ACTIVE}.querySelector('.shading-window')`);
  await shot('32-sombra-pela-lanca', ACTIVE);
  await setValue(boom, '');
  await waitFor(notReapplying, 'the boom cleared', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
});

await step('18. o efeito de deixar a sombra sem flag', async () => {
  // The two 100 m anemometers' mean (a calculated column, read with the default filtering).
  await pickMenu('Revisar', 'Configurar dataset…');
  await waitFor(MODAL, 'configure dialog');
  await clickText('Colunas', '.dialog-tabs button');
  await setValue(`document.querySelector('.modal .calculated-form input')`, AVERAGE_COLUMN);
  await checkOnly(`document.querySelector('.modal .calculated-form')`, [SPEEDS.a100, SPEEDS.b100]);
  await sleep(SHORT_SETTLE_MS);
  if (!(await clickText('Adicionar coluna', '.modal button'))) throw new Error('the average column was not added');
  await clickText('OK', '.modal button');
  await waitFor(`!document.querySelector('.modal') && ${notReapplying}`, 'the average column', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  await analysis('Distribuição de velocidade', `${ACTIVE}.querySelector('table.distribution tbody tr')`);
  const before = await evaluate(measuredRow);
  await setValue(labelled(ACTIVE, 'Sensor de velocidade'), AVERAGE_COLUMN);
  await waitFor(`${measuredRow} !== ${JSON.stringify(before)}`, 'the average distribution', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.averageBeforeShading = await evaluate(measuredRow);
  // The difference A − B by sector of the top pair: the redundant pair's agreement in m/s.
  await analysis('Distorção da torre', `${ACTIVE}.querySelector('.analysis-output canvas')`);
  await setValue(`[...${ACTIVE}.querySelectorAll('label')].find(l => l.innerText.trim().startsWith('Par')).querySelector('select')`, '0');
  await setValue(labelled(ACTIVE, 'Calcular'), 'difference');
  await clickText('Tabela', '.format button', ACTIVE);
  await waitFor(sectorTable, 'the difference by sector', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.distortionDifference = await text(sectorTable);
  await setValue(labelled(ACTIVE, 'Calcular'), 'ratio');
  await clickText('Gráfico', '.format button', ACTIVE);
  // The shading rules of every pair, run.
  await pickMenu('Flag', 'Sombra da torre…');
  await evaluate(`[...${ACTIVE}.querySelectorAll('.shading-basis label.check')].find(l => l.innerText.trim() === 'padrão de sombra detectado').querySelector('input').click()`);
  await waitFor(`${ACTIVE}.querySelector('.shading-plots canvas')`, 'the pair plots', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 2);
  await setValue(`${ACTIVE}.querySelector('.shading-list select')`, PAIR_80M);
  await sleep(SETTLE_MS);
  await setValue(labelled(`${ACTIVE}.querySelector('.shading-details')`, 'Sensor de direção para as regras'), VANES.top);
  await sleep(SETTLE_MS * 2);
  found.shadingRules = await text(`${ACTIVE}.querySelector('.shading-rules .rule-lines')`);
  if (!(await clickText('Executar regras', 'button', ACTIVE))) throw new Error('no Executar regras');
  await waitFor(`${ACTIVE}.querySelector('.cardfoot .note')?.innerText.match(/\\d/) && ${notReapplying}`, 'the shading rules run', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS * 2);
  found.shadingRun = await text(`${ACTIVE}.querySelector('.cardfoot .note')`);
  await shot('33-sombra-executada', ACTIVE);
  await analysis('Distribuição de velocidade', `${ACTIVE}.querySelector('table.distribution tbody tr')`);
  await setValue(labelled(ACTIVE, 'Sensor de velocidade'), AVERAGE_COLUMN);
  await waitFor(`${measuredRow} !== ${JSON.stringify(found.averageBeforeShading)}`, 'the average without the shaded sensor', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.averageAfterShading = await evaluate(measuredRow);
  await pickMenu('Flag', 'Inspecionar e remover flags…');
  await sleep(SETTLE_MS * 2);
  // The lower twins' daily drift, now that the shaded steps are out: what is left is the sensors.
  await pickMenu('Flag', 'Flag por regras…');
  await waitFor(`${ACTIVE}.querySelector('.ruleform')`, 'the rule form');
  for (const [twin, reference] of TWINS.slice(1)) await addRule({ kind: 'twin_drift', fields: { 'Tolerância (%)': TWIN_TOLERANCE_PERCENT }, reference, columns: [twin] });
  found.rulesFinalList = await text(`${ACTIVE}.querySelector('tbody')`);
  await clickText('Executar todas', 'button', ACTIVE);
  await waitFor(`${ACTIVE}.querySelector('.runsummary h4')?.innerText.startsWith('Última') && ${notReapplying}`, 'the twin rules run', IMPORT_TIMEOUT_MS);
  await sleep(SETTLE_MS);
  found.twinRun = await text(`${ACTIVE}.querySelector('.runsummary')`);
  await pickMenu('Flag', 'Inspecionar e remover flags…');
  await sleep(SETTLE_MS * 2);
  found.inspectAfterShading = await text(ACTIVE);
  await shot('34-inspecionar-sombra', ACTIVE);
});

writeFileSync(factsPath, JSON.stringify(found, null, 2));
console.log('--- problems ---');
console.log(problems.length ? problems.join('\n') : 'none');
socket.close();
chrome.kill();
process.exit(0);

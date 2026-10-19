// Builds the help (Ajuda › Conteúdo da ajuda, web/public/ajuda/index.html) from
// what capturar.mjs captured: the screens (web/public/ajuda/img/*.webp) and the
// numbers the app showed (fatos.json). It goes through the treatment of one real
// 100 m mast in numbered chapters and figures; each step says what it is, how to
// do it, why, and what the mast's data show, and every concept cites its source.
// Usage: node docs/ajuda/montar.mjs
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const found = JSON.parse(readFileSync(join(here, 'fatos.json'), 'utf8'));
const OUTPUT = join(root, 'web', 'public', 'ajuda', 'index.html');
const TITLE = 'Ajuda do OpenWindAxis';
const LOCALE = 'pt-BR';
const GENERATED_ON = new Date().toLocaleDateString(LOCALE, { day: '2-digit', month: 'long', year: 'numeric' });
/** The app's own mark. */
const LOGO = readFileSync(join(root, 'web', 'public', 'favicon.svg'), 'utf8');
const IMAGE_EXTENSION = 'webp';
const IMAGE_DIR = join(root, 'web', 'public', 'ajuda', 'img');
/** The 2× capture is shown at CSS size: half its pixels. */
const DEVICE_SCALE = 2;
/** Decimals of a speed difference quoted in the text, m/s. */
const DIFFERENCE_DIGITS = 2;
/** Decimals of a relative change quoted in the text, %. */
const PERCENT_DIGITS = 1;
const PERCENT = 100;
const DEGREES_PER_TURN = 360;
const STEPS_PER_DAY = 144;
/** The NREL handbook's sample criterion for a redundant pair's average difference, m/s (table 9.2). */
const REDUNDANT_DIFFERENCE_MS = 0.5;
/** MEASNET's shortest campaign (§7.2) and its recommended correlation for MCP (§8.3.2). */
const MEASNET_MONTHS = 12;
const MEASNET_MIN_R = 0.8;
/** The QC thresholds the capture typed (capturar.mjs), quoted in the text. */
const STALLED_BELOW_M_S = '0,01';
const STUCK_SPEED_HOURS = 3;
const STUCK_VANE_HOURS = 1;
const TWIN_TOLERANCE_PERCENT = 3;

/**
 * Width and height of a WebP (lossy VP8, lossless VP8L or extended VP8X), so
 * each figure reserves its box before it loads and the index links land right.
 */
function webpSize(path) {
  const bytes = readFileSync(path);
  const chunk = bytes.toString('ascii', 12, 16);
  if (chunk === 'VP8 ') return [bytes.readUInt16LE(26) & 0x3fff, bytes.readUInt16LE(28) & 0x3fff];
  if (chunk === 'VP8L') {
    const bits = bytes.readUInt32LE(21);
    return [(bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1];
  }
  if (chunk === 'VP8X') return [bytes.readUIntLE(24, 3) + 1, bytes.readUIntLE(27, 3) + 1];
  throw new Error(`${path}: WebP desconhecido (${chunk})`);
}

/** The sources, cited by key; numbered in the order the text first cites them. */
const SOURCES = {
  measnet: 'MEASNET. <i>Evaluation of Site-Specific Wind Conditions</i>. Version 3, setembro de 2022. <a href="https://measnet.com/documents/">measnet.com/documents</a>.',
  nrel: 'Bailey, B. H.; McDonald, S. L.; Bernadett, D. W.; Markus, M. J.; Elsholz, K. V. <i>Wind Resource Assessment Handbook: Fundamentals for Conducting a Successful Monitoring Program</i>. AWS Scientific para o NREL (subcontrato TAT-5-15283-01), abril de 1997. NREL/SR-440-22223. <a href="https://docs.nlr.gov/docs/legosti/fy97/22223.pdf">PDF</a>.',
  iec12: 'IEC 61400-12-1:2017. <i>Wind energy generation systems – Part 12-1: Power performance measurements of electricity producing wind turbines</i>. 2ª ed. Genebra: IEC. Anexo G: montagem dos instrumentos no mastro meteorológico.',
  iec1: 'IEC 61400-1:2019. <i>Wind energy generation systems – Part 1: Design requirements</i>. 4ª ed. Genebra: IEC.',
  windographer: 'UL Solutions. <i>Windographer Help</i>: artigos “Weibull Fit Algorithms”, “Openwind Algorithm for Weibull Fitting”, “Tower Distortion Factor”, “Scatter Factor”, “IEC Turbulence Modeling” e “Flag Tower Shading window”.',
  manwell: 'Manwell, J. F.; McGowan, J. G.; Rogers, A. L. <i>Wind Energy Explained: Theory, Design and Application</i>. 2ª ed. Chichester: Wiley, 2009.',
  stull: 'Stull, R. B. <i>An Introduction to Boundary Layer Meteorology</i>. Dordrecht: Kluwer, 1988.',
  iso2533: 'ISO 2533:1975. <i>Standard Atmosphere</i>. Genebra: ISO.',
  stevens: 'Stevens, M. J. M.; Smulders, P. T. The estimation of the parameters of the Weibull wind speed distribution for wind energy utilization purposes. <i>Wind Engineering</i>, 3(2), 132–145, 1979.',
  justus: 'Justus, C. G.; Hargraves, W. R.; Mikhail, A.; Graber, D. Methods for estimating wind speed frequency distributions. <i>Journal of Applied Meteorology</i>, 17(3), 350–353, 1978.',
  troen: 'Troen, I.; Petersen, E. L. <i>European Wind Atlas</i>. Roskilde: Risø National Laboratory, 1989.',
  gumbel: 'Gumbel, E. J. <i>Statistical Theory of Extreme Values and Some Practical Applications</i>. Applied Mathematics Series 33. Washington: National Bureau of Standards, 1954.',
  palutikof: 'Palutikof, J. P.; Brabson, B. B.; Lister, D. H.; Adcock, S. T. A review of methods to calculate extreme wind speeds. <i>Meteorological Applications</i>, 6, 119–132, 1999.',
  rogers: 'Rogers, A. L.; Rogers, J. W.; Manwell, J. F. Comparison of the performance of four measure–correlate–predict algorithms. <i>Journal of Wind Engineering and Industrial Aerodynamics</i>, 93, 243–264, 2005.',
  king: 'King, C.; Hurley, B. The SpeedSort, DynaSort and Scatter wind correlation methods. <i>Wind Engineering</i>, 29(3), 217–241, 2005.',
  leblanc: 'Leblanc, M.; Schoborg, D.; Cox, S.; Hach, A.; Tindal, A. Is a non-linear MCP method a useful tool for North American wind regimes? <i>Proceedings of WINDPOWER 2009</i>, Chicago, 2009.',
  lambert: 'Lambert, T.; Grue, A. The Matrix Time Series method for MCP. <i>Proceedings of WINDPOWER 2012</i>, Atlanta, 2012.',
  carta: 'Carta, J. A.; Velázquez, S.; Cabrera, P. A review of measure-correlate-predict (MCP) methods used to estimate long-term wind characteristics at a target site. <i>Renewable and Sustainable Energy Reviews</i>, 27, 362–400, 2013.',
  orlando: 'Orlando, S.; Bale, A.; Johnson, D. A. Experimental study of the effect of tower shadow on anemometer readings. <i>Journal of Wind Engineering and Industrial Aerodynamics</i>, 99(1), 1–6, 2011. <a href="https://doi.org/10.1016/j.jweia.2010.10.002">doi:10.1016/j.jweia.2010.10.002</a>.',
  stickland: 'Stickland, M.; Scanlon, T.; Fabre, S.; Oldroyd, A.; Kindler, D. Measurement and simulation of the flow field around a triangular lattice meteorological mast. <i>Energy and Power Engineering</i>, 5(10), 2013.',
};
const citedOrder = [];
/** A citation of `key`, numbered on first use; `where` is the section or table. */
function cite(key, where = '') {
  if (!SOURCES[key]) throw new Error(`fonte desconhecida: ${key}`);
  if (!citedOrder.includes(key)) citedOrder.push(key);
  const number = citedOrder.indexOf(key) + 1;
  return `<a class="cite" href="#ref-${key}" title="${SOURCES[key].replace(/<[^>]+>/g, '').replace(/"/g, '&quot;')}">[${number}${where ? `, ${where}` : ''}]</a>`;
}

/** Group `group` of `pattern` in `text`; a missing number stops the build (the app changed its wording). */
function pick(text, pattern, group = 1) {
  const match = pattern.exec(text ?? '');
  if (!match) throw new Error(`não achei ${pattern} em: ${String(text).slice(0, 300)}`);
  return match[group];
}
const lines = (text) => (text ?? '').split('\n').map((line) => line.trim()).filter(Boolean);
const escapeHtml = (text) => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
/** A number as the app writes it (1.234,5) read back. */
const parseLocal = (text) => Number(String(text).replace(/\./g, '').replace(',', '.'));
const formatLocal = (value, digits) => value.toLocaleString(LOCALE, { minimumFractionDigits: digits, maximumFractionDigits: digits });
/** A signed number, without the sign of a value that rounds to zero (−0,00). */
const signed = (value, digits) => {
  const rounded = Number(value.toFixed(digits));
  return `${rounded > 0 ? '+' : rounded < 0 ? '−' : ''}${formatLocal(Math.abs(rounded), digits)}`;
};

/** A logger column's id (`wind_speed 100m Avg (Anemómetro 1)`) as the text names it. */
function sensorName(id) {
  const match = /^(\w+) ([\d.]+)m \w+ \((.+)\)$/.exec(id);
  if (!match) return id;
  const [, kind, height, label] = match;
  const number = /(\d+)$/.exec(label)?.[1];
  const shown = height.replace('.', ',');
  if (kind === 'wind_speed') return `anemômetro ${number} (${shown} m)`;
  if (kind === 'wind_direction') return `cata-vento ${number} (${shown} m)`;
  return { temperature: 'temperatura', humidity: 'umidade', air_pressure: 'pressão' }[kind] ?? id;
}
/** The rows "rule, column, stretches, steps" of a rules run summary. */
const runRows = (text) => lines(text).filter((line) => /^\d+\t/.test(line)).map((line) => line.split('\t')).map(([rule, column, runs, steps]) => ({ rule: Number(rule), column, runs, steps }));
/** Each column's cells in the flag statistics: the flags' "steps (share)", in the header's order. */
const flagCells = (text) => Object.fromEntries(lines(text).filter((line) => line.includes('\t')).map((line) => line.split('\t').map((cell) => cell.replace('×', '').trim())).map(([column, ...cells]) => [column, cells]));
/** The "Dados medidos" row of the distribution table: steps, mean and power density. */
const measured = (row) => {
  const [, steps, mean, , power] = row.split(' | ');
  return { steps: pick(steps, /([\d.]+)/), mean: parseLocal(mean), power: parseLocal(power) };
};

// --- The numbers the text quotes ---
const dataset = found.dataset;
const days = parseLocal(dataset.Dias);
const months = (days / (365.25 / 12)).toFixed(0);
const detected = pick(found.basic, /Detectado: ([^\n]+)/);
const scatterBefore = { r2: pick(found.scatterSpeedsBeforeQc, /R²\s+([\d,]+)/), steps: pick(found.scatterSpeedsBeforeQc, /([\d.]+) passos/) };
const scatterAfter = { r2: pick(found.scatter, /R²\s+([\d,]+)/), steps: pick(found.scatter, /([\d.]+) passos/), slope: pick(found.scatter, /y = ([\d,]+)/) };
const vanes = { r2: pick(found.scatterVanes, /R²\s+([\d,]+)/) };
const qcRows = runRows(found.rulesRun).filter((row) => row.steps !== '0');
const flagged = flagCells(found.inspect);
const beforeQc = { n: pick(found.histogramBeforeQc, /N\s+([\d.]+)/), mean: pick(found.histogramBeforeQc, /Média\s+([\d,]+)/), k: pick(found.histogramBeforeQc, /\bk\s+([\d,]+)/) };
const afterQc = {
  n: pick(found.histogram, /N\s+([\d.]+)/),
  mean: pick(found.histogram, /Média\s+([\d,]+)/),
  k: pick(found.histogram, /\bk\s+([\d,]+)/),
  power: pick(found.histogram, /potência\s+([\d,]+) W/),
  density: pick(found.histogram, /ρ\s+([\d,]+)/),
};
const pairRows = (text) => lines(text).slice(1).map((line) => line.split('\t')).map(([pair, direction, steps, distortion, scatter]) => ({ pair, direction, steps, distortion, scatter }));
const distortionAll = pairRows(found.distortion);
const distortionAbove4 = pairRows(found.distortionAbove4);
const pairName = (pair) => pair.split(' × ').map(sensorName).join(' × ');
const shear = {
  alpha: pick(found.shear, /potência\)\s+([\d,]+)/),
  band: pick(found.shear, /potência\)\s+[\d,]+\s+\(([^)]+)\)/),
  z0: pick(found.shear, /log\)\s+([\d,]+) m/),
  steps: pick(found.shear, /simultâneos\s+([\d.]+)/),
};
const neighbourAlphas = lines(found.heightFits).filter((line) => line.startsWith('Vizinhas')).map((line) => parseLocal(line.split('\t')[1]));
const fits = found.distribution.slice(1).map((row) => row.split(' | '));
const fit = (name) => fits.find((row) => row[0] === name);
const measuredFit = fit('Dados medidos');
const turbulence = {
  mean: pick(found.turbulence, /15 m\/s:\s*([\d,]+)/),
  figure: pick(found.turbulence, /representativa\s*([\d,]+)/),
  category: pick(found.turbulence, /categoria IEC\s*(\S+)/),
  steps: pick(found.turbulence, /\(([\d.]+) passos\)/),
};
const extremes = { v50: pick(found.extremes, /50 anos\s+([\d,]+)/), peaks: pick(found.extremes, /(\d+) picos/) };
const openwind = {
  simple: pick(found.openwindMeans, /simples\s+([\d,]+) m\/s/),
  seasonal: pick(found.openwindMeans, /sazonal\s+([\d,]+) m\/s/),
  steps: pick(found.openwindMeans, /exportados\s+([\d.]+)/),
};
const seasonalBias = (parseLocal(openwind.simple) / parseLocal(openwind.seasonal) - 1) * PERCENT;
const shadingRules = [...found.shadingRules.matchAll(/a '([^']+)' onde o valor de '([^']+)' estiver de (\d+)° a (\d+)°/g)].map((match) => ({ sensor: match[1], vane: match[2], from: Number(match[3]), to: Number(match[4]) }));
const centerOf = ({ from, to }) => (from + ((to - from + DEGREES_PER_TURN) % DEGREES_PER_TURN) / 2) % DEGREES_PER_TURN;
const boomFrom = pick(found.shadingBoom, /vento de (\d+°) a \d+°/);
const boomTo = pick(found.shadingBoom, /vento de \d+° a (\d+°)/);
const topPair = shadingRules.slice(0, 2);
// The difference A − B by 5° sector of the top pair, inside each sensor's shaded sector.
const differenceRows = lines(found.distortionDifference)
  .slice(1)
  .map((line) => line.split('\t').map(parseLocal))
  .map(([sector, steps, mean]) => ({ sector, steps, mean }));
const inSector = (sector, { from, to }) => (sector - from + DEGREES_PER_TURN) % DEGREES_PER_TURN < (to - from + DEGREES_PER_TURN) % DEGREES_PER_TURN;
const weightedMean = (rows) => rows.reduce((sum, row) => sum + row.steps * row.mean, 0) / rows.reduce((sum, row) => sum + row.steps, 0);
const counted = differenceRows.filter((row) => row.steps > 0);
const difference = {
  overall: weightedMean(counted),
  shadedA: weightedMean(counted.filter((row) => inSector(row.sector, topPair[0]))),
  shadedB: weightedMean(counted.filter((row) => inSector(row.sector, topPair[1]))),
};
const averageBefore = measured(found.averageBeforeShading);
const averageAfter = measured(found.averageAfterShading);
const meanBias = (averageBefore.mean / averageAfter.mean - 1) * PERCENT;
const powerBias = (averageBefore.power / averageAfter.power - 1) * PERCENT;
const shadedShare = flagCells(found.inspectAfterShading);
const shadingRun = { runs: pick(found.shadingRun, /([\d.]+) trechos/), steps: pick(found.shadingRun, /([\d.]+) passos/) };
/** The numbers of the twin-drift rules in a rule list (its rows: number, test, columns…). */
const twinNumbers = (list) =>
  new Set(lines(list).filter((line) => /^\d+	/.test(line) && line.split('	')[1].startsWith('Deriva')).map((line) => Number(line.split('	')[0])));
/** The lower pairs' twins (80, 60 and 40 m): tested in chapter 3, before the shading flag; run in chapter 8, after it. */
const isTopTwin = (row) => row.column.endsWith('(Anemómetro 2)');
const twinTested = runRows(found.rulesTest).filter((row) => twinNumbers(found.rules).has(row.rule));
const topTwin = runRows(found.rulesRun).find((row) => twinNumbers(found.rulesRunList).has(row.rule));
const twinBefore = twinTested.filter((row) => !isTopTwin(row));
const twinAfter = runRows(found.twinRun).filter((row) => twinNumbers(found.rulesFinalList).has(row.rule) && !isTopTwin(row));
const dayCount = (steps) => Math.round(parseLocal(steps) / STEPS_PER_DAY);
for (const [name, value] of Object.entries({ ...difference, meanBias, powerBias, seasonalBias })) {
  if (!Number.isFinite(value)) throw new Error(`${name} não saiu da captura (veja fatos.json)`);
}
if (twinBefore.length !== 3 || twinAfter.length !== 3 || !topTwin) throw new Error('as regras de deriva entre gêmeos não saíram da captura');

// --- Layout helpers ---
let figureNumber = 0;
/** A numbered figure of `image` (img/<image>.webp), opening full size on click. */
function figure(image, caption, size = '') {
  figureNumber += 1;
  const source = `img/${image}.${IMAGE_EXTENSION}`;
  const [width, height] = webpSize(join(IMAGE_DIR, `${image}.${IMAGE_EXTENSION}`)).map((pixels) => Math.round(pixels / DEVICE_SCALE));
  return `<figure class="${size}"><a href="${source}" target="_blank" rel="noopener"><img src="${source}" width="${width}" height="${height}" alt="${escapeHtml(caption.replace(/<[^>]+>/g, ''))}" loading="lazy"></a><figcaption><b>Figura ${figureNumber}</b>: ${caption}</figcaption></figure>`;
}
const why = (body) => `<aside class="why"><h4>Por quê</h4>${body}</aside>`;
const data = (body, title = 'Nos dados do mastro') => `<aside class="data"><h4>${title}</h4>${body}</aside>`;
const how = (steps) => `<ol class="how">${steps.map((step) => `<li>${step}</li>`).join('')}</ol>`;
const table = (head, rows, className = '') =>
  `<table class="${className}"><thead><tr>${head.map((cell) => `<th>${cell}</th>`).join('')}</tr></thead><tbody>${rows.map((row) => `<tr>${row.map((cell) => `<td>${escapeHtml(cell)}</td>`).join('')}</tr>`).join('')}</tbody></table>`;
const menu = (path) => `<span class="menu">${path.split(' › ').map(escapeHtml).join(' <span class="sep">›</span> ')}</span>`;
/** The chapters and sections, for the side index. */
const toc = [];
const chapter = (id, title) => {
  toc.push({ id, title, sections: [] });
  return `<h2 id="${id}">${title}</h2>`;
};
const section = (id, title) => {
  toc.at(-1).sections.push({ id, title });
  return `<h3 id="${id}">${title}</h3>`;
};

const capabilities = [
  ['Arquivo', 'Abrir e anexar arquivos de logger, pastas e workbooks; salvar; exportar (CSV, WAsP .tab, Openwind, EPE); abertos recentemente', 'Juntar os arquivos diários num só registro e guardar o trabalho com as flags e as regras'],
  ['Revisar', 'Configurar dataset; excluir dados; escala e offset; deslocar tempo; reconstruir lacunas; extrapolação vertical; ajuste de longo prazo', 'Dizer ao programa o que cada coluna é, corrigir o que o logger gravou errado e estender os dados no tempo e na altura'],
  ['Flag', 'Flag manual, por dispersão, por regras e sombra da torre; inspecionar e remover; definir flags', 'Separar o dado ruim sem apagá-lo, com a evidência de cada marcação'],
  ['Analisar', 'Série temporal, dispersão, histograma, perfil diurno, rosa dos ventos, cobertura, turbulência, cisalhamento, distribuição, distorção da torre, ventos extremos, tabelas', 'Descrever o vento do local e conferir a coerência entre os sensores'],
  ['Fluxos', 'Gravar uma sequência de edições, regras e análises e repeti-la em outros datasets', 'Tratar várias torres do mesmo jeito sem refazer cada passo à mão'],
];

const sections = [];

sections.push(`
<header class="cover">
  <div class="logo">${LOGO}</div>
  <div>
    <p class="product">OpenWindAxis</p>
    <h1>Compatibilização, formatação e análise de dados de torres anemométricas</h1>
    <p class="subtitle">O que o sistema faz, passo a passo, sobre os dados brutos de um mastro de 100 m, e as fontes de cada conceito.</p>
  </div>
</header>`);

sections.push(`
<section>
  ${chapter('como-ler', 'Como ler esta ajuda')}
  <p>Esta ajuda percorre o tratamento completo de um mastro, dos arquivos do logger ao arquivo do modelo de parque. Os capítulos vão da configuração inicial às principais janelas, flags, análises, extrapolação vertical, longo prazo, exportação para o Openwind e sombra da torre. Cada passo tem quatro partes:</p>
  <ul>
    <li><b>O que é</b>: a função e onde ela fica no menu;</li>
    <li><b>Como fazer</b>: os cliques, na ordem;</li>
    <li><b>Por quê</b>: o motivo físico ou prático do passo, com a fonte entre colchetes; o número leva à lista de referências no fim;</li>
    <li><b>Nos dados do mastro</b>: o que o sistema mostrou com os dados brutos do exemplo.</li>
  </ul>
  <p>As figuras e os números foram produzidos pelo próprio OpenWindAxis, percorrendo as janelas como um usuário faria. Clique numa figura para vê-la em tamanho real.</p>
  ${section('o-que-faz', 'O que o OpenWindAxis faz')}
  ${table(['Menu', 'Funções', 'Para quê'], capabilities, 'capabilities')}
  ${section('dados', 'Os dados do exemplo')}
  <p>Os arquivos diários de um logger Ammonit Meteo-40 num mastro de 100 m, de ${dataset['Início']} a ${dataset.Fim} (${dataset.Dias} dias, ${dataset.Passos} passos de 10 minutos). O mastro tem:</p>
  <ul>
    <li>oito anemômetros de concha em pares, a 100, 80, 60 e 40 m;</li>
    <li>três cata-ventos, a 96, 77 e 57 m;</li>
    <li>temperatura e umidade a 97,5 m e pressão a 97 m.</li>
  </ul>
  <p>São dados brutos, com os defeitos de campo que o controle de qualidade precisa encontrar: um anemômetro que pifa e um cata-vento que trava.</p>
  ${why(`<p>A MEASNET pede ao menos ${MEASNET_MONTHS} meses completos e consecutivos de medição para captar a variação sazonal. A disponibilidade dos dados filtrados do sensor principal, junto com o reserva, deve passar de 90 % ${cite('measnet', '§7.2')}. Com ${dataset.Dias} dias (cerca de ${months} meses), esta campanha é <b>incompleta</b> por esse critério. O que ela mostra vale para o período medido, e o longo prazo pede uma referência (capítulo 6).</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('configuracao', '1º Configuração inicial')}
  ${section('abrir', '1.1 Abrir os dados')}
  <p><b>O que é.</b> O menu Arquivo tem:</p>
  <ul>
    <li>${menu('Arquivo › Abrir pasta como dataset…')}: junta todos os arquivos de uma pasta num só dataset;</li>
    <li>${menu('Arquivo › Adicionar dataset(s)…')}: abre arquivos soltos;</li>
    <li>${menu('Arquivo › Anexar dados…')} e ${menu('Arquivo › Anexar pasta…')}: acrescentam arquivos a um dataset já configurado;</li>
    <li>${menu('Arquivo › Abrir workbook…')}: abre um trabalho salvo.</li>
  </ul>
  <p>No fim do menu ficam os abertos recentemente.</p>
  ${figure('03-menu-arquivo', 'Menu Arquivo, com a pasta do logger entre os abertos recentemente.', 'narrow')}
  ${how([`Escolha ${menu('Arquivo › Abrir pasta como dataset…')} e aponte a pasta com os arquivos diários.`, 'Confira a configuração do primeiro arquivo (seção 1.2); os demais seguem a mesma.', `Salve com ${menu('Arquivo › Salvar workbook')}. O .wproj guarda os arquivos de origem, a configuração, as flags com as evidências e as regras.`])}
  ${data(`<p>Os ${dataset.Dias} arquivos diários viraram um dataset de ${dataset.Colunas} colunas em ${formatLocal(found.openSeconds, 1)} s. Um dos arquivos grava a data em outro formato (15/5/2018 00:00 em vez de 2018-05-15 00:00:00), e o anexo leu cada arquivo com o próprio formato.</p>`)}
  ${why(`<p>O logger grava as mesmas colunas todo dia. Ler a pasta como um registro garante que cada coluna seja tratada da mesma forma em todos os arquivos, e o anexo confere se o passo e as colunas batem.</p><p>A MEASNET exige documentação completa da medição: sensores, alturas, orientação das lanças, histórico e calibrações. Lacunas nessa documentação aumentam a incerteza ${cite('measnet', '§7.1 e anexo A')}. Ao guardar a configuração junto com os dados, o workbook é essa documentação.</p>`)}
  ${figure('02-workbook', 'O workbook com o mastro: o explorador com o dataset, o mapa e os gráficos-resumo.')}

  ${section('configurar', '1.2 Configurar o dataset')}
  <p><b>O que é.</b> Em ${menu('Revisar › Configurar dataset…')} se diz ao programa o que cada coluna é e onde a torre está. A mesma janela abre pelo botão de engrenagem da barra ou com duplo clique no dataset.</p>
  ${figure('02b-barra-de-ferramentas', 'Barra de ferramentas: novo, abrir e salvar workbook; configurar dataset (engrenagem); flags; as análises mais usadas; desfazer e refazer.', 'wide-strip')}
  ${figure('04-configurar-propriedades', 'Configurar dataset, aba Propriedades básicas: nome, localização, elevação, regra da turbulência, período, passo, significado do carimbo de tempo e leitura do arquivo.')}
  ${data(`<p>O formato do logger foi reconhecido (Ammonit Meteo-40): as alturas, o fuso e o tipo de cada sensor vêm da parte de configuração dos próprios arquivos. A leitura do primeiro arquivo: ${escapeHtml(detected)}. O logger não grava latitude, longitude nem elevação; elas se informam aqui.</p>`)}
  ${why(`<ul>
    <li><b>Elevação</b>: a densidade do ar cai com a altitude, e com ela a energia do vento, que é proporcional à densidade ${cite('manwell')}. Na atmosfera padrão, a 1.000 m o ar é cerca de 9 % menos denso que ao nível do mar ${cite('iso2533')}. Com temperatura e pressão medidas, como aqui, o sistema usa a densidade medida (${afterQc.density} kg/m³ neste mastro).</li>
    <li><b>Latitude e longitude</b>: posicionam a torre no mapa e são exigidas pelo arquivo .tab do WAsP e pelas variáveis solares.</li>
    <li><b>Fuso e carimbo de tempo</b>: o perfil diurno e a comparação com reanálises (em UTC) dependem de saber se 10:10 marca o intervalo que começa ou o que termina nessa hora.</li>
    <li><b>Passos faltando</b>: o teste de sequência do tempo é a primeira verificação recomendada ${cite('nrel', '§9.1 A.1')}; passos sem linha no arquivo entram como lacunas.</li>
  </ul>`)}
  ${figure('05-configurar-colunas', 'Aba Colunas: tipo, subtipo, altura, lança, unidade e a associação de cada coluna (máximo, mínimo e desvio padrão ligados à sua média), com a recuperação de dados.')}
  ${how(['Confira o <b>Tipo</b> e o <b>Subtipo</b> de cada coluna (velocidade, direção, temperatura…; média, mínimo, máximo, desvio padrão).', 'Confira a <b>Altura</b> dos sensores de vento.', 'Associe o máximo, o mínimo e o desvio padrão à média do mesmo sensor (<b>Associada a</b>).', 'Se souber, informe a <b>Lança</b>, a orientação do braço do sensor. Ela alimenta a sombra da torre (capítulo 8).'])}
  ${why(`<p>O tipo decide o que se pode calcular: sem "Velocidade" não há histograma nem Weibull; sem o desvio padrão associado à média não há intensidade de turbulência; sem as alturas não há cisalhamento. A associação aplica um teste de relação básico: quando a média de um passo é inválida, o desvio padrão, o máximo e o mínimo do mesmo sensor também são ${cite('measnet', 'anexo B')}.</p>`)}
  ${figure('06-configurar-colunas-primarias', 'Aba Colunas primárias: qual velocidade, direção e temperatura representam o dataset nos resumos e no explorador.')}
  ${why(`<p>Com vários anemômetros, os resumos precisam de um representante. O padrão é o mais alto, o mais próximo da altura de uma turbina; a MEASNET pede que a medição principal fique a pelo menos 2/3 da altura do cubo planejada ${cite('measnet', '§7.2')}.</p>`)}

  ${section('escala-offset', '1.3 Escala e offset')}
  <p><b>O que é.</b> ${menu('Revisar › Escala e offset…')} aplica y = a·x + b a colunas escolhidas, num trecho de tempo ou no registro todo: uma calibração nova, ou o offset de um cata-vento.</p>
  ${figure('07-escala-e-offset', 'Escala e offset: as colunas, a escala, o offset e o trecho em que valem.', 'narrow')}
  ${why(`<p>O cata-vento é montado com o zero apontando para onde o braço permite, não necessariamente para o norte. A orientação dele precisa ser alinhada e documentada na instalação, para permitir a correção do offset ${cite('measnet', '§7.3')}, ${cite('nrel', '§6.7')}. Sem o offset, toda a rosa dos ventos gira, e a sombra da torre aparece no setor errado.</p><p>Confira antes no cabeçalho do logger: o Meteo-40 deste mastro já aplica a calibração e o offset dos sensores, e aplicar de novo giraria as direções duas vezes.</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('janelas', '2º Principais janelas')}
  <p>As janelas de análise ficam no menu Analisar e na barra de ferramentas. Cada uma abre como uma aba à direita. À esquerda ficam os controles: o dataset, o formato (gráfico ou tabela) e o bloco <b>Filtrar por</b>, que filtra por flag, intervalo de datas, mês, hora, setor de direção ou faixa de uma coluna.</p>
  ${figure('08-graficos-resumo', 'Gráficos-resumo: cobertura de dados das colunas primárias, perfil vertical, frequência de direção, médias mensais e perfil diurno.')}
  ${section('serie', 'Série temporal')}
  ${figure('09-serie-temporal', 'Série temporal do mastro, em dados medidos (passos de 10 min).')}
  ${figure('10-medias-diarias', 'A mesma série em médias diárias.')}
  ${why(`<p>A inspeção visual detecta o que nenhum teste automático pega: picos, quedas bruscas, um sensor que trava. Ela deve incluir a comparação com as outras alturas ${cite('measnet', 'anexo B')}.</p>`)}
  ${section('cobertura', 'Cobertura de dados')}
  ${figure('11-cobertura', 'Cobertura de dados por mês, de cada coluna.')}
  ${why(`<p>Lacunas que se acumulam numa estação do ano introduzem erro sistemático. Por isso a distribuição das falhas no tempo deve ser avaliada e documentada ${cite('measnet', '§8.2.1 e §8.2.2')}.</p>`)}
  ${section('rosa-perfil', 'Rosa dos ventos e perfil diurno')}
  ${figure('19-rosa-dos-ventos', `Rosa dos ventos a 96 m (${escapeHtml(found.rose)})`)}
  ${figure('20-perfil-diurno', 'Perfil diurno: a velocidade média por hora do dia.')}
  ${why(`<p>A rosa diz de onde vem o vento e com que força. Ela orienta o arranjo das turbinas e mostra quais setores a sombra da torre afeta. O perfil diurno e o mensal estão entre os relatórios recomendados de um programa de medição ${cite('nrel', 'tab. 9.5')}.</p>`)}
  ${section('tabelas', 'Tabelas')}
  ${figure('22-tabelas', 'Tabelas: as médias mensais de cada ano.')}
  ${data(`<p>A média das médias mensais (MoMM) e a variabilidade interanual ficam em branco: com ${dataset.Dias} dias, não há um ano completo para dar peso a cada mês do calendário.</p>`)}
  ${why(`<p>A MoMM dá o mesmo peso a cada mês do calendário e evita o viés sazonal de uma média simples sobre um registro com lacunas. Ela só existe com os 12 meses medidos.</p>`)}
</section>`);

const qcTable = qcRows.map(({ rule, column, runs, steps }) => [String(rule), sensorName(column), runs, steps]);
sections.push(`
<section>
  ${chapter('flags', '3º Flags: o controle de qualidade')}
  <p><b>O que é.</b> Uma flag marca passos de uma coluna como inválidos, gelo, sombra da torre ou outra categoria definida em ${menu('Flag › Definir flags…')}. O dado não é apagado: as análises deixam de fora o que tem flag (Filtrar por › Flag), e cada marcação pode ser desfeita.</p>
  ${why(`<p>O NREL recomenda:</p><ul><li>validar sobre uma cópia, preservando o dado bruto;</li><li>dar a cada valor rejeitado um código com o motivo;</li><li>manter um registro de cada rejeição e substituição ${cite('nrel', '§9.1')}.</li></ul><p>A MEASNET pede que a filtragem seja documentada: quanto foi rejeitado, quais períodos e como as falhas se distribuem no tempo ${cite('measnet', '§8.2.1')}. As flags com evidência são esse registro.</p>`)}
  ${section('falhas', '3.1 Duas falhas vistas na dispersão')}
  <p>Antes das regras, vale olhar: dois sensores na mesma altura devem concordar, e a dispersão entre eles é o teste de correlação ${cite('measnet', 'anexo B')}.</p>
  ${figure('12-dispersao-100m-antes', 'Dispersão entre os dois anemômetros de 100 m, antes do controle de qualidade: os pontos colados no eixo x são passos em que o anemômetro 2 lê zero com o 1 em pleno vento.')}
  ${figure('13-dispersao-cata-ventos', 'Dispersão entre os cata-ventos de 96 m e de 77 m: fora da diagonal, os passos em que o de 77 m parou.')}
  ${data(`<p>Os dois anemômetros de 100 m: R² ${scatterBefore.r2} com ${scatterBefore.steps} passos, por causa dos dias 17 e 18 de julho, quando o anemômetro 2 pifou. Os cata-ventos de 96 e 77 m: R² ${vanes.r2}. O de 77 m ficou travado de 25 de abril ao início de julho e voltou a girar depois.</p>`)}
  ${section('regras', '3.2 Flag por regras')}
  ${figure('14-flag-por-regras', 'Flag por regras: as regras do controle de qualidade e o resultado da execução.')}
  ${how([`Abra ${menu('Flag › Flag por regras…')}.`, 'Monte cada regra: o teste, os limites, a duração mínima, a flag e as colunas, e clique em <b>Adicionar regra</b>.', '<b>Testar todas</b> mostra o que seria marcado, sem marcar nada.', '<b>Executar todas</b> marca. Ctrl+Z desfaz.'])}
  <p>As regras deste exemplo:</p>
  <ol>
    <li><b>Fora da faixa</b>, velocidades abaixo de ${STALLED_BELOW_M_S} m/s ou a partir de 60 m/s: anemômetro parado ou leitura impossível.</li>
    <li><b>Sensor travado</b>, a mesma velocidade por ${STUCK_SPEED_HOURS} h (18 passos).</li>
    <li><b>Sensor travado</b>, a mesma direção por ${STUCK_VANE_HOURS} h (6 passos).</li>
    <li><b>Deriva entre anemômetros gêmeos</b>, no par de 100 m: o dia em que a razão média entre os dois se afasta mais de ${TWIN_TOLERANCE_PERCENT} % da razão do registro inteiro.</li>
    <li>Faixas físicas de temperatura (−30 a 50 °C), umidade (1 a 100 %) e pressão (500 a 1.100 hPa).</li>
  </ol>
  ${why(`<ul>
    <li><b>Velocidade abaixo de ${STALLED_BELOW_M_S} m/s é sensor parado, não calmaria.</b> Uma média de 10 minutos exatamente zero não acontece com o anemômetro funcionando, porque o offset de calibração impede leituras nulas. Uma longa sequência de zeros é o sinal típico de fio desconectado ${cite('nrel', '§9.1 e tab. 9.1')}.</li>
    <li><b>O mesmo valor por horas é sensor travado</b>, por gelo, rolamento ou falha do logger. É o teste de valor constante ${cite('measnet', 'anexo B')}. Para o cata-vento, o NREL dá o exemplo de um que fica parado em 180° por seis intervalos de 10 minutos e passa no teste de faixa, mas não no de desvio padrão ${cite('nrel', '§9.1')}.</li>
    <li><b>A deriva entre gêmeos</b> é o teste de relação entre sensores redundantes ${cite('nrel', 'tab. 9.2')}: pega o dia inteiro de um anemômetro que lê baixo sem chegar a zero, o que as faixas não pegam.</li>
    <li><b>Faixas físicas</b> são o teste de faixa: os limites devem incluir quase todos os valores esperados para o local, e se afinam com a experiência ${cite('nrel', 'tab. 9.1')}.</li>
  </ul><p>Testada nos quatro pares, a deriva também dispara nos de 80, 60 e 40 m. Esses disparos são da sombra da torre, não dos sensores (seção 8.3): por isso, aqui ela só se executa no par de 100 m, e nos outros depois da flag de sombra.</p><p>A faixa de uma regra é semiaberta, [mínimo; máximo): o máximo fica de fora. Por isso a umidade usa como máximo um valor logo acima de 100, e o ar saturado (100 %) continua válido.</p>`)}
  ${data(`${table(['Regra', 'Coluna', 'Trechos', 'Passos'], qcTable)}<p>A deriva marcou ${topTwin.steps} passos do anemômetro 2 (${dayCount(topTwin.steps)} dias inteiros). Ficaram com flag ${flagged[found.rulesRun.includes('Anemómetro 2') ? Object.keys(flagged).find((key) => key.endsWith('Anemómetro 2)')) : '']?.[0] ?? '—'} dos passos do anemômetro 2 e ${flagged[Object.keys(flagged).find((key) => key.endsWith('Veleta 2)'))]?.[0] ?? '—'} dos do cata-vento de 77 m. Temperatura, umidade e pressão não tiveram nada fora das faixas.</p>`)}
  ${section('efeito-qc', '3.3 O efeito do controle de qualidade')}
  ${figure('21-dispersao-100m-depois', 'Os dois anemômetros de 100 m depois do controle de qualidade.')}
  ${data(`<p>A dispersão entre os anemômetros de 100 m vai de R² ${scatterBefore.r2} para ${scatterAfter.r2} (y = ${scatterAfter.slope}·x, ${scatterAfter.steps} passos). A média do anemômetro 1 quase não muda (${beforeQc.mean} m/s antes e ${afterQc.mean} depois), porque as falhas foram de outros sensores.</p>`)}
  ${why(`<p>O controle de qualidade não serve só para mudar a média: ele decide quais sensores valem em cada trecho. Sem ele, o cisalhamento, a sombra da torre e a média dos dois anemômetros de 100 m sairiam contaminados pelos dois dias de anemômetro parado e pelos dois meses de cata-vento travado. A MEASNET considera essa etapa essencial, porque falhas de sensor levam a uma estimativa ruim do recurso ${cite('measnet', 'anexo B')}.</p>`)}
  ${section('inspecionar', '3.4 Inspecionar e remover flags')}
  ${figure('15-inspecionar-flags', 'Inspecionar e remover flags: quantos passos de cada coluna têm cada flag, e a parcela do registro.')}
  ${section('manual', '3.5 Flag manual')}
  ${figure('16-flag-manual', 'Flag manual: escolha o trecho no gráfico (arrastando) ou na tabela de valores, as colunas e a flag, e aplique ao segmento.')}
  ${why(`<p>Nem todo defeito cabe numa regra, como um degrau depois de uma manutenção. A verificação dos dados suspeitos é uma decisão caso a caso de uma pessoa qualificada: manter, rejeitar ou substituir pelo sensor redundante ${cite('nrel', '§9.1')}. A flag manual registra essa decisão.</p>`)}
  ${section('dispersao', '3.6 Flag por dispersão')}
  ${figure('17-flag-por-dispersao', 'Flag por dispersão: os dois anemômetros de 100 m um contra o outro. Um retângulo ou um laço seleciona os pontos discrepantes.')}
  ${why(`<p>Dois anemômetros redundantes devem concordar. Como exemplo de critério, o NREL usa diferença média de até ${formatLocal(REDUNDANT_DIFFERENCE_MS, 1)} m/s ${cite('nrel', 'tab. 9.2')}. Um ponto longe da diagonal é um passo em que um deles errou.</p>`)}
</section>`);

const fitRows = fits.filter((row) => row[0] !== 'Dados medidos').map((row) => row.slice(0, 7));
sections.push(`
<section>
  ${chapter('analises', '4º Análises')}
  ${section('distorcao', '4.1 Distorção da torre')}
  <p><b>O que é.</b> ${menu('Analisar › Distorção da torre')} calcula, para cada par de anemômetros na mesma altura, a razão (ou a diferença) entre as velocidades em cada um dos 72 setores de 5° da direção. Dois fatores resumem o resultado ${cite('windographer')}:</p>
  <ul>
    <li><b>fator de distorção</b>: média de |mediana da razão − 1|, ponderada pelo número de passos;</li>
    <li><b>fator de dispersão</b>: média ponderada do desvio padrão da razão dentro de cada setor.</li>
  </ul>
  ${figure('23-distorcao-da-torre', 'Distorção da torre dos quatro pares, com o anemômetro 1 a partir de 4 m/s (Filtrar por › Coluna de dados).')}
  ${data(table(['Par', 'Passos (todos)', 'Distorção', 'Dispersão', 'Dispersão a partir de 4 m/s'], distortionAll.map((row, index) => [pairName(row.pair), row.steps, row.distortion, row.scatter, distortionAbove4[index]?.scatter ?? '—'])))}
  ${why(`<p>A torre é um obstáculo: o anemômetro do lado de sotavento lê menos, e o efeito foi medido em túnel de vento ${cite('orlando')}. A distorção mostra de que direções isso acontece e quanto pesa, para decidir o que marcar (capítulo 8).</p><p>Os fatores precisam de vento: com velocidades quase nulas a razão fica instável (0,5 / 0,02 = 25). Por isso se leem a partir de 4 m/s.</p>`)}

  ${section('cisalhamento', '4.2 Cisalhamento do vento')}
  <p><b>O que é.</b> ${menu('Analisar › Cisalhamento do vento')} ajusta a variação da velocidade com a altura por duas leis:</p>
  <ul>
    <li>lei de potência: U(z) = U(z<sub>r</sub>)·(z/z<sub>r</sub>)<sup>α</sup>, com α = ln(U<sub>1</sub>/U<sub>2</sub>)/ln(z<sub>1</sub>/z<sub>2</sub>) ${cite('measnet', '§9.4, eq. 2')};</li>
    <li>lei logarítmica: U(z) = (u<sub>*</sub>/κ)·ln(z/z<sub>0</sub>) ${cite('stull')}, ${cite('manwell')}.</li>
  </ul>
  <p>O ajuste pode ser feito no perfil médio ou subdividido por mês, hora do dia, setor de direção e outras variáveis.</p>
  ${figure('24-cisalhamento', 'Cisalhamento do mastro: o perfil médio das quatro alturas, a lei de potência ajustada e α por hora do dia e por setor de direção.')}
  ${data(`<p>α ${shear.alpha} (intervalo de confiança ${shear.band}) e z<sub>0</sub> ${shear.z0} m, com ${shear.steps} passos em que as quatro alturas têm dado. Entre alturas vizinhas, α fica entre ${formatLocal(Math.min(...neighbourAlphas), 3)} e ${formatLocal(Math.max(...neighbourAlphas), 3)}: o perfil segue bem uma única lei de potência de 40 a 100 m.</p>`)}
  ${why(`<p>O cisalhamento leva a velocidade da altura medida à altura da turbina. A MEASNET pede ${cite('measnet', '§9.4')}:</p><ul><li>calcular em classes de 1 m/s e setores de até 30°, com velocidades acima de 3 m/s;</li><li>avaliar sem viés sazonal ou diurno;</li><li>corrigir a sombra do mastro e os setores perturbados, ou excluí-los do cálculo.</li></ul><p>Também pede alturas que cubram o rotor, com ao menos 20 m entre elas ${cite('measnet', '§7.2')}, como as 40, 60, 80 e 100 m deste mastro.</p>`)}

  ${section('distribuicao', '4.3 Distribuição de velocidade')}
  <p><b>O que é.</b> ${menu('Analisar › Distribuição de velocidade')} ajusta a distribuição de Weibull aos dados por quatro algoritmos e compara as curvas com o histograma medido.</p>
  ${figure('25-distribuicao', 'Distribuição de velocidade do anemômetro 1 (100 m): os quatro ajustes de Weibull e o histograma medido.')}
  ${data(table(['Algoritmo', 'k', 'A (m/s)', 'Média (m/s)', 'Acima da média', 'Potência (W/m²)', 'R²'], fitRows))}
  <p>A densidade de Weibull é f(U) = (k/A)·(U/A)<sup>k−1</sup>·exp[−(U/A)<sup>k</sup>], e a acumulada é F(U) = 1 − exp[−(U/A)<sup>k</sup>]. A é o fator de escala, ligado à média; k é o fator de forma ${cite('manwell')}. Os algoritmos:</p>
  <ul>
    <li><b>Máxima verossimilhança</b>: os k e A que tornam o conjunto observado o mais provável ${cite('stevens')}.</li>
    <li><b>Mínimos quadrados</b>: a reta de ln[−ln(1 − F)] contra ln U, sensível às classes de baixa velocidade ${cite('justus')}.</li>
    <li><b>WAsP</b>: reproduz a densidade de potência medida (média de U³) e a fração de passos acima da média ${cite('troen')}.</li>
    <li><b>Openwind</b>: reproduz a média de U e a densidade de potência ${cite('windographer')}.</li>
  </ul>
  ${why(`<p>A energia vem de U³: um ajuste que erra a cauda dos ventos fortes erra a energia, mesmo com um R² bom. WAsP e Openwind preservam a densidade de potência medida (${measuredFit[4]} W/m² a 1,225 kg/m³). Neste mastro de vento forte e regular, os quatro ajustes concordam. Num local de vento fraco e muitas calmarias eles se separam, e os mínimos quadrados costumam errar a energia.</p><p>A MEASNET pede que a forma de ajustar a Weibull seja descrita e justificada, com atenção às velocidades altas ${cite('measnet', '§8.5')}. A tabela usa ar padrão (1,225 kg/m³); o histograma usa a densidade medida, ${afterQc.density} kg/m³, e por isso dá ${afterQc.power} W/m².</p>`)}

  ${section('turbulencia', '4.4 Turbulência')}
  <p><b>O que é.</b> ${menu('Analisar › Turbulência')} calcula a intensidade de turbulência de cada passo, TI = σ/U, e a agrupa por classe de velocidade, direção, mês, hora ou altura ${cite('measnet', '§9.3, eq. 1')}. O resultado é comparado com as categorias da IEC 61400-1 ${cite('iec1')}.</p>
  ${figure('26-turbulencia', 'Turbulência do anemômetro 1 por velocidade, com as curvas das categorias IEC A+, A, B e C.')}
  ${data(`<p>Na classe de 15 m/s: TI média ${turbulence.mean} e TI representativa ${turbulence.figure}, categoria ${turbulence.category}, com ${turbulence.steps} passos.</p>`)}
  ${why(`<p>A IEC 61400-1 define as categorias pela TI de referência a 15 m/s (A+ 0,18; A 0,16; B 0,14; C 0,12) e o modelo normal de turbulência para a TI representativa ${cite('iec1')}, ${cite('windographer')}. A MEASNET pede a TI por setor de 30° e classe de 1 m/s, onde houver dados suficientes ${cite('measnet', '§9.3')}. O número de passos é um componente da incerteza ${cite('measnet', 'tab. 4')}: aqui, com centenas de passos a 15 m/s, a categoria tem base.</p>`)}

  ${section('extremos', '4.5 Ventos extremos')}
  ${figure('27-ventos-extremos', 'Ventos extremos: o ajuste de Gumbel aos máximos e o vento de 50 anos.')}
  ${data(`<p>Vento de 50 anos: ${extremes.v50} m/s, a partir de ${extremes.peaks} máximos mensais.</p>`)}
  ${why(`<p>O vento de 50 anos (V<sub>50</sub>, média de 10 min) é um critério da classe da turbina e entra no projeto estrutural ${cite('iec1')}. Ele vem da teoria de valores extremos ${cite('gumbel')}, que admite vários métodos ${cite('palutikof')}.</p><p>A MEASNET recomenda cerca de sete anos de série para extrapolar extremos medidos ${cite('measnet', '§8.5')}. Com ${extremes.peaks} máximos mensais, este V<sub>50</sub> é só uma indicação; o valor de projeto pede uma série longa de referência.</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('extrapolacao', '5º Extrapolação vertical')}
  <p><b>O que é.</b> ${menu('Revisar › Extrapolação vertical…')} sintetiza a velocidade em outras alturas, passo a passo, a partir do cisalhamento. Se pedido, também o desvio padrão, a direção e a temperatura.</p>
  ${figure('28-extrapolacao-vertical', 'Extrapolação vertical para 120 m: o processo (a cada passo, ou só a tabela mês × hora), as velocidades usadas no ajuste e de qual se extrapola.')}
  ${how(['Informe as alturas de destino.', 'Escolha o processo: calcular α em cada passo, com a tabela mês × hora como reserva, ou usar só a tabela.', 'Escolha as velocidades do ajuste e de onde extrapolar: o sensor mais próximo ou a reta de melhor ajuste.', 'Opcional: restrinja α a uma faixa e marque as novas colunas com uma flag.'])}
  ${why(`<p>A turbina não está na altura medida. Extrapolar passo a passo preserva a relação entre a hora do dia e o cisalhamento, que um α médio perde. A IEC 61400-1 pede o cisalhamento sobre a área do rotor, e a MEASNET pede que o α medido seja sempre informado junto com as alturas usadas ${cite('measnet', '§9.4')}.</p><p>A nova coluna é derivada: herda as flags dos sensores que lê. Com quatro alturas de 40 a 100 m, subir para 120 m é um passo curto. Quanto maior a distância entre a medição e o cubo, maior a incerteza.</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('longo-prazo', '6º Ajuste de longo prazo (MCP)')}
  <p><b>O que é.</b> ${menu('Revisar › Ajuste de longo prazo…')} correlaciona o local com uma referência de longa duração no período em que os dois têm dado. Depois usa essa relação para estender o local por todo o período da referência: medir, correlacionar, prever ${cite('measnet', '§8.3')}.</p>
  <p>A referência é outro dataset do workbook: outro mastro com muitos anos, ou uma reanálise baixada dos próprios portais (NASA POWER, ERA5), que o OpenWindAxis abre como qualquer arquivo. Com um só dataset, a janela diz:</p>
  <blockquote>${escapeHtml(found.longTerm)}</blockquote>
  ${how(['Abra a série de referência com Arquivo › Adicionar dataset(s)…', `Em ${menu('Revisar › Ajuste de longo prazo…')}, escolha o local, a referência e as colunas.`, 'Compare os métodos na tabela: a janela valida cada um deixando um mês de fora por vez e recomenda o de menor erro.'])}
  ${why(`<p>${dataset.Dias} dias num local não são o clima: a variação interanual exige uma base de muitos anos ${cite('measnet', '§8.3')}. Os requisitos da MEASNET:</p><ul><li>concorrência de ao menos um ano ${cite('measnet', '§8.3.3')};</li><li>referência de cerca de dez anos ${cite('measnet', '§8.3.4')};</li><li>correlação mínima recomendada R = ${formatLocal(MEASNET_MIN_R, 1)} ${cite('measnet', '§8.3.2')}.</li></ul><p>O método também importa. A regressão linear encolhe a variância das velocidades previstas e subestima U³; a razão de variância foi proposta para preservá-la ${cite('rogers')}. SpeedSort ${cite('king')}, fatias verticais ${cite('leblanc')} e Matrix Time Series ${cite('lambert')} são alternativas não lineares, revisadas em ${cite('carta')}. A validação mês a mês é o que a MEASNET pede ao exigir MBE, MAE ou RMSE documentados ${cite('measnet', '§8.3.2')}.</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('exportar', '7º Exportar dados para o Openwind')}
  <p><b>O que é.</b> ${menu('Arquivo › Exportar dados…')} grava:</p>
  <ul>
    <li>a série em CSV;</li>
    <li>o clima de vento observado do WAsP (.tab) ${cite('troen')};</li>
    <li>o arquivo de torre do Openwind (MM 2.1);</li>
    <li>o arquivo da EPE para os leilões.</li>
  </ul>
  ${figure('30-exportar-openwind', 'Exportar dados, parte do Openwind: sensores de velocidade, direção e temperatura, período, as três médias e a escala para uma média-alvo.')}
  ${data(`<p>Média simples ${openwind.simple} m/s e média sem viés sazonal ${openwind.seasonal} m/s, com ${openwind.steps} passos exportados (os que têm todos os valores válidos). A média simples fica ${formatLocal(seasonalBias, PERCENT_DIGITS)} % acima da sem viés: o registro cobre só de janeiro a julho.</p>`)}
  ${why(`<p>O Openwind modela o parque a partir da série da torre. O arquivo leva só os passos com velocidade, direção e temperatura válidas, já sem o que recebeu flag. As médias mostram o viés sazonal de um período que não cobre o ano, antes de a série ser escalada para uma média de longo prazo (capítulo 6).</p>`)}
</section>`);

const ruleRowsText = (rules) => rules.map((rule) => `<li>${escapeHtml(sensorName(rule.sensor))}: à sombra em ${centerOf(rule)}° (${rule.from}° a ${rule.to}° do ${escapeHtml(sensorName(rule.vane))});</li>`).join('');
const twinTable = twinBefore.map((before) => {
  const after = twinAfter.find((row) => row.column === before.column);
  return [sensorName(before.column), `${before.steps} passos (${dayCount(before.steps)} dias)`, `${after.steps} passos (${dayCount(after.steps)} dias)`];
});
sections.push(`
<section>
  ${chapter('sombra', '8º Sombra da torre')}
  <p><b>O que é.</b> ${menu('Flag › Sombra da torre…')} gera regras que marcam com a flag "Sombra da torre" os passos em que o vento vem do lado em que a torre encobre cada anemômetro. Os setores podem vir da <b>orientação da lança</b> ou do <b>padrão de sombra detectado</b> nos dados.</p>
  ${section('padrao', '8.1 Pelo padrão detectado')}
  ${figure('31-sombra-padrao-detectado', 'Sombra da torre pelo padrão detectado, par de 100 m. À esquerda, a mediana da razão entre os dois anemômetros nos 72 setores; à direita, a razão de cada passo. Em amarelo o setor de um, em verde o do outro; embaixo, as regras geradas.')}
  ${how(['Escolha o dataset (a torre) no alto da janela.', 'Marque "padrão de sombra detectado" e escolha o par na lista.', 'Confira os centros e as larguras dos setores (o padrão é 30°) e o cata-vento das regras.', 'Escolha a flag e clique em <b>Executar regras</b>.'])}
  ${data(`<p>Os setores detectados nos quatro pares:</p><ul>${ruleRowsText(shadingRules)}</ul><p>O par de 80 m usa o cata-vento de 96 m: o de 77 m, o mais próximo, ficou travado de abril a julho, e com ele a regra marcaria o anemômetro errado durante dois meses. As regras marcaram ${shadingRun.steps} passos em ${shadingRun.runs} trechos.</p>`)}
  ${why(`<p>O sistema calcula a razão entre os dois sensores em cada passo e a mediana dessa razão em cada setor. Onde a mediana é mínima, o primeiro está atrás da torre; onde é máxima, o segundo ${cite('windographer')}. Esses são os centros dos setores. Detectar pelo dado dispensa saber a orientação exata das lanças.</p><p>A flag não corrige o vento. Ela tira o sensor encoberto da análise naquele setor, e o outro anemômetro cobre o setor. É a substituição pelo sensor redundante que o NREL recomenda, com um código de rejeição próprio para a sombra da torre ${cite('nrel', '§9.1 e tab. 9.4')}.</p>`)}
  ${section('lanca', '8.2 Pela orientação da lança')}
  ${figure('32-sombra-pela-lanca', 'Sombra da torre pela orientação da lança. Com a lança do anemômetro 1 a 0° (norte), ele fica à sombra com o vento do sul.')}
  ${data(`<p>Com a lança a 0° e largura de 30°, o anemômetro fica à sombra de ${boomFrom} a ${boomTo}.</p>`)}
  ${why(`<p>O setor sombreado é o oposto da lança, a 180° dela. As regras de montagem existem para manter o sensor fora da esteira na maior parte das direções:</p><ul><li>o NREL pede lanças de ao menos três larguras de torre treliçada ou seis diâmetros de torre tubular, voltadas para o vento predominante ${cite('nrel', '§6.7')};</li><li>a MEASNET pede as lanças a 45° do vento predominante em torre tubular e a 90° em torre treliçada ${cite('measnet', '§7.2')};</li><li>a IEC 61400-12-1 detalha a montagem no seu anexo G ${cite('iec12', 'anexo G')}.</li></ul><p>Em torres treliçadas, medições em túnel e CFD mostraram que o método do anexo G superestima a distorção ${cite('stickland')}.</p>`)}

  ${section('ordem', '8.3 A ordem importa: sombra antes da deriva entre gêmeos')}
  <p>A regra <b>Deriva entre anemômetros gêmeos</b> (seção 3.2) compara cada anemômetro com o seu par, dia a dia. Nos pares de 80, 60 e 40 m ela foi testada antes da flag de sombra e executada depois dela.</p>
  ${data(`${table(['Anemômetro', 'Testada antes da flag de sombra', 'Executada depois dela'], twinTable)}<p>Antes da flag de sombra, a regra dispara em dias de vento soprando quase todo do setor sombreado: a razão do dia muda pela torre, não pelo sensor. Depois dela, os passos sombreados saem da comparação, e o que sobraria seria defeito de sensor.</p>`, 'Nos dados do mastro: a mesma regra, antes e depois')}
  ${why(`<p>A sombra da torre muda a razão entre os dois anemômetros conforme a direção ${cite('orlando')}. Um teste que compare os gêmeos sem tirar a sombra antes confunde direção com defeito. Rode a sombra primeiro.</p>`)}
  ${figure('33-sombra-executada', 'As regras da sombra da torre executadas nos quatro pares.')}
  ${figure('34-inspecionar-sombra', 'Inspecionar e remover flags depois da sombra e da deriva: a parcela de cada anemômetro com cada flag.')}

  ${section('sem-flag', '8.4 Quando a sombra pode ficar sem flag?')}
  <p>As referências não dão um limite de distorção abaixo do qual o dado sombreado possa ficar sem tratamento. O que elas dizem:</p>
  <ul>
    <li><b>MEASNET</b>: não há uma definição geral de dado errôneo. O anemômetro sob a sombra do mastro pode ser tratado como erro e eliminado para alguns fins, e mantido para outros, sobretudo se o efeito do mastro for corrigido ${cite('measnet', '§8.2.1')}. A distorção na esteira do mastro deve entrar na estimativa da incerteza ${cite('measnet', '§7.2')}. No cisalhamento, os setores perturbados devem ser corrigidos ou excluídos ${cite('measnet', '§9.4')}.</li>
    <li><b>NREL</b>: dá como critério de exemplo, para dois anemômetros redundantes, uma diferença média de até ${formatLocal(REDUNDANT_DIFFERENCE_MS, 1)} m/s, e recomenda substituir o valor rejeitado pelo do sensor redundante ${cite('nrel', 'tab. 9.2, tab. 9.4 e §9.1')}.</li>
    <li><b>IEC 61400-12-1</b>: o anexo G trata da montagem dos instrumentos no mastro, para limitar a distorção do fluxo no sensor ${cite('iec12', 'anexo G')}. É um critério de instalação, não de aceitação do dado. Dentro da esteira, o anemômetro lê sistematicamente menos ${cite('orlando')}.</li>
  </ul>
  ${data(`<p>A diferença média entre os dois anemômetros de 100 m é ${signed(difference.overall, DIFFERENCE_DIGITS)} m/s no registro todo e, nos setores sombreados, ${signed(difference.shadedA, DIFFERENCE_DIGITS)} m/s (sombra do anemômetro 1) e ${signed(difference.shadedB, DIFFERENCE_DIGITS)} m/s (sombra do anemômetro 2).</p>
  <p>Para medir o efeito na velocidade de 100 m, comparamos a média dos dois anemômetros (coluna calculada "Vel 100 m média dos dois") antes e depois de executar as regras da sombra. Depois delas, a média usa só o anemômetro descoberto no setor sombreado:</p>
  ${table(['', 'Passos', 'Média (m/s)', 'Densidade de potência (W/m², ar padrão)'], [['Média dos dois, sem a flag de sombra', averageBefore.steps, formatLocal(averageBefore.mean, 3), formatLocal(averageBefore.power, 1)], ['Com a flag de sombra', averageAfter.steps, formatLocal(averageAfter.mean, 3), formatLocal(averageAfter.power, 1)]])}
  <p>Sem a flag, a velocidade média de 100 m fica ${signed(meanBias, 2)} % e a densidade de potência ${signed(powerBias, 2)} % em relação ao valor com a sombra tratada.</p>`, 'Nos dados do mastro: quanto a sombra pesa')}
  ${why(`<p>A decisão depende da finalidade ${cite('measnet', '§8.2.1')}. Neste mastro, os dois anemômetros de 100 m quase não diferem nem nos setores sombreados (±${formatLocal(Math.max(Math.abs(difference.shadedA), Math.abs(difference.shadedB)), DIFFERENCE_DIGITS)} m/s). A sombra muda pouco a média dos dois, e o efeito cabe na incerteza da medição, desde que declarado ${cite('measnet', '§7.2')}.</p><p>Isso muda de mastro para mastro: dentro da esteira de uma torre o anemômetro pode ler bem menos ${cite('orlando')}, e então a média dos dois cai alguns por cento na velocidade e o triplo disso na energia, que vai com U³. Para o cisalhamento, a regra vale sempre: os setores perturbados se corrigem ou se excluem ${cite('measnet', '§9.4')}. No OpenWindAxis, tratar a sombra é executar as regras desta janela e usar uma velocidade que deixe de fora o sensor marcado, como a média de colunas ou "Reconstruir e média".</p>`)}
</section>`);

sections.push(`
<section>
  ${chapter('salvar', '9º Salvar, reabrir e repetir em outras torres')}
  <ul>
    <li>${menu('Arquivo › Salvar workbook')} grava um .wproj com os arquivos de origem, a configuração, as flags com as evidências, as regras e a exibição das colunas. Ao reabrir, tudo volta como estava.</li>
    <li>O navegador guarda a sessão: um F5 ou o fechamento da aba não perdem o trabalho. Os últimos workbooks e arquivos ficam em ${menu('Arquivo › Abertos recentemente')}.</li>
    <li>${menu('Fluxos › Gerenciar fluxos…')} grava a sequência de edições, regras e análises de uma torre e a executa nas outras, para que todas recebam o mesmo controle de qualidade.</li>
  </ul>
  ${why(`<p>Um tratamento de dados só é confiável se puder ser refeito e conferido. A MEASNET classifica a integridade dos dados pela possibilidade de verificar a cadeia de processamento ${cite('measnet', '§8.1')}. O workbook guarda as decisões junto com os dados, e o fluxo garante que todas as torres passem pelas mesmas regras.</p>`)}
</section>`);

const references = citedOrder.map((key, index) => `<li id="ref-${key}"><span class="number">[${index + 1}]</span> ${SOURCES[key]}</li>`).join('\n');
sections.push(`
<section>
  ${chapter('referencias', 'Referências')}
  <ol class="references">${references}</ol>
  <p class="colophon">Esta ajuda foi gerada em ${GENERATED_ON} pelo próprio OpenWindAxis, rodando no navegador sobre os arquivos do logger. Os scripts <code>docs/ajuda/capturar.mjs</code> e <code>docs/ajuda/montar.mjs</code> percorrem as janelas e montam a página.</p>
</section>`);

const nav = toc
  .map((entry) => `<li><a href="#${entry.id}">${entry.title}</a>${entry.sections.length ? `<ul>${entry.sections.map((item) => `<li><a href="#${item.id}">${item.title}</a></li>`).join('')}</ul>` : ''}</li>`)
  .join('');

const CSS = `
:root { --brand: #0f5d6b; --brand-soft: #e6f1f3; --amber-soft: #fdf4e3; --amber: #9a6700; --ink: #1b232d; --muted: #5b6770; --rule: #d5dde2; --surface: #ffffff; --page: #f4f6f8; --code: #eef1f3; }
@media (prefers-color-scheme: dark) {
  :root { --brand: #5fb3c2; --brand-soft: #16323a; --amber-soft: #33290f; --amber: #e0b04f; --ink: #e3e8ec; --muted: #9aa6b0; --rule: #34404a; --surface: #1a2026; --page: #11161b; --code: #232b33; }
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; scroll-padding-top: 12px; }
body { margin: 0; color: var(--ink); background: var(--page); font: 15px/1.6 "Segoe UI", system-ui, -apple-system, Arial, sans-serif; }
.layout { display: grid; grid-template-columns: 280px minmax(0, 1fr); min-height: 100vh; }
nav.toc { position: sticky; top: 0; align-self: start; height: 100vh; overflow-y: auto; padding: 20px 16px 32px; background: var(--surface); border-right: 1px solid var(--rule); font-size: 13.5px; }
nav.toc .brand { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; font-weight: 600; color: var(--brand); }
nav.toc .brand svg { width: 28px; height: 28px; }
nav.toc ul { list-style: none; margin: 0; padding: 0; }
nav.toc li { margin: 2px 0; }
nav.toc ul ul { margin: 2px 0 8px 12px; }
nav.toc a { color: var(--ink); text-decoration: none; display: block; padding: 2px 6px; border-radius: 4px; }
nav.toc ul ul a { color: var(--muted); }
nav.toc a:hover { background: var(--brand-soft); color: var(--ink); }
main { max-width: 900px; width: 100%; margin: 0 auto; padding: 24px 32px 64px; }
.cover { display: flex; gap: 20px; align-items: center; padding: 24px 0 8px; }
.cover .logo svg { width: 72px; height: 72px; }
.cover .product { margin: 0; font-weight: 600; letter-spacing: .06em; color: var(--brand); }
.cover h1 { margin: 2px 0 6px; font-size: 26px; line-height: 1.25; color: var(--brand); }
.cover .subtitle { margin: 0; color: var(--muted); }
h2, h3, h4 { color: var(--brand); line-height: 1.3; }
h2 { font-size: 22px; margin: 40px 0 12px; padding-bottom: 6px; border-bottom: 2px solid var(--brand); }
h3 { font-size: 17px; margin: 28px 0 8px; }
h4 { font-size: 15px; margin: 18px 0 6px; }
p { margin: 8px 0; }
ul, ol { margin: 8px 0 8px 22px; padding: 0; }
li { margin: 3px 0; }
a { color: var(--brand); }
a.cite { text-decoration: none; font-size: 12.5px; white-space: nowrap; }
a.cite:hover { text-decoration: underline; }
figure { margin: 16px auto; text-align: center; }
figure img { max-width: 100%; height: auto; border: 1px solid var(--rule); border-radius: 6px; background: #fff; }
figure.narrow img { max-width: 70%; }
figcaption { font-size: 13px; color: var(--muted); margin-top: 6px; }
aside { border-radius: 8px; padding: 10px 14px; margin: 14px 0; }
aside h4 { margin: 0 0 4px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; }
aside.why { background: var(--brand-soft); border-left: 4px solid var(--brand); }
aside.data { background: var(--amber-soft); border-left: 4px solid var(--amber); }
aside.data h4 { color: var(--amber); }
ol.how { background: var(--surface); border: 1px solid var(--rule); border-radius: 8px; padding: 10px 14px 10px 34px; }
table { border-collapse: collapse; width: 100%; margin: 10px 0; font-size: 13.5px; background: var(--surface); }
th, td { border-bottom: 1px solid var(--rule); padding: 5px 8px; text-align: left; vertical-align: top; }
th { background: var(--code); font-weight: 600; }
td:not(:first-child) { font-variant-numeric: tabular-nums; }
table.capabilities td:first-child { font-weight: 600; color: var(--brand); white-space: nowrap; }
table.compare td:nth-child(n+2) { text-align: right; }
.menu { font-weight: 600; white-space: nowrap; }
.menu .sep { color: var(--muted); font-weight: 400; }
code { font: 13px Consolas, "Courier New", monospace; background: var(--code); padding: 0 4px; border-radius: 3px; }
ol.references { list-style: none; margin-left: 0; }
ol.references li { margin: 8px 0; padding-left: 44px; text-indent: -44px; }
ol.references li:target { background: var(--amber-soft); border-radius: 6px; }
ol.references .number { display: inline-block; width: 44px; text-indent: 0; color: var(--brand); font-weight: 600; }
.colophon { color: var(--muted); font-size: 13px; margin-top: 24px; }
@media (max-width: 900px) {
  .layout { display: block; }
  nav.toc { position: static; height: auto; border-right: 0; border-bottom: 1px solid var(--rule); }
  main { padding: 16px; }
  figure.narrow img { max-width: 100%; }
}
@media print { nav.toc { display: none; } .layout { display: block; } body { background: #fff; } }
`;

const html = `<!doctype html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${TITLE}</title>
<link rel="icon" href="../favicon.svg">
<style>${CSS}</style>
</head>
<body>
<div class="layout">
<nav class="toc" aria-label="Índice"><div class="brand">${LOGO}<span>Ajuda do OpenWindAxis</span></div><ul>${nav}</ul></nav>
<main>${sections.join('\n')}</main>
</div>
</body>
</html>`;

writeFileSync(OUTPUT, html);
console.log(`ajuda: ${OUTPUT} (${figureNumber} figuras, ${citedOrder.length} referências)`);
const uncited = Object.keys(SOURCES).filter((key) => !citedOrder.includes(key));
if (uncited.length) console.log(`fontes sem citação: ${uncited.join(', ')}`);

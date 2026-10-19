// Every check in one command, in the order a change should pass them: the Rust
// tests, the TypeScript types, the core through the WASM build, the native CLI
// against the browser's results and the published formulas, and (with a Chrome
// path and the dev server's URL) the real UI end to end. Prints one line per
// check and exits with 1 when any failed.
// Usage, from the repository root (after `npm run wasm` in web/ and
// `cargo build --release -p wind-cli`):
//   node bench/check.mjs [--skip-rust] [--ui <chrome.exe> <app url>]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const argumentsGiven = process.argv.slice(2);
const skipRust = argumentsGiven.includes('--skip-rust');
const uiAt = argumentsGiven.indexOf('--ui');
const [chromePath, appUrl] = uiAt >= 0 ? argumentsGiven.slice(uiAt + 1, uiAt + 3) : [];
const isWindows = process.platform === 'win32';
const wind = join(root, 'target', 'release', isWindows ? 'wind.exe' : 'wind');
const wasm = join(root, 'web', 'src', 'wasm');
const sample = join(root, 'samples', 'torre_exemplo.csv');
/** The browser check's verdict line when nothing went wrong. */
const UI_PASSED = /--- problems ---\s*none/;
/** The core checks that read the WASM build, run as `node bench/<name>_check.mjs web/src/wasm`. */
const WASM_CHECKS = ['filter', 'rules', 'edits', 'history', 'derived', 'confidence', 'methods', 'flag_stats', 'wasp_tab'];

/** One check: a label, the command, and whether its output also has to show success. */
const checks = [];
const add = (label, command, args, options = {}) => checks.push({ label, command, args, ...options });
if (!skipRust) add('Rust: cargo test', 'cargo', ['test', '--quiet']);
add('TypeScript: tsc', 'npx', ['tsc', '--noEmit', '-p', '.'], { shell: isWindows, cwd: join(root, 'web') });
for (const name of WASM_CHECKS) add(`núcleo: ${name}`, 'node', [`bench/${name}_check.mjs`, 'web/src/wasm'], { needs: wasm });
add('núcleo: front', 'node', ['bench/front_check.mjs', 'web']);
add('núcleo: project', 'node', ['bench/project_check.mjs', 'web'], { needs: wasm });
add('CLI: cli', 'node', ['bench/cli_check.mjs', 'web', wind], { needs: wind });
add('CLI: recipe', 'node', ['bench/recipe_check.mjs', 'web/src/wasm', wind, 'samples/torre_exemplo.csv'], { needs: wind });
add('fórmulas: crosscheck.py', isWindows ? 'python' : 'python3', ['-I', 'bench/crosscheck.py', sample, wind], { needs: wind });
let uiOut;
if (chromePath && appUrl) {
  uiOut = mkdtempSync(join(tmpdir(), 'openwindaxis-ui-'));
  add('interface: ui_check', 'node', ['bench/ui_check.mjs', chromePath, appUrl, sample, uiOut], { verdict: UI_PASSED });
}

const failed = [];
for (const check of checks) {
  if (check.needs && !existsSync(check.needs)) {
    console.log(`– ${check.label}: pulado (falta ${check.needs})`);
    continue;
  }
  const started = Date.now();
  const run = spawnSync(check.command, check.args, { cwd: check.cwd ?? root, encoding: 'utf8', shell: check.shell ?? false, maxBuffer: 1 << 28 });
  const output = `${run.stdout ?? ''}${run.stderr ?? ''}`;
  const passed = run.status === 0 && (!check.verdict || check.verdict.test(output));
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  console.log(`${passed ? '✓' : '✗'} ${check.label} (${seconds} s)`);
  if (!passed) {
    failed.push(check.label);
    console.log(output.trim().split('\n').slice(-20).map((line) => `    ${line}`).join('\n'));
  }
}
if (uiOut) rmSync(uiOut, { recursive: true, force: true });
if (!chromePath) console.log('– interface: ui_check pulado (passe --ui <chrome.exe> <url do servidor de desenvolvimento>)');
console.log(failed.length ? `falharam: ${failed.join(', ')}` : 'tudo passou');
process.exit(failed.length ? 1 : 0);

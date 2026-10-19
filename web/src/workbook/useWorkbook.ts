/**
 * The workbook's state and every action on it, for the whole main window:
 * import, re-import with corrections, reorder, remove, flag edits with undo,
 * project save/open and exports. Answers from the core are applied to the
 * entry's *current* state (functional updates), so a late answer never
 * undoes what changed while it was in flight.
 */
import { useEffect, useRef, useState } from 'react';
import { WindApi } from '../api/client';
import type { AppendStatusDto } from '../api/generated/AppendStatusDto';
import type { AppendedSourceDto } from '../api/generated/AppendedSourceDto';
import type { ColumnCorrectionDto } from '../api/generated/ColumnCorrectionDto';
import type { DatasetId } from '../api/generated/DatasetId';
import type { EditStateDto } from '../api/generated/EditStateDto';
import type { ImportResultDto } from '../api/generated/ImportResultDto';
import type { ImportConfigDto } from '../api/generated/ImportConfigDto';
import type { OpDto } from '../api/generated/OpDto';
import type { RecipeDto } from '../api/generated/RecipeDto';
import { CORE_VERSION, RECIPE_FORMAT } from '../api/generated/version';
import { initialFilter } from '../FilterBy';
import { errorMessage } from '../format';
import { PROJECT_EXTENSION, readProject, writeProject, type SavedDataset } from '../project';
import { addRecent, listRecent, loadRecent, loadSession, saveSession, type RecentEntry, type RecentKind } from './persist';
import { storedDefaultDisplay } from '../display';
import { datasetColor, UNTITLED_WORKBOOK } from '../shell/brand';
import { baseName, byName, DEFAULT_IMPORT_OPTIONS, defaultPlot, importConfig, isDerived, type Entry, type ImportJob, type ImportOptions } from './model';

/** How long a download link stays valid, ms. */
const DOWNLOAD_URL_LIFETIME_MS = 1000;
/** Byte order mark: makes Excel read the CSV as UTF-8. */
const BYTE_ORDER_MARK = '﻿';
/** Why a project cannot be saved: the browser reads sources from disk at save time. */
const SOURCE_UNREADABLE = 'o arquivo de origem mudou, foi movido ou apagado desde a importação; importe-o de novo para salvar o workbook';
/** Status while the last visit's session comes back. */
const RESTORING = 'Restaurando o workbook da última sessão…';
/** A recent entry whose stored files the browser dropped. */
const RECENT_GONE = 'Esse item recente não está mais guardado no navegador.';
/** Logged when the browser refuses its storage (a private window, a full disk). */
const STORE_UNAVAILABLE = 'armazenamento do navegador indisponível';
/** The session is saved this long after the last change, ms. */
const SESSION_SAVE_DELAY_MS = 1500;
/** Why a recipe cannot be written: it names one source file. */
const RECIPE_WITH_APPENDED = 'a receita guarda um único arquivo de origem, e este dataset tem arquivos anexados';
/** Why a flag edit had nothing to act on. */
const NO_MEASURED_COLUMNS = 'Escolha colunas medidas para marcar: as derivadas herdam as marcações das colunas de origem.';

/** A SHA-256 digest as text: two lowercase hex digits per byte. */
const HEX_RADIX = 16;
const HEX_DIGITS_PER_BYTE = 2;
/** Indentation of a saved recipe, spaces. */
const JSON_INDENT = 2;
/** The type that gets the byte order mark. */
const CSV_TYPE = 'text/csv';

/** Text as UTF-8; a CSV gets a byte order mark so Excel reads it as UTF-8,
 *  and nothing else does (JSON must not start with one, RFC 8259 §8.1). */
export const textFile = (text: string, type: string) => new Blob(type === CSV_TYPE ? [BYTE_ORDER_MARK, text] : [text], { type });

/** Hands `contents` to the browser as a download named `name`. */
export function download(name: string, contents: Blob) {
  const url = URL.createObjectURL(contents);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), DOWNLOAD_URL_LIFETIME_MS);
}

/** The bytes of `file` followed by those of `appended`, as the core's import
 *  reads them, with the size of each appended file. */
async function sourceBuffer(file: File, appended: File[]): Promise<{ bytes: Uint8Array; sources: AppendedSourceDto[] }> {
  const parts = await Promise.all([file, ...appended].map((part) => part.arrayBuffer()));
  const bytes = new Uint8Array(parts.reduce((total, part) => total + part.byteLength, 0));
  let offset = 0;
  for (const part of parts) {
    bytes.set(new Uint8Array(part), offset);
    offset += part.byteLength;
  }
  return { bytes, sources: appended.map((part, index) => ({ name: part.name, bytes: parts[index + 1].byteLength })) };
}

/** What became of the files appended to one dataset (Status do anexo). */
export interface AppendReport {
  dataset: string;
  statuses: AppendStatusDto[];
}

export function useWorkbook() {
  const [api, setApi] = useState<WindApi | null>(null);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [selectedId, setSelectedId] = useState<DatasetId | null>(null);
  const [busy, setBusy] = useState('');
  const [errors, setErrors] = useState<string[]>([]);
  /** The last append's report, until it is closed. */
  const [appendReport, setAppendReport] = useState<AppendReport | null>(null);
  /** File name the workbook was opened from or saved as. */
  const [workbookName, setWorkbookName] = useState(UNTITLED_WORKBOOK);
  /** Datasets ever added: each new one takes the next colour. */
  const added = useRef(0);
  /** The workbooks and files opened recently (Arquivo › Abertos recentemente). */
  const [recent, setRecent] = useState<RecentEntry[]>([]);
  /** Whether the last visit's session was looked for: until then nothing is saved over it. */
  const [restored, setRestored] = useState(false);

  useEffect(() => {
    WindApi.load()
      .then(setApi)
      .catch((thrown) => setErrors([`Falha ao carregar o núcleo WebAssembly: ${errorMessage(thrown)}`]));
    // Ask the browser not to evict the stored workbook under storage pressure.
    navigator.storage?.persist?.().catch(() => undefined);
  }, []);

  const idOf = (entry: Entry) => entry.result.dataset.id;
  const selected = entries.find((entry) => idOf(entry) === selectedId);

  /** Changes one entry starting from its current state. */
  const updateEntry = (id: DatasetId, change: (latest: Entry) => Partial<Entry>) =>
    setEntries((current) => current.map((entry) => (idOf(entry) === id ? { ...entry, ...change(entry) } : entry)));

  /** Imports each job as a new dataset; failures are listed, the rest still load. */
  async function addDatasets(jobs: ImportJob[]) {
    if (!api) return;
    const failures: string[] = [];
    const fresh: Entry[] = [];
    for (const { file, appended = [], config, restore, rules, display } of jobs) {
      setBusy(`Importando ${file.name}${appended.length ? ` e ${appended.length} arquivo(s) anexado(s)` : ''}…`);
      try {
        const { bytes, sources } = await sourceBuffer(file, appended);
        const result = await api.importCsv(bytes, { name: file.name, config, restore, appended: sources });
        if (appended.length && !restore) setAppendReport({ dataset: file.name, statuses: result.appended });
        fresh.push({
          file,
          appended,
          result,
          config,
          corrections: Object.fromEntries(config.columns.map((correction) => [correction.fileColumn, correction])),
          dirty: false,
          plotted: defaultPlot(result.dataset),
          canUndo: false,
          canRedo: false,
          rules: rules ?? [],
          filter: initialFilter(result.dataset),
          color: datasetColor(added.current++),
          visible: true,
          display: display ?? storedDefaultDisplay(),
        });
      } catch (thrown) {
        failures.push(`${file.name}: ${errorMessage(thrown)}`);
      }
    }
    setBusy('');
    setErrors(failures);
    if (fresh.length) {
      setEntries((current) => [...current, ...fresh]);
      setSelectedId(idOf(fresh[fresh.length - 1]));
    }
  }

  /** Adds raw data files, read with the detected settings. */
  function importFiles(files: FileList | File[] | null) {
    if (!files?.length) return;
    const config = importConfig(DEFAULT_IMPORT_OPTIONS, {});
    const jobs = Array.from(files).map((file) => ({ file, config }));
    jobs.forEach((job) => remember('file', job.file.name, [job]));
    return addDatasets(jobs);
  }

  /** Adds one dataset from a folder of a logger's files: the first by name
   *  read as the dataset, the others appended to it (Arquivo › Abrir pasta). */
  function importFolder(files: FileList | File[] | null) {
    const [first, ...rest] = byName(files ?? []);
    if (!first) return;
    const job = { file: first, appended: rest, config: importConfig(DEFAULT_IMPORT_OPTIONS, {}) };
    remember('file', first.name, [job]);
    return addDatasets([job]);
  }

  /** Appends `files` to dataset `id`, read with its configuration; files it
   *  already holds (by name) are skipped. Flags and edits follow their time
   *  stamps into the longer record, and each file gets a status. */
  async function appendFiles(id: DatasetId, files: FileList | File[] | null) {
    const entry = entries.find((candidate) => idOf(candidate) === id);
    if (!api || !entry || !files?.length) return;
    const held = new Set([entry.file, ...entry.appended].map((file) => file.name));
    const added = byName(files).filter((file) => !held.has(file.name));
    if (!added.length) {
      setErrors(['Todos os arquivos escolhidos já fazem parte deste dataset.']);
      return;
    }
    const appended = [...entry.appended, ...added];
    setBusy(`Anexando ${added.length} arquivo(s) a ${entry.file.name}…`);
    try {
      const { bytes, sources } = await sourceBuffer(entry.file, appended);
      const result = await api.importCsv(bytes, { name: entry.file.name, config: entry.config, replace: id, appended: sources });
      updateEntry(id, () => ({ result, appended, canUndo: false, canRedo: false }));
      // Only the files just added: the earlier ones were reported when they joined.
      setAppendReport({ dataset: entry.file.name, statuses: result.appended.slice(entry.appended.length) });
      setErrors([]);
    } catch (thrown) {
      setErrors([`${entry.file.name}: ${errorMessage(thrown)}`]);
    }
    setBusy('');
  }

  /** Refreshes the recent list from the browser's store. */
  const refreshRecent = () =>
    listRecent()
      .then(setRecent)
      .catch(() => undefined);

  /** Remembers `jobs` as a recent entry; a store that refuses only loses the entry. */
  const remember = (kind: RecentKind, name: string, jobs: ImportJob[]) =>
    addRecent(kind, name, jobs)
      .then(refreshRecent)
      .catch((thrown) => console.warn(`${STORE_UNAVAILABLE}: ${errorMessage(thrown)}`));

  // The last visit's session comes back once the core is up (a reload, F5, keeps the work).
  useEffect(() => {
    if (!api) return;
    (async () => {
      try {
        const session = await loadSession();
        if (session?.jobs.length) {
          setBusy(RESTORING);
          await addDatasets(session.jobs);
          setWorkbookName(session.name);
        }
      } catch (thrown) {
        console.warn(`${STORE_UNAVAILABLE}: ${errorMessage(thrown)}`);
      }
      setRestored(true);
      refreshRecent();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);

  // The session is saved a moment after every change: its files once, its state each time.
  useEffect(() => {
    if (!api || !restored) return;
    const timer = setTimeout(async () => {
      try {
        const jobs: ImportJob[] = [];
        for (const { file, appended, config, result, rules, display } of entries) {
          jobs.push({ file, appended, config, restore: await api.projectState({ dataset: result.dataset.id }), rules, display });
        }
        await saveSession(workbookName, jobs);
      } catch (thrown) {
        console.warn(`${STORE_UNAVAILABLE}: ${errorMessage(thrown)}`);
      }
    }, SESSION_SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [api, restored, entries, workbookName]);

  /** Adds a saved workbook's datasets, with their flags and rules. */
  async function openWorkbook(file: File | undefined) {
    if (!file) return;
    try {
      const datasets = await readProject(file);
      await addDatasets(datasets);
      setWorkbookName(file.name);
      remember('workbook', file.name, datasets);
    } catch (thrown) {
      setErrors([`${file.name}: ${errorMessage(thrown)}`]);
    }
  }

  /** Opens a recent entry: a workbook's datasets join the open ones, as Abrir workbook does. */
  async function openRecent(id: string) {
    try {
      const stored = await loadRecent(id);
      if (!stored) {
        setErrors([RECENT_GONE]);
        refreshRecent();
        return;
      }
      await addDatasets(stored.jobs);
      if (stored.kind === 'workbook' && !entries.length) setWorkbookName(stored.name);
      remember(stored.kind, stored.name, stored.jobs);
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
  }

  /** Saves every dataset with the configuration last applied to it, its flags
   *  with their rule evidence, and its flag rules. */
  async function saveWorkbook() {
    if (!api || !entries.length) return;
    for (const file of entries.flatMap((entry) => [entry.file, ...entry.appended])) {
      try {
        await file.slice(0, 1).arrayBuffer();
      } catch {
        setErrors([`${file.name}: ${SOURCE_UNREADABLE}`]);
        return;
      }
    }
    const datasets: SavedDataset[] = [];
    try {
      for (const { file, appended, config, result, rules, display } of entries) {
        datasets.push({ file, appended, config, restore: await api.projectState({ dataset: result.dataset.id }), rules, display });
      }
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
      return;
    }
    const name = workbookName === UNTITLED_WORKBOOK ? `workbook${PROJECT_EXTENSION}` : workbookName;
    download(name, writeProject(datasets));
    setWorkbookName(name);
    remember('workbook', name, datasets);
  }

  /** Re-reads a dataset's file with `options` and its pending corrections;
   *  the new result, or nothing when it failed. */
  async function reapply(entry: Entry, options: ImportOptions, configPatch: Partial<ImportConfigDto> = {}): Promise<ImportResultDto | undefined> {
    if (!api) return;
    const id = idOf(entry);
    setBusy(`Reaplicando ${entry.file.name}…`);
    let reread: ImportResultDto | undefined;
    try {
      const sentCorrections = entry.corrections;
      const { bytes, sources } = await sourceBuffer(entry.file, entry.appended);
      // Fields the dialog does not edit (rows to skip, time columns, ...) keep their saved values.
      const config = { ...entry.config, ...importConfig(options, sentCorrections), ...configPatch };
      const result = await api.importCsv(bytes, { name: entry.file.name, config, replace: id, appended: sources });
      const columnIds = new Set(result.dataset.columns.map((column) => column.id));
      updateEntry(id, (latest) => ({
        result,
        config,
        dirty: latest.corrections !== sentCorrections,
        plotted: latest.plotted.filter((columnId) => columnIds.has(columnId)),
        // A re-import starts a new history in the core.
        canUndo: false,
        canRedo: false,
      }));
      setErrors([]);
      reread = result;
    } catch (thrown) {
      setErrors([`${entry.file.name}: ${errorMessage(thrown)}`]);
    }
    setBusy('');
    return reread;
  }

  async function remove(id: DatasetId) {
    if (!api || busy) return;
    const position = entries.findIndex((entry) => idOf(entry) === id);
    const neighbour = entries[position + 1] ?? entries[position - 1];
    setBusy('Removendo…');
    try {
      await api.remove({ dataset: id });
      setEntries((current) => current.filter((entry) => idOf(entry) !== id));
      setSelectedId((current) => (current === id ? (neighbour ? idOf(neighbour) : null) : current));
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
    setBusy('');
  }

  /** Starts an empty workbook: every dataset leaves the session. */
  async function newWorkbook() {
    for (const entry of entries) await remove(idOf(entry));
    setEntries([]);
    setSelectedId(null);
    setWorkbookName(UNTITLED_WORKBOOK);
  }

  /** Moves a dataset one place up (-1) or down (+1); the order holds in every list. */
  function move(id: DatasetId, step: -1 | 1) {
    setEntries((current) => {
      const from = current.findIndex((entry) => idOf(entry) === id);
      const to = from + step;
      if (from < 0 || to < 0 || to >= current.length) return current;
      const reordered = [...current];
      [reordered[from], reordered[to]] = [reordered[to], reordered[from]];
      return reordered;
    });
  }

  /** Records a correction for dataset column `position` (keyed by its file column). */
  function setCorrection(entry: Entry, position: number, patch: Partial<ColumnCorrectionDto>) {
    updateEntry(idOf(entry), (latest) => {
      const fileColumn = latest.result.report.fileColumnByColumn[position];
      const correction = { ...latest.corrections[fileColumn], ...patch, fileColumn };
      return { corrections: { ...latest.corrections, [fileColumn]: correction }, dirty: true };
    });
  }

  function togglePlot(entry: Entry, columnId: string) {
    updateEntry(idOf(entry), (latest) => {
      const order = latest.result.dataset.columns.map((column) => column.id);
      const plotted = latest.plotted.includes(columnId)
        ? latest.plotted.filter((id) => id !== columnId)
        : order.filter((id) => id === columnId || latest.plotted.includes(id));
      return { plotted };
    });
  }

  /** Shows the flags and summaries an edit, undo or redo left. */
  async function showEdit(entry: Entry, edited: Promise<EditStateDto>) {
    try {
      const state = await edited;
      updateEntry(idOf(entry), (latest) => {
        const dataset = { ...latest.result.dataset, startMs: state.startMs, flags: state.flags, segments: state.segments };
        // New values: a new column list, so the charts of the values draw again.
        if (state.valuesChanged) dataset.columns = [...dataset.columns];
        return { result: { ...latest.result, dataset, summaries: state.summaries }, canUndo: state.canUndo, canRedo: state.canRedo };
      });
      setErrors([]);
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
  }

  /** Applies `flag` to `columns` between two instants, or removes it (or every
   *  flag, with `flag` undefined) when `apply` is false. */
  async function editFlags(entry: Entry, columns: string[], window: [number, number], apply: boolean, flag: number | undefined) {
    if (!api) return;
    const derived = new Set(entry.result.dataset.columns.filter(isDerived).map((column) => column.id));
    const measured = columns.filter((id) => !derived.has(id));
    if (!measured.length) {
      setErrors([NO_MEASURED_COLUMNS]);
      return;
    }
    const [fromMs, toMs] = window;
    const stretch = { dataset: idOf(entry), columns: measured, fromMs, toMs };
    await showEdit(entry, apply ? api.flag({ ...stretch, flag: flag ?? 0 }) : api.unflag({ ...stretch, flag }));
  }

  /** Applies one op (a value edit from the Revisar menu, ...) to the dataset. */
  async function applyOp(entry: Entry, op: OpDto) {
    if (!api) return;
    await showEdit(entry, api.applyOp({ dataset: idOf(entry), op }));
  }

  /** Undoes or redoes the dataset's last edit. */
  async function stepHistory(entry: Entry, direction: 'undo' | 'redo') {
    if (!api) return;
    const request = { dataset: idOf(entry) };
    await showEdit(entry, direction === 'undo' ? api.undo(request) : api.redo(request));
  }

  async function exportCsv(entry: Entry, delimiter: ',' | ';', includeDerived: boolean) {
    try {
      const text = await api!.exportCsv({ dataset: idOf(entry), delimiter, decimalComma: delimiter === ';', includeDerived });
      download(`${baseName(entry.file)}_export.csv`, textFile(text, 'text/csv'));
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
  }

  /** Saves the recipe of one analysis result (ARQUITETURA C3): the source
   *  file by SHA-256, its configuration and state, the request, the core's
   *  answer and version, for `wind replay` to reproduce. */
  async function exportRecipe(entry: Entry, analysis: string, request: object) {
    try {
      // ponytail: a recipe names one source file; appended files would need a list there.
      if (entry.appended.length) throw new Error(RECIPE_WITH_APPENDED);
      const bytes = new Uint8Array(await entry.file.arrayBuffer());
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
      const sha256 = Array.from(digest, (byte) => byte.toString(HEX_RADIX).padStart(HEX_DIGITS_PER_BYTE, '0')).join('');
      const dataset = idOf(entry);
      const [restore, result] = await Promise.all([api!.projectState({ dataset }), api!.analyze({ dataset, analysis, request })]);
      const recipe: RecipeDto = {
        format: RECIPE_FORMAT,
        coreVersion: CORE_VERSION,
        source: { name: entry.file.name, bytes: bytes.length, sha256 },
        config: entry.config,
        restore,
        analysis,
        request,
        result,
      };
      download(`${baseName(entry.file)}_${analysis}_receita.json`, textFile(JSON.stringify(recipe, null, JSON_INDENT), 'application/json'));
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
  }

  async function exportJson(entry: Entry) {
    try {
      const text = await api!.exportJson({ dataset: idOf(entry) });
      download(`${baseName(entry.file)}_metadados.json`, textFile(text, 'application/json'));
    } catch (thrown) {
      setErrors([errorMessage(thrown)]);
    }
  }

  return {
    api,
    entries,
    selected,
    selectedId,
    setSelectedId,
    busy,
    errors,
    setErrors,
    workbookName,
    updateEntry,
    importFiles,
    importFolder,
    appendFiles,
    appendReport,
    setAppendReport,
    openWorkbook,
    openRecent,
    recent,
    saveWorkbook,
    newWorkbook,
    reapply,
    remove,
    move,
    setCorrection,
    togglePlot,
    showEdit,
    editFlags,
    applyOp,
    stepHistory,
    exportCsv,
    exportJson,
    exportRecipe,
  };
}

/** Everything the main window gets from {@link useWorkbook}. */
export type Workbook = ReturnType<typeof useWorkbook>;

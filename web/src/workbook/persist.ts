/**
 * The workbook kept in the browser (IndexedDB), so a reload (F5) finds it
 * again, and the workbooks and files opened recently (Arquivo › Abertos
 * recentemente). Each source file is stored once, by name, size and date;
 * the session and every recent entry keep what to apply to them (the
 * configuration, the flags with their evidence and edits, the rules and the
 * display), as a project file does (`../project`). The browser may refuse
 * storage (a private window, a full disk): every function then rejects, and
 * the page goes on without it.
 */
import type { ColumnDisplayDto } from '../api/generated/ColumnDisplayDto';
import type { FlagRuleDto } from '../api/generated/FlagRuleDto';
import type { ImportConfigDto } from '../api/generated/ImportConfigDto';
import type { RestoreDto } from '../api/generated/RestoreDto';
import type { ImportJob } from './model';

const DATABASE = 'openwindaxis';
const DATABASE_VERSION = 1;
/** Source files, by {@link sourceKey}. */
const SOURCES = 'sources';
/** The session and the recent entries, by id. */
const WORKBOOKS = 'workbooks';
/** Id of the workbook open when the page was left. */
const SESSION_ID = 'session';
/** Recent entries kept, newest first. */
export const MAX_RECENT = 8;

/** What a recent entry reopens. */
export type RecentKind = 'workbook' | 'file';

/** One dataset as stored: its files by key and what to apply to them. */
interface StoredDataset {
  source: string;
  appended: string[];
  config: ImportConfigDto;
  restore?: RestoreDto;
  rules?: FlagRuleDto[];
  display?: ColumnDisplayDto;
}

interface StoredWorkbook {
  id: string;
  kind: RecentKind;
  name: string;
  savedAt: number;
  datasets: StoredDataset[];
}

/** A recent entry as the menu lists it. */
export interface RecentEntry {
  id: string;
  kind: RecentKind;
  name: string;
  /** When it was opened or saved, epoch ms. */
  savedAt: number;
}

/** A stored workbook made ready to import again. */
export interface StoredJobs {
  name: string;
  kind: RecentKind;
  jobs: ImportJob[];
}

/** A file's key in the store: the same file picked again finds its copy. */
const sourceKey = (file: File) => `${file.name}|${file.size}|${file.lastModified}`;

let opened: Promise<IDBDatabase> | null = null;

function database(): Promise<IDBDatabase> {
  opened ??= new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, DATABASE_VERSION);
    request.onupgradeneeded = () => {
      request.result.createObjectStore(SOURCES);
      request.result.createObjectStore(WORKBOOKS, { keyPath: 'id' });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => {
      opened = null;
      reject(request.error);
    };
  });
  return opened;
}

/** Runs `work` in one transaction over `stores` and resolves when it commits. */
async function transaction<T>(stores: string[], mode: IDBTransactionMode, work: (transaction: IDBTransaction) => Promise<T> | T): Promise<T> {
  const db = await database();
  const tx = db.transaction(stores, mode);
  const done = new Promise<void>((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
  const result = await work(tx);
  await done;
  return result;
}

function request<T>(pending: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    pending.onsuccess = () => resolve(pending.result);
    pending.onerror = () => reject(pending.error);
  });
}

/** Stores the files of `jobs` not stored yet, and the workbook record. */
async function put(record: Omit<StoredWorkbook, 'datasets'>, jobs: ImportJob[]) {
  const files = jobs.flatMap((job) => [job.file, ...(job.appended ?? [])]);
  const datasets: StoredDataset[] = jobs.map((job) => ({
    source: sourceKey(job.file),
    appended: (job.appended ?? []).map(sourceKey),
    config: job.config,
    restore: job.restore,
    rules: job.rules,
    display: job.display,
  }));
  await transaction([SOURCES, WORKBOOKS], 'readwrite', async (tx) => {
    const sources = tx.objectStore(SOURCES);
    for (const file of files) {
      const key = sourceKey(file);
      if ((await request(sources.count(key))) === 0) sources.put(file, key);
    }
    tx.objectStore(WORKBOOKS).put({ ...record, datasets });
  });
}

/** Removes the oldest recent entries past {@link MAX_RECENT} and the files nothing refers to. */
async function tidy() {
  await transaction([SOURCES, WORKBOOKS], 'readwrite', async (tx) => {
    const workbooks = tx.objectStore(WORKBOOKS);
    const all = (await request(workbooks.getAll())) as StoredWorkbook[];
    const recent = all.filter((workbook) => workbook.id !== SESSION_ID).sort((first, second) => second.savedAt - first.savedAt);
    const dropped = new Set(recent.slice(MAX_RECENT).map((workbook) => workbook.id));
    dropped.forEach((id) => workbooks.delete(id));
    const used = new Set(all.filter((workbook) => !dropped.has(workbook.id)).flatMap((workbook) => workbook.datasets.flatMap((dataset) => [dataset.source, ...dataset.appended])));
    const sources = tx.objectStore(SOURCES);
    for (const key of (await request(sources.getAllKeys())) as string[]) {
      if (!used.has(key)) sources.delete(key);
    }
  });
}

/** Keeps `jobs` as the session the next visit restores. */
export async function saveSession(name: string, jobs: ImportJob[]) {
  await put({ id: SESSION_ID, kind: 'workbook', name, savedAt: Date.now() }, jobs);
  await tidy();
}

/** Adds (or brings to the top) a recent entry. */
export async function addRecent(kind: RecentKind, name: string, jobs: ImportJob[]) {
  await put({ id: `${kind}:${name}`, kind, name, savedAt: Date.now() }, jobs);
  await tidy();
}

/** The recent entries, newest first. */
export async function listRecent(): Promise<RecentEntry[]> {
  const all = (await transaction([WORKBOOKS], 'readonly', (tx) => request(tx.objectStore(WORKBOOKS).getAll()))) as StoredWorkbook[];
  return all
    .filter((workbook) => workbook.id !== SESSION_ID)
    .sort((first, second) => second.savedAt - first.savedAt)
    .map(({ id, kind, name, savedAt }) => ({ id, kind, name, savedAt }));
}

/** Workbook `id` with its files, or `null` when it is not stored. */
async function load(id: string): Promise<StoredJobs | null> {
  return transaction([SOURCES, WORKBOOKS], 'readonly', async (tx) => {
    const workbook = (await request(tx.objectStore(WORKBOOKS).get(id))) as StoredWorkbook | undefined;
    if (!workbook) return null;
    const sources = tx.objectStore(SOURCES);
    const file = async (key: string) => {
      const stored = (await request(sources.get(key))) as File | undefined;
      if (!stored) throw new Error(`o arquivo ${key.split('|')[0]} não está mais guardado no navegador`);
      return stored;
    };
    const jobs: ImportJob[] = [];
    for (const dataset of workbook.datasets) {
      jobs.push({
        file: await file(dataset.source),
        appended: await Promise.all(dataset.appended.map(file)),
        config: dataset.config,
        restore: dataset.restore,
        rules: dataset.rules,
        display: dataset.display,
      });
    }
    return { name: workbook.name, kind: workbook.kind, jobs };
  });
}

/** The session left on the last visit, or `null`. */
export const loadSession = () => load(SESSION_ID);

/** Recent entry `id`, or `null` when it is gone. */
export const loadRecent = (id: string) => load(id);

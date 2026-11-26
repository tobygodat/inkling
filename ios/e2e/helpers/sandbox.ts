import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { join, resolve } from 'node:path';
import { expect } from 'e2e';
import { DELETE_TILE_Y, SHELF_TILES, type Point } from './points.ts';

export const APP_BUNDLE_ID = 'com.tobygodat.inkling';

export function simulatorUdid(): string {
  const udid = process.env.INKLING_E2E_SIMULATOR;
  if (!udid) throw new Error('INKLING_E2E_SIMULATOR is not set.');
  return udid;
}

export function appContainer(): string {
  return execFileSync(
    'xcrun',
    ['simctl', 'get_app_container', simulatorUdid(), APP_BUNDLE_ID, 'data'],
    { encoding: 'utf8' },
  ).trim();
}

export function dataDir(): string {
  return join(appContainer(), 'Library', 'Application Support', 'Inkling');
}

export function appDocumentsDir(): string {
  return join(appContainer(), 'Documents');
}

/** Clears app data files so each test file starts from a clean state. */
export function resetSandbox(): void {
  const dir = dataDir();
  for (const name of ['documents', 'draft.md', 'settings.json', 'sent-feedback.json', 'library', 'sync.json', 'fake-server.json']) {
    rmSync(join(dir, name), { recursive: true, force: true });
  }
  rmSync(appDocumentsDir(), { recursive: true, force: true });
}

export function settingsPath(): string {
  return join(dataDir(), 'settings.json');
}

export function readSettings<T = Record<string, unknown>>(): T | null {
  const file = settingsPath();
  if (!existsSync(file)) return null;
  return JSON.parse(readFileSync(file, 'utf8')) as T;
}

export function writeSettings(settings: Record<string, unknown>): void {
  const dir = dataDir();
  mkdirSync(dir, { recursive: true });
  writeFileSync(settingsPath(), JSON.stringify(settings));
}

export function openDocumentPath(): string | null {
  const docs = join(dataDir(), 'documents');
  if (!existsSync(docs)) return null;
  const settings = readSettings<{ document?: string }>();
  if (settings?.document) {
    const named = join(docs, `${settings.document}.md`);
    if (existsSync(named)) return named;
  }
  const files = readdirSync(docs)
    .filter((name) => name.endsWith('.md'))
    .map((name) => ({ path: join(docs, name), mtimeMs: statSync(join(docs, name)).mtimeMs }))
    .sort((a, b) => b.mtimeMs - a.mtimeMs);
  return files[0]?.path ?? null;
}

export function readDraft(): string {
  const file = openDocumentPath();
  return file && existsSync(file) ? readFileSync(file, 'utf8') : '';
}

export function documentCount(): number {
  const docs = join(dataDir(), 'documents');
  if (!existsSync(docs)) return 0;
  return readdirSync(docs).filter((name) => name.endsWith('.md')).length;
}

export function readAllDocuments(): string[] {
  const docs = join(dataDir(), 'documents');
  if (!existsSync(docs)) return [];
  return readdirSync(docs)
    .filter((name) => name.endsWith('.md'))
    .map((name) => readFileSync(join(docs, name), 'utf8'));
}

/** Seeds draft.md (clearing documents/) so the next launch migrates it as the sole open document. */
export function seedDraft(text: string): void {
  const dir = dataDir();
  mkdirSync(dir, { recursive: true });
  rmSync(join(dir, 'documents'), { recursive: true, force: true });
  writeFileSync(join(dir, 'draft.md'), text);
}

export async function expectDraft(expected: string, timeout = 6_000): Promise<void> {
  await expect.poll(() => readDraft(), { timeout }).toBe(expected);
}

export async function expectDraftLetters(expectedLower: string, timeout = 6_000): Promise<void> {
  await expect.poll(() => readDraft().toLowerCase(), { timeout }).toBe(expectedLower);
}

export function sentFeedbackPath(): string {
  return join(dataDir(), 'sent-feedback.json');
}

export function dataFilesContain(needle: string): boolean {
  const walk = (dir: string): boolean => {
    if (!existsSync(dir)) return false;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (walk(full)) return true;
      } else if (readFileSync(full, 'utf8').includes(needle)) {
        return true;
      }
    }
    return false;
  };
  return walk(dataDir());
}

interface LibraryBook {
  readonly id: string;
  readonly title: string;
  readonly opened: number;
  readonly position: { readonly chapter: number; readonly fraction: number };
}

interface LibraryFile {
  readonly books: readonly LibraryBook[];
}

export function libraryJsonPath(): string {
  return join(dataDir(), 'library', 'library.json');
}

export function readLibrary(): readonly LibraryBook[] {
  const file = libraryJsonPath();
  if (!existsSync(file)) return [];
  return (JSON.parse(readFileSync(file, 'utf8')) as LibraryFile).books;
}

export function bookCount(): number {
  return readLibrary().length;
}

export function bookByPrefix(prefix: string): LibraryBook | undefined {
  return readLibrary().find((book) => book.title.startsWith(prefix));
}

export function bookTilePoint(prefix: string): Point {
  const index = readLibrary().findIndex((book) => book.title.startsWith(prefix));
  if (index < 0 || index >= SHELF_TILES.length) {
    throw new Error(`No shelf tile for book starting with "${prefix}" (index ${index})`);
  }
  return SHELF_TILES[index]!;
}

export function deleteBookPoint(prefix: string): Point {
  return { x: bookTilePoint(prefix).x, y: DELETE_TILE_Y };
}

export function readHighlights(): string {
  const dir = join(dataDir(), 'library', 'highlights');
  if (!existsSync(dir)) return '';
  return readdirSync(dir)
    .filter((name) => name.endsWith('.json'))
    .map((name) => readFileSync(join(dir, name), 'utf8'))
    .join('\n');
}

export function storedBookFiles(bookId: string): number {
  const root = join(dataDir(), 'library');
  if (!existsSync(root)) return 0;
  let count = 0;
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === '.removed') continue;
      if (entry.name.startsWith(bookId)) count += 1;
      if (entry.isDirectory()) walk(join(dir, entry.name));
    }
  };
  walk(root);
  return count;
}

/** Copies book fixtures downloaded by books/fetch-fixtures.sh into the app's Documents folder. */
export function copyBookFixtures(names: readonly string[]): void {
  const fixturesDir = process.env.INKLING_FIXTURES
    ? resolve(process.env.INKLING_FIXTURES)
    : resolve(import.meta.dirname, '..', '..', '..', 'target', 'fixtures');
  const docs = appDocumentsDir();
  mkdirSync(docs, { recursive: true });
  for (const name of names) {
    copyFileSync(join(fixturesDir, name), join(docs, name));
  }
}

export interface FakeServerState {
  documents: Record<
    string,
    { text: string; saved_at: number; device: string; version: number; deleted: boolean }
  >;
  down?: boolean;
  [key: string]: unknown;
}

export function fakeServerPath(): string {
  return join(dataDir(), 'fake-server.json');
}

export function editFakeServer(mutate: (state: FakeServerState) => void): void {
  const file = fakeServerPath();
  mkdirSync(dataDir(), { recursive: true });
  const state: FakeServerState = existsSync(file)
    ? (JSON.parse(readFileSync(file, 'utf8')) as FakeServerState)
    : { documents: {} };
  state.documents ??= {};
  mutate(state);
  writeFileSync(file, JSON.stringify(state));
}

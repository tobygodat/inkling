import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import {
  filesDiffer,
  longPress,
  openDraft,
  pause,
  stepShot,
  tap,
  tapKeys,
} from '../helpers/ios.ts';
import {
  bookByPrefix,
  bookCount,
  bookTilePoint,
  copyBookFixtures,
  dataDir,
  deleteBookPoint,
  expectDraft,
  readHighlights,
  resetSandbox,
  storedBookFiles,
  writeSettings,
} from '../helpers/sandbox.ts';

const READING_DRAFT = 'The draft from before the books.';
const WALDEN_PARAGRAPH =
  'I went to the woods because I wished to live deliberately, to front only the essential facts of life, and see if I could not learn what it had to teach.';

test.describe('library, reading, highlights, quotes, find, and delete', { serial: true }, () => {
  test('find books, EPUB paged reading, contents, write/read toggle, and PDF', async ({ app, device, screen }) => {
    resetSandbox();
    copyBookFixtures(['pride-and-prejudice.epub', 'rfc2616.pdf']);
    writeSettings({ provider: 'Claude', reading: { pages: 'paged' } });
    await openDraft(app, READING_DRAFT, 2_000);

    // books-find
    await tap(screen, device, 'show-reading', 1_500);
    expect(bookCount()).toBe(0);
    await tap(screen, device, 'find-books', 4_000);
    await stepShot(app, 'books-find');
    // The PDF starts unticked; tick it before adding.
    await tap(screen, device, 'found-pdf', 800);

    // books-library
    await tap(screen, device, 'add-found', 2_500);
    await expect.poll(() => bookCount(), { timeout: 12_000 }).toBe(2);
    await stepShot(app, 'books-library');

    // books-epub
    await tap(screen, device, bookTilePoint('Pride'), 3_000);
    await expect.poll(() => bookByPrefix('Pride')?.opened ?? 0, { timeout: 5_000 }).not.toBe(0);
    await stepShot(app, 'books-epub');

    // books-page-turned
    await tap(screen, device, 'page-forward', 1_500);
    await expect
      .poll(() => {
        const pos = bookByPrefix('Pride')?.position;
        return pos ? pos.chapter === 0 && pos.fraction > 0 : false;
      }, { timeout: 5_000 })
      .toBe(true);
    await stepShot(app, 'books-page-turned');

    // books-contents
    await tap(screen, device, 'contents', 800);
    await stepShot(app, 'books-contents');

    // books-chapter
    await tap(screen, device, 'chapter-one', 1_500);
    await expect
      .poll(() => bookByPrefix('Pride')?.position, { timeout: 5_000 })
      .toEqual({ chapter: 1, fraction: 0 });
    await stepShot(app, 'books-chapter');

    // books-write
    await tap(screen, device, 'show-writing', 1_500);
    await expectDraft(READING_DRAFT);
    await stepShot(app, 'books-write');

    // books-read
    await tap(screen, device, 'show-reading', 2_000);
    expect(bookByPrefix('Pride')?.position).toEqual({ chapter: 1, fraction: 0 });
    await stepShot(app, 'books-read');

    // books-pdf
    await tap(screen, device, 'back-to-library', 1_500);
    await tap(screen, device, bookTilePoint('Microsoft'), 3_000);
    await expect
      .poll(() => (bookByPrefix('Microsoft')?.opened ?? 0) > (bookByPrefix('Pride')?.opened ?? 0), {
        timeout: 5_000,
      })
      .toBe(true);
    await stepShot(app, 'books-pdf');
  });

  test('Walden footnotes, highlights, and quotes', async ({
    app,
    device,
    screen,
  }) => {
    // books-walden
    await tap(screen, device, 'back-to-library', 1_500);
    copyBookFixtures(['walden.epub']);
    await tap(screen, device, 'find-books', 4_000);
    await tap(screen, device, 'add-one-found', 2_500);
    await expect.poll(() => bookCount(), { timeout: 12_000 }).toBe(3);
    await tap(screen, device, bookTilePoint('Walden'), 3_000);
    await expect.poll(() => bookByPrefix('Walden')?.opened ?? 0, { timeout: 5_000 }).not.toBe(0);
    await stepShot(app, 'books-walden');

    // books-footnote
    const beforeNote = await app.screenshot('books-footnote-before');
    await tap(screen, device, 'footnote-marker', 1_500);
    const noteShot = await stepShot(app, 'books-footnote');
    expect(filesDiffer(beforeNote, noteShot), 'tapping the footnote marker changed nothing').toBe(true);
    expect(bookByPrefix('Walden')?.position).toEqual({ chapter: 0, fraction: 0 });
    await tap(screen, device, 'reading-middle', 1_200);
    expect(bookByPrefix('Walden')?.position).toEqual({ chapter: 0, fraction: 0 });

    // books-hold
    await longPress(device, 'walden-paragraph', 900);
    await pause(1_200);
    await stepShot(app, 'books-hold');

    // books-highlight
    await tap(screen, device, 'pill-highlight', 800);
    await expect.poll(() => readHighlights(), { timeout: 5_000 }).toContain(`"words": "${WALDEN_PARAGRAPH}`);
    await tap(screen, device, 'reading-middle', 1_200);
    expect(bookByPrefix('Walden')?.position).toEqual({ chapter: 0, fraction: 0 });
    await stepShot(app, 'books-highlight');
    await tap(screen, device, 'page-forward', 1_500);
    await expect
      .poll(() => {
        const pos = bookByPrefix('Walden')?.position;
        return pos ? pos.chapter === 0 && pos.fraction > 0 : false;
      }, { timeout: 5_000 })
      .toBe(true);
    await tap(screen, device, 'page-back', 1_500);

    // books-remove
    await longPress(device, 'walden-paragraph', 900);
    await pause(1_200);
    await tap(screen, device, 'pill-highlight', 800);
    await expect.poll(() => readHighlights().includes(WALDEN_PARAGRAPH), { timeout: 5_000 }).toBe(false);
    await tap(screen, device, 'reading-middle', 1_200);
    await stepShot(app, 'books-remove');
    // Re-add highlight for the Highlights tab check
    await longPress(device, 'walden-paragraph', 900);
    await pause(1_200);
    await tap(screen, device, 'pill-highlight', 800);
    await tap(screen, device, 'reading-middle', 1_200);

    // books-highlights-tab
    await tap(screen, device, 'page-forward', 1_500);
    await tap(screen, device, 'page-forward', 1_500);
    await expect.poll(() => bookByPrefix('Walden')?.position.chapter, { timeout: 5_000 }).toBe(1);
    await tap(screen, device, 'contents', 800);
    await tap(screen, device, 'contents-highlights', 1_200);
    await stepShot(app, 'books-highlights-tab');
    await tap(screen, device, 'first-highlight', 1_500);
    await expect.poll(() => bookByPrefix('Walden')?.position.chapter, { timeout: 5_000 }).toBe(0);

    // books-quote
    await longPress(device, 'walden-paragraph', 900);
    await pause(1_200);
    await tap(screen, device, 'pill-quote', 1_500);
    const quoted = `${READING_DRAFT}\n\n> ${WALDEN_PARAGRAPH.replace('deliberately', '*deliberately*')}\n>\n> — Henry David Thoreau, *Walden*\n\n`;
    await expectDraft(quoted);
    expect(bookByPrefix('Walden')?.position.chapter).toBe(0);
    await stepShot(app, 'books-quote');
    await tap(screen, device, 'show-writing', 1_500);
    await stepShot(app, 'books-quote-page');
  });

  test('find in book, find in library, delete book, and undo', async ({ app, device, screen }) => {
    // find-book
    await tap(screen, device, 'show-reading', 2_000);
    await tap(screen, device, 'open-find-tall', 800);
    await tapKeys(screen, 'woods');
    await pause(800);
    await stepShot(app, 'find-book');

    // find-book-next
    await tapKeys(screen, '/');
    await pause(1_200);
    await stepShot(app, 'find-book-next');
    await tap(screen, device, 'find-done-tall', 1_500);
    await expect.poll(() => bookByPrefix('Walden')?.position.chapter, { timeout: 5_000 }).toBe(1);

    // find-library
    await tap(screen, device, 'back-to-library', 1_500);
    await tap(screen, device, 'open-find-tall', 800);
    await tapKeys(screen, 'pride');
    await pause(800);
    await stepShot(app, 'find-library');
    const prideBefore = bookByPrefix('Pride')?.opened ?? 0;
    // Filtered, Pride and Prejudice is the first tile even though Walden is newer.
    await tap(screen, device, { x: 90, y: 296 }, 3_000);
    await expect.poll(() => bookByPrefix('Pride')?.opened ?? 0, { timeout: 5_000 }).toBeGreaterThan(prideBefore);

    // delete-book
    await tap(screen, device, 'back-to-library', 1_500);
    const waldenId = bookByPrefix('Walden')?.id;
    expect(waldenId).toBeDefined();
    const fileCountBefore = storedBookFiles(waldenId!);
    const markedPath = join(dataDir(), 'library', 'highlights', `${waldenId}.json`);
    expect(fileCountBefore).toBeGreaterThanOrEqual(4);
    expect(readFileSync(markedPath, 'utf8')).toContain('"words"');
    const beforeMarks = readFileSync(markedPath, 'utf8');
    await longPress(device, bookTilePoint('Walden'), 800);
    await pause(1_200);
    await stepShot(app, 'delete-book');

    // book-deleted
    await tap(screen, device, deleteBookPoint('Walden'), 1_500);
    await expect.poll(() => bookCount(), { timeout: 5_000 }).toBe(2);
    expect(storedBookFiles(waldenId!)).toBe(0);
    expect(existsSync(markedPath)).toBe(false);
    await stepShot(app, 'book-deleted');

    // undo-book
    await tap(screen, device, 'undo-remove-book', 1_500);
    await expect.poll(() => bookCount(), { timeout: 5_000 }).toBe(3);
    expect(storedBookFiles(waldenId!)).toBe(fileCountBefore);
    expect(readFileSync(markedPath, 'utf8')).toBe(beforeMarks);
    expect(bookByPrefix('Walden')?.position.chapter).toBe(1);
    await stepShot(app, 'undo-book');
  });
});

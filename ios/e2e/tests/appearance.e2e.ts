import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openDraft, pan, pause, stepShot, tap } from '../helpers/ios.ts';
import {
  bookByPrefix,
  bookCount,
  bookTilePoint,
  copyBookFixtures,
  readSettings,
  resetSandbox,
  writeSettings,
} from '../helpers/sandbox.ts';

interface AppearanceSettings {
  readonly appearance?: { readonly mode?: string };
  readonly reading?: {
    readonly synced?: boolean;
    readonly pages?: string;
    readonly look?: { readonly mode?: string };
  };
}

test('appearance sheet, reading/writing tone sync, and paged/scroll position hold', async ({
  app,
  device,
  screen,
}) => {
  resetSandbox();
  copyBookFixtures(['pride-and-prejudice.epub', 'rfc2616.pdf']);
  writeSettings({ provider: 'Claude', reading: { pages: 'paged' } });
  await openDraft(app, 'The draft for appearance.', 2_000);

  // Import both books and open the PDF so the reading view is active.
  await tap(screen, device, 'show-reading', 1_500);
  await tap(screen, device, 'find-books', 4_000);
  await tap(screen, device, 'found-pdf', 800);
  await tap(screen, device, 'add-found', 2_500);
  await expect.poll(() => bookCount(), { timeout: 12_000 }).toBe(2);
  await tap(screen, device, bookTilePoint('Microsoft'), 3_000);

  // appearance-page
  await tap(screen, device, 'show-writing', 1_500);
  await tap(screen, device, 'cog', 1_200);
  await stepShot(app, 'appearance-page');
  await tap(screen, device, 'sheet-done', 800);

  // appearance-book
  await tap(screen, device, 'show-reading', 2_000);
  await tap(screen, device, 'cog', 1_200);
  await stepShot(app, 'appearance-book');

  // appearance-tone: sync off -> Sepia changes only reading
  await tap(screen, device, 'sync', 800);
  await tap(screen, device, 'tone-sepia', 1_200);
  await expect
    .poll(() => {
      const s = readSettings<AppearanceSettings>();
      return [s?.reading?.look?.mode, s?.reading?.synced, s?.appearance?.mode ?? 'light'];
    }, { timeout: 5_000 })
    .toEqual(['sepia', false, 'light']);
  await stepShot(app, 'appearance-tone');

  // appearance-sync: turning sync back on carries Sepia to writing
  await tap(screen, device, 'sync', 1_200);
  await expect
    .poll(() => readSettings<AppearanceSettings>()?.appearance?.mode, { timeout: 5_000 })
    .toBe('sepia');
  await stepShot(app, 'appearance-sync');

  // appearance-pages: Paged -> Scroll -> Paged preserves reading position
  await tap(screen, device, 'sheet-done', 800);
  await tap(screen, device, 'page-forward', 1_500);
  const held = bookByPrefix('Microsoft')?.position;
  expect(held).toBeDefined();

  await tap(screen, device, 'cog', 1_200);
  // Pan past the sheet's end along x = 392 (right of the sliders).
  await pan(screen, { x: 392, y: 800 }, 0, -700, 1_200);
  await pause(1_200);
  await tap(screen, device, 'pages-scroll', 1_200);
  await expect
    .poll(() => readSettings<AppearanceSettings>()?.reading?.pages, { timeout: 5_000 })
    .toBe('scroll');
  await tap(screen, device, 'pages-paged', 1_200);
  await tap(screen, device, 'sheet-done', 800);
  await tap(screen, device, 'back-to-library', 1_500);
  expect(bookByPrefix('Microsoft')?.position).toEqual(held);
  await stepShot(app, 'appearance-pages');
});

import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { filesDiffer, openDraft, pan, pause, stepShot, tap, tapKeys } from '../helpers/ios.ts';
import {
  dataDir,
  documentCount,
  expectDraft,
  readAllDocuments,
  resetSandbox,
} from '../helpers/sandbox.ts';

const FERRY = '# The ferry\n\nIt leaves at seven.';

test('documents migration, creation, switching, swipe delete, undo, and find', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, FERRY);

  // docs-list: draft.md migrated into documents/
  expect(existsSync(join(dataDir(), 'draft.md'))).toBe(false);
  await expectDraft(FERRY);
  await tap(screen, device, 'open-documents', 1_200);
  await stepShot(app, 'docs-list');

  // docs-new
  await tap(screen, device, 'new-document', 1_200);
  await expect(screen.getByLabel('Paste', { exact: true })).toBeVisible();
  await tapKeys(screen, 'the second piece');
  await expectDraft('The second piece');
  await expect.poll(() => documentCount(), { timeout: 5_000 }).toBe(2);
  await stepShot(app, 'docs-new');

  // docs-switch
  await tap(screen, device, 'header-done', 800);
  await tap(screen, device, 'open-documents', 1_200);
  await tap(screen, device, 'row-two', 1_200);
  await expectDraft(FERRY);
  await stepShot(app, 'docs-switch');

  // docs-delete
  await tap(screen, device, 'open-documents', 1_200);
  await pan(screen, { x: 300, y: 136 }, -220, 0, 400);
  await expect.poll(() => documentCount(), { timeout: 5_000 }).toBe(1);
  await stepShot(app, 'docs-delete');

  // docs-undo
  await tap(screen, device, 'undo-remove', 1_200);
  await expect.poll(() => documentCount(), { timeout: 5_000 }).toBe(2);
  expect(readAllDocuments().map((doc) => doc.trim())).toContain('The second piece');
  await stepShot(app, 'docs-undo');

  // find-page
  await tap(screen, device, 'row-two', 1_200);
  await expectDraft(FERRY);
  await tap(screen, device, 'open-find', 800);
  await tapKeys(screen, 't');
  await pause(800);
  const firstMatchShot = await stepShot(app, 'find-page');

  // find-page-next: Search key moves highlight to the next match
  await tapKeys(screen, '/');
  await pause(800);
  const nextMatchShot = await stepShot(app, 'find-page-next');
  expect(filesDiffer(firstMatchShot, nextMatchShot), 'Search key did not move to next match').toBe(true);
  await tap(screen, device, 'find-done', 800);
  await expect(screen.getByLabel('Paste', { exact: true })).toBeHidden();
  await expectDraft(FERRY);

  // find-list
  await tap(screen, device, 'open-documents', 1_200);
  await tap(screen, device, 'open-find', 800);
  await tapKeys(screen, 'ferry');
  await pause(800);
  await stepShot(app, 'find-list');
  await tap(screen, device, 'row-one', 1_200);
  await expectDraft(FERRY);
});

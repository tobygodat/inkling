import { existsSync, readFileSync } from 'node:fs';
import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openDraft, pan, pause, stepShot, tap } from '../helpers/ios.ts';
import {
  APP_BUNDLE_ID,
  editFakeServer,
  fakeServerPath,
  readAllDocuments,
  resetSandbox,
  writeSettings,
} from '../helpers/sandbox.ts';

const ACCOUNT_DRAFT = '# On keeping accounts\n\nEvery list I keep grows until it is the work itself.';

test('account sign-in, bidirectional fake-server sync, offline stop, and retry', async ({
  app,
  device,
  screen,
}) => {
  resetSandbox();
  editFakeServer((state) => {
    state.documents['1700000000000'] = {
      text: '# Letter to the board\n\nWritten on the Mac.',
      saved_at: 1_700_000_000_000,
      device: 'mac-a1b2',
      version: 1,
      deleted: false,
    };
  });
  writeSettings({ provider: 'Claude' });
  await openDraft(app, ACCOUNT_DRAFT, 2_000);

  // account-signed-out
  await tap(screen, device, 'cog', 1_200);
  await pan(screen, { x: 200, y: 700 }, 0, -600, 500);
  await pause(800);
  await pan(screen, { x: 200, y: 700 }, 0, -600, 500);
  await pause(800);
  await tap(screen, device, 'sheet-account', 1_200);
  await stepShot(app, 'account-signed-out');

  // account-signed-in
  await tap(screen, device, 'sign-in', 2_500);
  await expect
    .poll(
      () => (existsSync(fakeServerPath()) ? readFileSync(fakeServerPath(), 'utf8') : ''),
      { timeout: 10_000 },
    )
    .toContain('Every list I keep');
  await expect
    .poll(() => readAllDocuments().join('\n'), { timeout: 6_000 })
    .toContain('Written on the Mac.');
  await tap(screen, device, 'copy-address', 800);
  await expect.poll(() => device.clipboard(), { timeout: 4_000 }).toBe('https://inkling.invalid/mcp');
  await stepShot(app, 'account-signed-in');

  // account-sync-stopped: server down, background & foreground triggers sync failure
  editFakeServer((state) => {
    state.down = true;
  });
  await device.home();
  await pause(1_000);
  await device.openApp(APP_BUNDLE_ID);
  await pause(3_000);
  // While it is down, the Mac saves again; Try again must pull that version down.
  editFakeServer((state) => {
    const doc = state.documents['1700000000000'];
    if (doc) {
      doc.text = '# Letter to the board\n\nWritten again on the Mac.';
      doc.version += 1;
    }
  });
  await stepShot(app, 'account-sync-stopped');

  // account-try-again
  editFakeServer((state) => {
    state.down = false;
  });
  await tap(screen, device, 'try-again', 2_500);
  await expect
    .poll(() => readAllDocuments().join('\n'), { timeout: 8_000 })
    .toContain('Written again on the Mac.');
  await stepShot(app, 'account-try-again');
});

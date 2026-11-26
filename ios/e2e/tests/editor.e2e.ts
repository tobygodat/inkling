import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

test('pasting in the editor autosaves the draft', async ({ app, device, screen }) => {
  await app.open();
  const container = execFileSync('xcrun', [
    'simctl', 'get_app_container', process.env.INKLING_E2E_SIMULATOR!, 'com.tobygodat.inkling', 'data',
  ], { encoding: 'utf8' }).trim();
  const documents = join(container, 'Library', 'Application Support', 'Inkling', 'documents');
  const draft = 'the river held the light';
  // The GPUI editor is one native canvas, so focus it by position. The Paste control is UIKit.
  await screen.tapAt({ x: 200, y: 300 });
  const paste = screen.getByLabel('Paste', { exact: true });
  await expect(paste).toBeVisible();
  // On a fresh simulator, iOS covers the keyboard with a first-time QuickPath intro sheet.
  // @e2e-dev/mobile flags nodes inside the keyboard band as hidden, so locate by testId + text
  // and tap its center directly when present.
  const introContinue = screen.getByTestId('UIContinuousPathIntroductionView').getByText('Continue');
  if ((await introContinue.count()) > 0) {
    const box = await introContinue.boundingBox();
    if (box) {
      await screen.tapAt({ x: box.x + box.width / 2, y: box.y + box.height / 2 });
    }
  }
  await device.setClipboard(draft);
  await paste.tap();

  await expect.poll(() => {
    try {
      return readdirSync(documents)
        .filter((name) => name.endsWith('.md'))
        .map((name) => readFileSync(join(documents, name), 'utf8').trim());
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return [];
      throw error;
    }
  }).toContain(draft);
});

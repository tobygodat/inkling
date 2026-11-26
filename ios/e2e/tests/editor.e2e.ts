import { test } from '@e2e-dev/mobile';
import { openDraft, paste } from '../helpers/ios.ts';
import { expectDraft, resetSandbox } from '../helpers/sandbox.ts';

test('pasting in the editor autosaves the draft', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, '');
  const draft = 'the river held the light';
  // The GPUI editor is one native canvas, so focus it by position. The Paste control is UIKit.
  await screen.tapAt({ x: 200, y: 300 });
  await paste(screen, device, draft);
  await expectDraft(draft);
});

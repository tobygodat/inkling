import { test } from '@e2e-dev/mobile';
import { openDraft, paste, stepShot, tap, tapKeys } from '../helpers/ios.ts';
import { expectDraft, resetSandbox } from '../helpers/sandbox.ts';

const TYPED_LINE = 'the light held';
const PASTED_TAIL = ' and i dont know wich way teh river went';

test('writing, pasting, and reviewing grammar changes end to end', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, '');

  // empty
  await expectDraft('');
  await stepShot(app, 'empty');

  // written: type a line on the software keyboard, then paste the misspelled tail
  await tap(screen, device, 'page', 800);
  await tapKeys(screen, TYPED_LINE);
  await paste(screen, device, PASTED_TAIL);
  await expectDraft(`The light held${PASTED_TAIL}`);
  await stepShot(app, 'written');

  // keyboard-down
  await tap(screen, device, 'header-done', 800);
  await stepShot(app, 'keyboard-down');

  // review
  await tap(screen, device, 'fix-grammar', 3_000);
  await stepShot(app, 'review');

  // change-tapped ("teh" on the second line of the review)
  await tap(screen, device, 'change-teh', 800);
  await stepShot(app, 'change-tapped');

  // used
  await tap(screen, device, 'use', 800);
  await stepShot(app, 'used');

  // kept
  await tap(screen, device, 'keep', 800);
  await stepShot(app, 'kept');

  // accepted: Accept the rest applies the remaining changes and returns to the draft
  await tap(screen, device, 'accept-rest', 1_500);
  await expectDraft("The light held and I don't know which way the river went");
  await stepShot(app, 'accepted');
});

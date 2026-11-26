import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { capture3xWidth, editMenu, openDraft, pan, pause, stepShot, tap, tapKeys } from '../helpers/ios.ts';
import { POINTS } from '../helpers/points.ts';
import { expectDraft, expectDraftLetters, readDraft, resetSandbox } from '../helpers/sandbox.ts';

const TWO_BLOCKS = 'The light held over the river\n\nSecond paragraph with **bold** text here';
const MATHS = [
  '# The river',
  '',
  'The light held and i dont know wich way teh river went. Euler wrote $e^{i\\pi} + 1 = 0$ and it **held**.',
  '',
  '$$',
  '\\int_0^1 x^2\\,dx = \\frac{1}{3}',
  '$$',
].join('\n');

test.describe('live editor on iPhone', { serial: true }, () => {
  test('typing, headings, autocorrect, and suggestion bar', async ({ app, device, screen }) => {
    resetSandbox();

    // live-typed
    await openDraft(app, '');
    await tap(screen, device, 'page', 800);
    await tapKeys(screen, 'the light held');
    await expectDraft('The light held');
    await expect(screen.getByLabel('Paste', { exact: true })).toBeVisible();
    await stepShot(app, 'live-typed');

    // live-heading
    await openDraft(app, '');
    await tap(screen, device, 'page', 800);
    await tapKeys(screen, '# title//the end');
    await expectDraftLetters('# title\n\nthe end');
    await stepShot(app, 'live-heading');

    // live-autocorrect
    await tapKeys(screen, ' becuase ');
    await expectDraftLetters('# title\n\nthe end because ');
    await stepShot(app, 'live-autocorrect');

    // live-suggestion
    await tapKeys(screen, 'rive');
    await pause(800);
    await screen.getByRole('button', 'river').tap();
    await expectDraftLetters('# title\n\nthe end because river ');
    await stepShot(app, 'live-suggestion');
  });

  test('word selection, handle drag across blocks, paste, maths, and review', async ({ app, device, screen }) => {
    // live-select
    await openDraft(app, TWO_BLOCKS);
    await tap(screen, device, 'line-one-end', 1_200);
    await screen.doubleTapAt(POINTS['word-light']);
    await pause(1_200);
    await stepShot(app, 'live-select');
    await editMenu(device, 'Copy');
    await expect.poll(() => device.clipboard(), { timeout: 4_000 }).toBe('light');

    // live-handles
    await screen.doubleTapAt(POINTS['word-paragraph']);
    await pause(1_200);
    await pan(screen, POINTS['start-handle'], -30, -38, 900);
    await pause(1_200);
    await stepShot(app, 'live-handles');
    await editMenu(device, 'Copy');
    await expect
      .poll(() => device.clipboard(), { timeout: 4_000 })
      .toBe('light held over the river\n\nSecond paragraph');

    // live-paste
    await tap(screen, device, 'line-two-end', 800);
    await screen.getByLabel('Paste', { exact: true }).tap();
    await expectDraft(`${TWO_BLOCKS}light held over the river\n\nSecond paragraph`);
    await stepShot(app, 'live-paste');

    // live-maths
    await openDraft(app, MATHS);
    expect(readDraft()).toBe(MATHS);
    await stepShot(app, 'live-maths', device);
    expect(await capture3xWidth(device)).toBe(1206);

    // live-review
    await openDraft(app, 'The light held and i dont know wich way teh river went');
    await tap(screen, device, 'fix-grammar', 3_000);
    await tap(screen, device, 'use', 800);
    await tap(screen, device, 'keep', 800);
    await tap(screen, device, 'keep', 800);
    await tap(screen, device, 'keep', 1_500);
    await expectDraft("The light held and I don't know wich way teh river went");
    await stepShot(app, 'live-review');
  });
});

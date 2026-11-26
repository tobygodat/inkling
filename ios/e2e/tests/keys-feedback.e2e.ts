import { existsSync, readFileSync } from 'node:fs';
import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openDraft, pan, paste, pause, stepShot, tap, tapKeys } from '../helpers/ios.ts';
import { dataFilesContain, resetSandbox, sentFeedbackPath, settingsPath } from '../helpers/sandbox.ts';

const FAKE_KEY = 'sk-ant-snap-FAKE-0000';
const GOOD_LINK = 'https://hooks.example/inkling#k=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
// The fake transport fails any .invalid host as offline.
const FAILING_LINK = 'https://hooks.invalid/inkling#k=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';

test('keys, feedback link redaction, and sending feedback', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, "The light held and I don't know which way the river went");

  const scrollKeyScreen = async (dir: 'down' | 'up'): Promise<void> => {
    if (dir === 'down') await pan(screen, { x: 380, y: 700 }, 0, -400, 600);
    else await pan(screen, { x: 380, y: 300 }, 0, 400, 600);
    await pause(800);
  };

  const saveLink = async (link: string): Promise<void> => {
    await tap(screen, device, 'open-keys', 1_200);
    await scrollKeyScreen('down');
    await tap(screen, device, 'feedback-link', 800);
    await paste(screen, device, link);
    await pause(800);
    await tapKeys(screen, '/');
    await pause(1_200);
  };

  // keys
  await tap(screen, device, 'add-key', 1_200);
  await stepShot(app, 'keys');

  // key-pasted
  await tap(screen, device, 'key-field', 800);
  await paste(screen, device, FAKE_KEY);
  await pause(800);
  await tap(screen, device, 'reveal-key', 800);
  await stepShot(app, 'key-pasted');

  // key-saved: settings.json appears only after Return saves
  expect(existsSync(settingsPath())).toBe(false);
  await tapKeys(screen, '/');
  await expect.poll(() => existsSync(settingsPath()), { timeout: 5_000 }).toBe(true);
  await stepShot(app, 'key-saved');

  // back-with-key
  await tap(screen, device, 'back', 1_200);
  await stepShot(app, 'back-with-key');

  // link-saved: secret fragment must never land in any data file
  await saveLink(GOOD_LINK);
  expect(dataFilesContain('#k=')).toBe(false);
  await stepShot(app, 'link-saved');
  await scrollKeyScreen('up');
  await tap(screen, device, 'back', 1_200);

  // feedback-open
  await tap(screen, device, 'open-feedback', 1_200);
  await stepShot(app, 'feedback-open');

  // feedback-sent: stores the feedback text and never the draft
  const sentFile = sentFeedbackPath();
  await tap(screen, device, 'feedback-field', 800);
  await tapKeys(screen, 'bigger margins');
  await tap(screen, device, 'feedback-done', 800);
  await tap(screen, device, 'send-feedback', 1_200);
  await expect.poll(() => (existsSync(sentFile) ? readFileSync(sentFile, 'utf8') : ''), { timeout: 6_000 }).toContain(
    '"text": "Bigger margins"',
  );
  expect(readFileSync(sentFile, 'utf8').toLowerCase()).not.toContain('the light held');
  await stepShot(app, 'feedback-sent');

  // feedback-failed: offline .invalid link does not append to sent-feedback.json
  await tap(screen, device, 'feedback-back', 1_200);
  await saveLink(FAILING_LINK);
  await scrollKeyScreen('up');
  await tap(screen, device, 'back', 1_200);
  await tap(screen, device, 'open-feedback', 1_200);
  await tap(screen, device, 'feedback-field', 800);
  await tapKeys(screen, 'a darker caret');
  await tap(screen, device, 'feedback-done', 800);
  await tap(screen, device, 'send-feedback', 1_500);
  const idMatches = readFileSync(sentFile, 'utf8').match(/"id"/g) ?? [];
  expect(idMatches).toHaveLength(1);
  await stepShot(app, 'feedback-failed');
});

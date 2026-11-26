import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Device } from '@e2e-dev/mobile';
import { expect, type App, type Screen } from 'e2e';
import { POINTS, type Point, type PointName } from './points.ts';
import { APP_BUNDLE_ID, seedDraft } from './sandbox.ts';

/**
 * English QWERTY key coordinates on the iPhone 18 Pro portrait software keyboard, matching ios/keys.sh.
 * Tapping individual keys keeps the software keyboard up and fires autocorrect & suggestions like a finger.
 */
const KEYS: Readonly<Record<string, Point>> = {
  q: { x: 24, y: 618 }, w: { x: 63, y: 618 }, e: { x: 102, y: 618 }, r: { x: 141, y: 618 }, t: { x: 180, y: 618 },
  y: { x: 220, y: 618 }, u: { x: 259, y: 618 }, i: { x: 299, y: 618 }, o: { x: 339, y: 618 }, p: { x: 378, y: 618 },
  a: { x: 43, y: 672 }, s: { x: 82, y: 672 }, d: { x: 122, y: 672 }, f: { x: 161, y: 672 }, g: { x: 200, y: 672 },
  h: { x: 240, y: 672 }, j: { x: 279, y: 672 }, k: { x: 318, y: 672 }, l: { x: 358, y: 672 },
  z: { x: 82, y: 727 }, x: { x: 122, y: 727 }, c: { x: 161, y: 727 }, v: { x: 200, y: 727 }, b: { x: 240, y: 727 },
  n: { x: 279, y: 727 }, m: { x: 318, y: 727 }, '<': { x: 372, y: 727 },
  ' ': { x: 200, y: 780 }, '/': { x: 349, y: 780 },
};

const KEY_123: Point = { x: 28, y: 780 };
const KEY_SYMBOLS: Point = { x: 28, y: 727 };
const KEY_HASH: Point = { x: 180, y: 618 };

export const pause = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Dismisses iOS's first-boot QuickPath keyboard intro sheet when present.
 * @e2e-dev/mobile flags nodes inside the keyboard band as hidden, so locate by testId + text
 * and tap its boundingBox center directly.
 */
export async function dismissKeyboardIntro(screen: Screen): Promise<void> {
  const introContinue = screen.getByTestId('UIContinuousPathIntroductionView').getByText('Continue');
  if ((await introContinue.count()) > 0) {
    const box = await introContinue.boundingBox();
    if (box) {
      await screen.tapAt({ x: box.x + box.width / 2, y: box.y + box.height / 2 });
    }
  }
}

export async function assertForeground(device: Device, context: string): Promise<void> {
  const app = await device.foregroundApp();
  expect(app.bundleId, `Inkling left foreground after ${context}`).toBe(APP_BUNDLE_ID);
}

/** Taps a named iPhone 18 Pro coordinate (or explicit Point) and checks Inkling stayed in foreground. */
export async function tap(
  screen: Screen,
  device: Device,
  target: PointName | Point,
  settleMs = 400,
): Promise<void> {
  const point = typeof target === 'string' ? POINTS[target] : target;
  const label = typeof target === 'string' ? target : `(${point.x}, ${point.y})`;
  await screen.tapAt(point);
  if (settleMs > 0) await pause(settleMs);
  await assertForeground(device, label);
}

/** Types on the iPhone 18 Pro English software keyboard by tapping keys (lowercase, space, '<', '/', '#'). */
export async function tapKeys(screen: Screen, text: string): Promise<void> {
  await dismissKeyboardIntro(screen);
  for (const ch of text) {
    if (ch === '#') {
      await screen.tapAt(KEY_123);
      await screen.tapAt(KEY_SYMBOLS);
      await screen.tapAt(KEY_HASH);
      continue;
    }
    const point = KEYS[ch];
    if (!point) throw new Error(`Unsupported software key: "${ch}"`);
    await screen.tapAt(point);
  }
}

/** Writes text to the simulator clipboard and pastes it via the UIPasteControl above the keyboard. */
export async function paste(screen: Screen, device: Device, text: string): Promise<void> {
  const pasteButton = screen.getByLabel('Paste', { exact: true });
  await expect(pasteButton).toBeVisible();
  await dismissKeyboardIntro(screen);
  await device.setClipboard(text);
  await pasteButton.tap();
}

/** Seeds draft.md and relaunches Inkling fresh with --fake-ai. */
export async function openDraft(app: App, text: string, settleMs = 1_500): Promise<void> {
  seedDraft(text);
  await app.open();
  if (settleMs > 0) await pause(settleMs);
}

/** Pans from `from` by `(dx, dy)` over `duration` ms. */
export async function pan(screen: Screen, from: Point, dx: number, dy: number, duration: number): Promise<void> {
  await screen.swipe({
    from,
    to: { x: from.x + dx, y: from.y + dy },
    duration,
  });
}

/**
 * Bare-point long press. @e2e-dev/mobile 0.12.9 has no public screen.longPressAt() yet;
 * keep the unstable_raw escape hatch isolated here so it is a one-line swap upstream.
 */
export async function longPress(device: Device, target: PointName | Point, durationMs = 900): Promise<void> {
  const point = typeof target === 'string' ? POINTS[target] : target;
  const { client } = device.unstable_raw;
  await client.interactions.longPress({ x: point.x, y: point.y, durationMs });
}

/** Taps an item in UIKit's edit menu (e.g. "Copy"). */
export async function editMenu(device: Device, label: string): Promise<void> {
  await device.locator(`role="cell" label="${label}"`).tap();
}

/** Reads pixel width from a PNG file's IHDR header. */
export function pngWidth(filePath: string): number {
  const buf = readFileSync(filePath);
  return buf.readUInt32BE(16);
}

export function filesDiffer(a: string, b: string): boolean {
  return !readFileSync(a).equals(readFileSync(b));
}

/** Captures a 3x Retina PNG via agent-device (matching ios/snap.sh's --pixel-density 3) and returns its width. */
export async function capture3xWidth(device: Device): Promise<number> {
  const dir = mkdtempSync(join(tmpdir(), 'inkling-e2e-3x-'));
  const file = join(dir, 'shot.png');
  try {
    const { client } = device.unstable_raw;
    await client.capture.screenshot({ path: file, pixelDensity: 3 });
    return pngWidth(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Captures a named step screenshot and optionally exports a 3x PNG when INKLING_E2E_SNAP_OUT is set. */
export async function stepShot(app: App, name: string, device?: Device): Promise<string> {
  const shotPath = await app.screenshot(name);
  const outDir = process.env.INKLING_E2E_SNAP_OUT;
  if (outDir) {
    mkdirSync(outDir, { recursive: true });
    const dest = join(outDir, `${name}.png`);
    if (device) {
      await device.unstable_raw.client.capture.screenshot({ path: dest, pixelDensity: 3 });
    } else if (existsSync(shotPath)) {
      copyFileSync(shotPath, dest);
    }
  }
  return shotPath;
}

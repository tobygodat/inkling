import { test } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openDraft, stepShot, tap } from '../helpers/ios.ts';
import { resetSandbox } from '../helpers/sandbox.ts';

const TWO_BLOCKS = 'The light held over the river\n\nSecond paragraph with **bold** text here';

interface SnapshotNode {
  readonly role?: string;
  readonly value?: string;
  readonly focused?: boolean;
  readonly rect?: { readonly height?: number };
}

test('focused page reports its source and 44pt+ target to iOS accessibility', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, TWO_BLOCKS);
  await tap(screen, device, 'line-one-end', 1_200);

  const { client } = device.unstable_raw;
  const snapshot = (await client.capture.snapshot({})) as { readonly nodes?: readonly SnapshotNode[] };
  const page = (snapshot.nodes ?? []).find((node) => node.role === 'GPUITextInputView');

  // a11y-ios
  expect(page, 'focused GPUITextInputView must appear in the iOS accessibility snapshot').toBeDefined();
  expect(page?.value).toBe(TWO_BLOCKS);
  expect(page?.focused).toBe(true);
  await stepShot(app, 'a11y-ios');

  // a11y-target
  const height = page?.rect?.height ?? 0;
  expect(height).toBeGreaterThanOrEqual(44);
  await stepShot(app, 'a11y-target');
});

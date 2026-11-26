import { test, type Device } from '@e2e-dev/mobile';
import { expect } from 'e2e';
import { openDraft, pause, stepShot, tap } from '../helpers/ios.ts';
import { resetSandbox } from '../helpers/sandbox.ts';

const TWO_BLOCKS = 'The light held over the river\n\nSecond paragraph with **bold** text here';
const FORMULAS = 'Energy is $E = mc^2$ by ![the harbour](https://img.example/harbour.png).\n\n$$\n\\sum_{k=1}^{n} k\n$$\n\nThe last paragraph.';

interface SnapshotNode {
  readonly role?: string;
  readonly label?: string;
  readonly value?: string;
  readonly identifier?: string;
  readonly focused?: boolean;
  readonly selected?: boolean;
  readonly rect?: { readonly y?: number; readonly width?: number; readonly height?: number };
}

async function snapshot(device: Device): Promise<readonly SnapshotNode[]> {
  const { client } = device.unstable_raw;
  const tree = (await client.capture.snapshot({})) as { readonly nodes?: readonly SnapshotNode[] };
  return tree.nodes ?? [];
}

const byLabel = (nodes: readonly SnapshotNode[], label: string): SnapshotNode | undefined =>
  nodes.find((node) => node.label === label);

test('GPUI controls reach iOS accessibility with labels and frames before the page is tapped', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, FORMULAS);
  const nodes = await snapshot(device);

  // a11y-controls
  for (const [label, identifier] of [['Appearance', 'cog'], ['Writing', 'show-writing'], ['Reading', 'show-reading'], ['Find', 'open-find']]) {
    const node = byLabel(nodes, label);
    expect(node, `${label} must appear in the iOS accessibility snapshot`).toBeDefined();
    expect(node?.identifier).toBe(identifier);
    expect(node?.rect?.width ?? 0).toBeGreaterThan(0);
    expect(node?.rect?.height ?? 0).toBeGreaterThanOrEqual(44);
  }
  expect(byLabel(nodes, 'Writing')?.selected).toBe(true);
  // The page, unfocused, reads its placeholder and the draft; the formulas and images read their labels.
  expect(byLabel(nodes, 'Start writing')?.value).toBe(FORMULAS);
  for (const label of ['Formula E = mc^2', 'Formula \\sum_{k=1}^{n} k', 'the harbour']) {
    expect(byLabel(nodes, label), `${label} must appear in the iOS accessibility snapshot`).toBeDefined();
  }
  expect(nodes.some((node) => node.role === 'GPUITextInputView'), 'no text input view before the page has focus').toBe(false);
  await stepShot(app, 'a11y-controls');

  // a11y-sheet: the Appearance sheet hides what it covers and shows the page only above it.
  await screen.getByLabel('Appearance', { exact: true }).tap();
  await pause(600);
  const sheet = await snapshot(device);
  expect(byLabel(sheet, 'Done')?.identifier).toBe('appearance-done');
  expect(byLabel(sheet, 'Send'), 'the sheet covers Send').toBeUndefined();
  const page = byLabel(sheet, 'Start writing')?.rect;
  expect((page?.y ?? 0) + (page?.height ?? 0)).toBeLessThanOrEqual(byLabel(sheet, 'Done')?.rect?.y ?? 0);
  await stepShot(app, 'a11y-sheet');
  await screen.getByLabel('Done', { exact: true }).tap();
  await pause(600);
  expect(byLabel(await snapshot(device), 'Send'), 'Send is back once the sheet closes').toBeDefined();

  // a11y-press: a tap found by label reaches GPUI.
  await screen.getByLabel('Reading', { exact: true }).tap();
  await pause(600);
  expect(byLabel(await snapshot(device), 'Reading')?.selected).toBe(true);
  await stepShot(app, 'a11y-press');
});

test('focused page reports its source and 44pt+ target to iOS accessibility', async ({ app, device, screen }) => {
  resetSandbox();
  await openDraft(app, TWO_BLOCKS);
  await tap(screen, device, 'line-one-end', 1_200);

  const nodes = await snapshot(device);
  const page = nodes.find((node) => node.role === 'GPUITextInputView');

  // a11y-ios
  expect(page, 'focused GPUITextInputView must appear in the iOS accessibility snapshot').toBeDefined();
  expect(page?.value).toBe(TWO_BLOCKS);
  expect(page?.focused).toBe(true);
  // It stands in for the page's own element, so the page is not read twice.
  expect(byLabel(nodes, 'Start writing'), 'the focused page is read once, from the text input view').toBeUndefined();
  await stepShot(app, 'a11y-ios');

  // a11y-target
  const height = page?.rect?.height ?? 0;
  expect(height).toBeGreaterThanOrEqual(44);
  await stepShot(app, 'a11y-target');
});

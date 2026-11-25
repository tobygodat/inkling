import { mobile } from '@e2e-dev/mobile';
import type { E2EConfig } from 'e2e';

const device = process.env.INKLING_E2E_SIMULATOR;
if (!device || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(device)) {
  throw new Error('Set INKLING_E2E_SIMULATOR to the UUID of a booted, disposable iPhone simulator.');
}

export default {
  targets: [{
    name: 'ios',
    engine: mobile({ platform: 'ios', device }),
    app: { bundleId: 'com.tobygodat.inkling', launchArguments: ['--fake-ai'] },
  }],
  workers: 1,
  retries: 0,
} satisfies E2EConfig;

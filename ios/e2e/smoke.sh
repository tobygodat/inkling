#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export E2E_TELEMETRY_DISABLED=1

simulator="${INKLING_E2E_SIMULATOR:-}"
if ! [[ "$simulator" =~ ^[[:xdigit:]]{8}(-[[:xdigit:]]{4}){3}-[[:xdigit:]]{12}$ ]]; then
  echo 'Set INKLING_E2E_SIMULATOR to a booted, disposable iPhone simulator UUID.' >&2
  exit 2
fi
# A real device cannot pass this check. Do not reuse a simulator with existing Inkling data.
devices="$(xcrun simctl list devices booted --json)"
SIMULATORS="$devices" node --input-type=module -e '
  const devices = Object.values(JSON.parse(process.env.SIMULATORS).devices).flat();
  if (!devices.some((d) => d.udid === process.env.INKLING_E2E_SIMULATOR &&
      d.state === "Booted" && d.deviceTypeIdentifier === "com.apple.CoreSimulator.SimDeviceType.iPhone-18-Pro")) {
    console.error("Boot a disposable iPhone 18 Pro simulator for this smoke test.");
    process.exit(2);
  }
'
apps="$(xcrun simctl listapps "$simulator")"
if [[ "$apps" == *com.tobygodat.inkling* ]]; then
  echo 'This simulator already has Inkling installed; use a fresh test simulator.' >&2
  exit 2
fi
test -x node_modules/.bin/e2e || { echo 'Install the pinned e2e dependencies first.' >&2; exit 2; }
../run.sh
xcrun simctl install "$simulator" "../target/xcode/Build/Products/Debug-iphonesimulator/Inkling.app"
node_modules/.bin/e2e run --target ios

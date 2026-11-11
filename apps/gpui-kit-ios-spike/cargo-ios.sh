#!/bin/bash
# Xcode build phase: builds libgpui_kit_ios_spike.a for the SDK Xcode is building.
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
case "${PLATFORM_NAME:-iphonesimulator}" in
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  iphoneos) target=aarch64-apple-ios ;;
  *) echo "error: unsupported platform ${PLATFORM_NAME}" >&2; exit 1 ;;
esac
unset SDKROOT
export IPHONEOS_DEPLOYMENT_TARGET=18.0
cargo build --target "$target"

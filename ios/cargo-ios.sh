#!/bin/bash
# Xcode build phase: builds libinkling_ios.a for the SDK and configuration Xcode is building.
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
case "${PLATFORM_NAME:-iphonesimulator}" in
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  iphoneos) target=aarch64-apple-ios ;;
  *) echo "error: unsupported platform ${PLATFORM_NAME}" >&2; exit 1 ;;
esac
profile=()
[ "${CONFIGURATION:-Debug}" = Release ] && profile=(--release)
unset SDKROOT
export IPHONEOS_DEPLOYMENT_TARGET=18.0
# The build number in the feedback body: the commit count, as mac/install.sh uses for the Mac.
INKLING_BUILD="$(git -C .. rev-list --count HEAD)"
export INKLING_BUILD
cargo build --target "$target" ${profile[@]+"${profile[@]}"}

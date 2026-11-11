#!/bin/bash
# Fetches and patches gpui-mobile, builds the Rust static library, generates the Xcode project,
# builds the Simulator app and prints its path.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
./fetch-mobile.sh
./cargo-ios.sh
xcodegen generate --spec ios/project.yml --project ios --quiet
xcodebuild -project ios/GpuiKitSpike.xcodeproj -scheme GpuiKitSpike -configuration Debug \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$root/target/xcode" CODE_SIGNING_ALLOWED=NO -quiet build
echo "$root/target/xcode/Build/Products/Debug-iphonesimulator/GpuiKitSpike.app"

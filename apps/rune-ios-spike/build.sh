#!/bin/bash
# Builds the Rust library, generates the Xcode project, builds the Simulator app and prints its path.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
./cargo-ios.sh
xcodegen generate --spec ios/project.yml --project ios --quiet
xcodebuild -project ios/RuneSpike.xcodeproj -scheme RuneSpike -configuration Debug \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$root/target/xcode" CODE_SIGNING_ALLOWED=NO -quiet build
echo "$root/target/xcode/Build/Products/Debug-iphonesimulator/RuneSpike.app"

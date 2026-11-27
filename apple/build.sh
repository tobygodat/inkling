#!/bin/sh
# Builds the native apps of ADR 0007, unsigned, for the Mac and the iPhone Simulator into
# target/apple. It runs apple/core.sh first unless SKIP_CORE=1 and InklingCore is already built.
# Needs XcodeGen (`brew install xcodegen`). Mac only, and it opens nothing.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if [ "${SKIP_CORE:-0}" != 1 ] || [ ! -d apple/InklingCore/InklingFFI.xcframework ]; then
    apple/core.sh
fi

xcodegen generate --spec apple/project.yml --quiet

build() {
    xcodebuild -project apple/Inkling.xcodeproj -scheme "$1" -configuration Debug -destination "$2" \
        -derivedDataPath target/apple CODE_SIGNING_ALLOWED=NO -quiet build
}
build InklingMac "platform=macOS"
build InklingPhone "generic/platform=iOS Simulator"
echo "Built target/apple/Build/Products/Debug/Inkling.app and Debug-iphonesimulator/Inkling.app"

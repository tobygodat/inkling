#!/bin/bash
# Builds Inkling for the iPhone Simulator, installs it and launches it through the T3 device
# tools' agent-device command. Usage: run.sh <agent-device> [launch flags]
# Launch flags (Debug builds only): --fake-ai for canned answers with keys in memory,
# --memory-keys for keys in memory with no fake model (the missing-key path).
# With no agent-device it only builds and prints the app's path.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
ad="${1:-}"
[ $# -gt 0 ] && shift
./fetch-mobile.sh
./cargo-ios.sh
xcodegen generate --spec app/project.yml --project app --quiet
xcodebuild -project app/Inkling.xcodeproj -scheme Inkling -configuration Debug \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$root/target/xcode" CODE_SIGNING_ALLOWED=NO -quiet build
app="$root/target/xcode/Build/Products/Debug-iphonesimulator/Inkling.app"
echo "$app"
[ -n "$ad" ] || exit 0
"$ad" install com.tobygodat.inkling "$app"
launch=()
for flag in "$@"; do launch+=(--launch-args "$flag"); done
"$ad" open com.tobygodat.inkling --relaunch ${launch[@]+"${launch[@]}"}

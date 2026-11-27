#!/bin/bash
# Renders every native screen in Light and Dark and asserts what they show (ADR 0007).
#
#   apple/snap.sh mac               offscreen on the Mac, in a test bundle: nothing takes focus
#   apple/snap.sh iphone <udid>     on a booted Simulator, through XCUITest, headless
#
# Screenshots go to docs/screenshots/apple/<mac|iphone>/, or to INKLING_SNAP_OUT. Both run on a temp
# data folder with the fake model, from the demo in apple/Demo and the fixture books. Runs only on
# a Mac with Xcode and XcodeGen; apple/core.sh must have built InklingCore first.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
where=${1:?usage: apple/snap.sh mac | iphone <udid>}
out="${INKLING_SNAP_OUT:-$root/docs/screenshots/apple}"
books/fetch-fixtures.sh >/dev/null
xcodegen generate --spec apple/project.yml --project apple --quiet
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT
export TEST_RUNNER_INKLING_DEMO="$root/apple/Demo"
export TEST_RUNNER_INKLING_FIXTURES="$root/target/fixtures"
export TEST_RUNNER_INKLING_FAKE_AI=1
derived="$root/target/apple"
# The result bundle stays in target/apple for a failure to be read: xcrun xcresulttool get test-results tests --path <it>.
rm -rf "$derived/$where.xcresult"
status=0

case "$where" in
  mac)
    mkdir -p "$out/mac"
    TEST_RUNNER_INKLING_SNAP_OUT="$out" xcodebuild test -project apple/Inkling.xcodeproj -scheme MacSnapshots \
      -destination 'platform=macOS' -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO \
      -resultBundlePath "$derived/mac.xcresult" -quiet || status=$?
    ;;
  iphone)
    udid=${2:?usage: apple/snap.sh iphone <udid>}
    xcrun simctl status_bar "$udid" override --time 9:41 --batteryState charged --batteryLevel 100 --cellularBars 4 --wifiBars 3
    TEST_RUNNER_INKLING_SNAP_DATA="$temp/data" xcodebuild test -project apple/Inkling.xcodeproj -scheme PhoneSnapshots \
      -destination "platform=iOS Simulator,id=$udid" -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO \
      -resultBundlePath "$derived/iphone.xcresult" -quiet || status=$?
    xcrun simctl status_bar "$udid" clear
    mkdir -p "$out/iphone" "$temp/attachments"
    xcrun xcresulttool export attachments --path "$derived/iphone.xcresult" --output-path "$temp/attachments" >/dev/null
    python3 - "$temp/attachments" "$out/iphone" <<'EOF'
import json, shutil, sys
src, dst = sys.argv[1], sys.argv[2]
for test in json.load(open(f"{src}/manifest.json")):
    for a in test["attachments"]:
        name = a["suggestedHumanReadableName"].split("_")[0]
        if name.startswith("iphone-"):
            shutil.copy(f"{src}/{a['exportedFileName']}", f"{dst}/{name.removeprefix('iphone-')}.png")
EOF
    ;;
  *) echo "usage: apple/snap.sh mac | iphone <udid>" >&2; exit 2 ;;
esac
if [ "$status" -ne 0 ]; then
  echo "FAIL apple/snap.sh $where:" >&2
  xcrun xcresulttool get test-results tests --path "$derived/$where.xcresult" | grep -B1 '"Failure Message"' | grep '"name"' >&2 || true
  exit "$status"
fi
echo "ok   apple/snap.sh $where: screenshots in $out/$where"

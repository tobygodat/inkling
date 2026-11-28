#!/bin/bash
# Measures how smoothly the iPhone app moves on a booted Simulator: hitches and frame rate from the
# app's frame probe, and the app's CPU from XCTest, while the reader scrolls a long book and an
# article with pictures, the lists scroll, a row swipes, screens push and pop, sheets open and
# close, and the reader's gestures run. The idle journey, where nothing moves, is the floor the
# machine's own load sets.
#
#   apple/motion.sh <udid> [test…]
#
# Builds the app in Release, as it ships, runs each journey five times on a temp data folder with
# the fake model, and prints each metric's median. Runs only on a Mac with Xcode and XcodeGen;
# apple/core.sh must have built InklingCore first.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
udid=${1:?usage: apple/motion.sh <udid> [test…]}
shift
books/fetch-fixtures.sh >/dev/null
xcodegen generate --spec apple/project.yml --project apple --quiet
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT
derived="$root/target/apple"
result="$derived/motion.xcresult"
rm -rf "$result"
only=()
for test in "$@"; do only+=("-only-testing:InklingPhoneSnapshots/MotionMetrics/$test"); done
[ ${#only[@]} -eq 0 ] && only=(-only-testing:InklingPhoneSnapshots/MotionMetrics)

echo "System load: $(uptime)"
status=0
TEST_RUNNER_INKLING_MOTION_DATA="$temp/data" \
TEST_RUNNER_INKLING_DEMO="$root/apple/Demo" \
TEST_RUNNER_INKLING_FIXTURES="$root/target/fixtures" \
TEST_RUNNER_INKLING_FAKE_AI=1 \
  xcodebuild test -project apple/Inkling.xcodeproj -scheme PhoneSnapshots -configuration Release \
    -destination "platform=iOS Simulator,id=$udid" -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO \
    -resultBundlePath "$result" "${only[@]}" -quiet >"$temp/log" 2>&1 || status=$?
if [ "$status" -ne 0 ]; then
  tail -n 40 "$temp/log" >&2
  xcrun xcresulttool get test-results tests --path "$result" | grep -B1 '"Failure Message"' >&2 || true
  exit "$status"
fi
xcrun xcresulttool get test-results metrics --path "$result" >"$temp/metrics.json"
python3 - "$temp/metrics.json" <<'EOF'
import json, statistics, sys
wanted = ("hitch", "frame", "cpu time", "instructions")
for test in json.load(open(sys.argv[1])):
    print(test["testIdentifier"].split("/")[-1].removesuffix("()"))
    for run in test["testRuns"]:
        for metric in run["metrics"]:
            name = metric["displayName"]
            if not any(w in name.lower() for w in wanted):
                continue
            values = metric["measurements"]
            print(f"  {name:<52} median {statistics.median(values):9.2f} {metric.get('unitOfMeasurement', '')}"
                  f"   (runs {', '.join(f'{v:.1f}' for v in values)})")
EOF

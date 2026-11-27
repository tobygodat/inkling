#!/bin/bash
# Measures keystroke-to-painted-frame latency typing 50 characters into a 5,000-word draft
# (same draft and typed string as ios/bench.sh) on the Mac and on a booted iOS Simulator.
#
#   apple/bench.sh <udid> [rounds]
#
# Runs 5 rounds by default (per benchmark-checklist) and reports fastest, median and worst in ms.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
udid=${1:?usage: apple/bench.sh <udid> [rounds]}
rounds=${2:-5}

xcodegen generate --spec apple/project.yml --project apple --quiet
derived="$root/target/apple"
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT

echo "System load: $(uptime)"
echo "Running $rounds rounds per platform (50 chars into 5,000-word draft, same payload as ios/bench.sh)..."

echo ""
echo "=== Mac (TextKit 2, Debug test bundle) ==="
mac_medians=()
mac_worsts=()
for r in $(seq 1 "$rounds"); do
  out="$temp/mac-$r.log"
  probe="$temp/mac-samples-$r.txt"
  rm -f "$probe"
  TEST_RUNNER_INKLING_DEMO="$root/apple/Demo" \
  TEST_RUNNER_INKLING_FIXTURES="$root/target/fixtures" \
  TEST_RUNNER_INKLING_FAKE_AI=1 \
  TEST_RUNNER_INKLING_PROBE_OUT="$probe" \
  TEST_RUNNER_INKLING_SNAP_OUT="$temp/snap" \
    xcodebuild test -project apple/Inkling.xcodeproj -scheme MacSnapshots \
      -destination 'platform=macOS' -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO \
      '-only-testing:InklingMacSnapshots/MacSnapshots/typingStaysSmoothIn5000WordDraft()' \
      >"$out" 2>&1
  summary=$(sort -n "$probe" | awk '
    { t[NR] = $1 }
    END {
      if (NR == 0) { print "samples=0"; exit 1 }
      printf "samples=%d fastest=%.2fms median=%.2fms worst=%.2fms", NR, t[1] / 1000, t[int((NR + 1) / 2)] / 1000, t[NR] / 1000
    }')
  echo "  run $r: $summary"
  mac_medians+=("$(echo "$summary" | sed -E 's/.*median=([0-9.]+)ms.*/\1/')")
  mac_worsts+=("$(echo "$summary" | sed -E 's/.*worst=([0-9.]+)ms.*/\1/')")
done

echo ""
echo "=== iPhone Simulator (TextKit 2, Debug XCUITest) ==="
ios_medians=()
ios_worsts=()
for r in $(seq 1 "$rounds"); do
  dir="$temp/ios-$r"
  mkdir -p "$dir/documents"
  awk 'BEGIN {
    printf "# Benchmark\n\n"
    for (p = 0; p < 100; p++) {
      for (w = 0; w < 50; w++) printf "%s ", (w % 9 == 4 ? "**river**" : "light")
      printf "\n\n"
    }
  }' >"$dir/documents/bench.md"
  probe="$temp/ios-samples-$r.txt"
  rm -f "$probe"
  TEST_RUNNER_INKLING_BENCH_DATA="$dir" \
  TEST_RUNNER_INKLING_PROBE_OUT="$probe" \
  TEST_RUNNER_INKLING_FAKE_AI=1 \
    xcodebuild test -project apple/Inkling.xcodeproj -scheme PhoneSnapshots \
      -destination "platform=iOS Simulator,id=$udid" -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO \
      -only-testing:InklingPhoneSnapshots/PhoneSnapshots/testTypingBenchmark5000Words \
      >"$temp/ios-$r.log" 2>&1 || { tail -n 40 "$temp/ios-$r.log" >&2; exit 1; }
  summary=$(sort -n "$probe" | awk '
    { t[NR] = $1 }
    END {
      if (NR == 0) { print "samples=0"; exit 1 }
      printf "samples=%d fastest=%.2fms median=%.2fms worst=%.2fms", NR, t[1] / 1000, t[int((NR + 1) / 2)] / 1000, t[NR] / 1000
    }')
  echo "  run $r: $summary"
  ios_medians+=("$(echo "$summary" | sed -E 's/.*median=([0-9.]+)ms.*/\1/')")
  ios_worsts+=("$(echo "$summary" | sed -E 's/.*worst=([0-9.]+)ms.*/\1/')")
done

echo ""
echo "=== Summary across $rounds rounds ==="
python3 - "${mac_medians[*]}" "${mac_worsts[*]}" "${ios_medians[*]}" "${ios_worsts[*]}" <<'EOF'
import statistics, sys
def show(name, meds, worsts):
    m = [float(x) for x in meds.split() if x]
    w = [float(x) for x in worsts.split() if x]
    print(f"{name}: median-of-medians={statistics.median(m):.2f} ms (range {min(m):.2f}–{max(m):.2f} ms), median-worst={statistics.median(w):.2f} ms (max worst {max(w):.2f} ms)")
show("Mac (native TextKit 2)      ", sys.argv[1], sys.argv[2])
show("Simulator (native TextKit 2)", sys.argv[3], sys.argv[4])
print("Simulator (GPUI ios/bench.sh baseline, 2026-11-12): median=14.7 ms")
EOF

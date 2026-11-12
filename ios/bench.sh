#!/bin/bash
# Types 50 characters into a 5,000-word draft on the Simulator and prints the median and the
# worst time from a typed character to its painted frame. Usage: bench.sh <agent-device>
# The app must be installed already, as a Debug build (run.sh): it logs the times only there,
# with --probe-typing. A number from the Simulator is a floor for a real iPhone.
set -euo pipefail
cd "$(dirname "$0")"
ad="$1"
app=com.tobygodat.inkling
"$ad" open "$app" >/dev/null
udid="$("$ad" appstate --json | sed -n 's/.*"device_udid": "\(.*\)".*/\1/p')"
[ -n "$udid" ] || { echo "no Simulator udid from agent-device" >&2; exit 1; }
data="$(xcrun simctl get_app_container "$udid" "$app" data)/Library/Application Support/Inkling"
mkdir -p "$data"
awk 'BEGIN {
  for (p = 0; p < 100; p++) {
    for (w = 0; w < 50; w++) printf "%s ", (w % 9 == 4 ? "**river**" : "light")
    printf "\n\n"
  }
}' >"$data/draft.md"
"$ad" open "$app" --relaunch --launch-args --fake-ai --launch-args --probe-typing >/dev/null
sleep 3
"$ad" press 200 300 >/dev/null
sleep 2
"$ad" logs clear --restart >/dev/null 2>&1
typed="the light held over the river and the light held  "
./keys.sh "$ad" "$typed"
sleep 1
log="$("$ad" logs path 2>/dev/null | grep -o '/.*app\.log')"
"$ad" logs stop >/dev/null 2>&1
grep -qi "light held over the river and" "$data/draft.md" || { echo "the typed text is not in the draft" >&2; exit 1; }
grep -o 'typed to painted [0-9]* us' "$log" | awk '{ print $4 }' | sort -n | awk -v typed="${#typed}" '
  { t[NR] = $1 }
  END {
    if (NR != typed) { printf "%d samples for %d typed characters: is this a Debug build with the probe?\n", NR, typed; exit 1 }
    printf "samples %d  fastest %.1f ms  median %.1f ms  worst %.1f ms\n", NR, t[1] / 1000, t[int((NR + 1) / 2)] / 1000, t[NR] / 1000
  }'

#!/bin/bash
# Runs the agent-device command in $INKLING_AGENT_DEVICE with the arguments given, through the
# gaps a shared, busy or sleeping Mac leaves in it.
# - "Remote daemon is unavailable" comes before the request reaches the device, so any command is
#   run again.
# - "Runner command deadline exceeded" means the Simulator stalled. A command that only reads is
#   run again. A tap or gesture is not, since the runner still delivers it once the Simulator
#   wakes: this waits until the runner answers again and carries on, and the snap's next check
#   proves whether the tap landed.
set -uo pipefail
out="$(mktemp)"
err="$(mktemp)"
trap 'rm -f "$out" "$err"' EXIT
case "$1" in
  appstate | screenshot | snapshot | is | clipboard) reads=1 ;;
  *) reads=0 ;;
esac
[ "$1 ${2:-}" = "clipboard write" ] && reads=0
for attempt in $(seq 12); do
  "$INKLING_AGENT_DEVICE" "$@" >"$out" 2>"$err"
  status=$?
  if [ "$status" -ne 0 ] && [ "$attempt" -lt 12 ]; then
    if grep -q "Remote daemon is unavailable" "$out" "$err"; then
      sleep 5
      continue
    fi
    if grep -q "Runner command deadline exceeded" "$out" "$err"; then
      [ "$reads" = 1 ] && continue
      echo "device.sh: $1 outlasted the runner's deadline; waiting for the Simulator" >&2
      for _ in $(seq 12); do
        "$INKLING_AGENT_DEVICE" appstate --json >/dev/null 2>&1 && exit 0
        sleep 5
      done
    fi
  fi
  cat "$out"
  cat "$err" >&2
  exit "$status"
done

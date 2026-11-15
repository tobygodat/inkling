#!/bin/bash
# Runs the agent-device command in $INKLING_AGENT_DEVICE with the arguments given. When its
# daemon is unavailable, which happens for seconds at a time while other sessions share it, it
# waits and runs the command again. That error comes before the request reaches the device, so a
# tap is never sent twice.
set -uo pipefail
out="$(mktemp)"
err="$(mktemp)"
trap 'rm -f "$out" "$err"' EXIT
for attempt in $(seq 12); do
  "$INKLING_AGENT_DEVICE" "$@" >"$out" 2>"$err"
  status=$?
  if [ "$status" -ne 0 ] && [ "$attempt" -lt 12 ] && grep -q "Remote daemon is unavailable" "$out" "$err"; then
    sleep 5
    continue
  fi
  cat "$out"
  cat "$err" >&2
  exit "$status"
done

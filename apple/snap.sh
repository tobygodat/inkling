#!/bin/sh
# Runs InklingUI's tests, which check the reader's blocks and render the native app's screens
# offscreen on the Mac (light and dark) into docs/screenshots/apple/, or INKLING_SNAP_OUT when set.
# The window is never made key and sits off every screen, so nothing takes focus. Needs
# apple/core.sh to have built InklingCore. Mac only.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
INKLING_SNAP_OUT=${INKLING_SNAP_OUT:-$root/docs/screenshots/apple} \
    swift test --package-path "$root/apple/InklingUI"

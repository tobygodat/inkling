#!/bin/sh
# Rebuilds docs/screenshots/mac headless and asserts the app's behaviour as it goes.
# Offscreen render, input dispatched inside the app, fake AI, keys in memory, a temp data folder.
set -eu
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../docs/screenshots/mac"
cargo build --manifest-path "$here/../Cargo.toml" -p inkling-mac --features snap
bin="$here/../target/debug/inkling"
data=$(mktemp -d)
trap 'rm -rf "$data"' EXIT
INKLING_FAKE_AI=1 INKLING_DATA_DIR="$data/fake" "$bin" --snap "$out"
env -u INKLING_FAKE_AI INKLING_MEMORY_KEYS=1 INKLING_DATA_DIR="$data/nokey" "$bin" --snap "$out"
INKLING_FAKE_AI=1 INKLING_DATA_DIR="$data/live" "$bin" --snap-live "$out"
GPUI_A11Y_ACTIVE=1 INKLING_FAKE_AI=1 INKLING_DATA_DIR="$data/a11y" "$bin" --snap-a11y "$out"
echo "ok   a11y-10 every run of mac/snap.sh passed"

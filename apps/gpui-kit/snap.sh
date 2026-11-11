#!/bin/sh
# Rebuilds docs/screenshots/gpui-kit headless: offscreen Metal render, input dispatched inside the app.
set -eu
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../../docs/screenshots/gpui-kit"
cargo build --manifest-path "$here/Cargo.toml" --features snap
bin="$here/target/debug/inkling"
data=$(mktemp -d)
trap 'rm -rf "$data"' EXIT
INKLING_FAKE_AI=1 INKLING_DATA_DIR="$data/fake" "$bin" --snap "$out"
env -u INKLING_FAKE_AI INKLING_DATA_DIR="$data/nokey" "$bin" --snap "$out"

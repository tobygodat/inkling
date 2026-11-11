#!/bin/bash
# Rebuilds docs/screenshots/gpui-fast headless: GPUI's test platform with real CoreText shaping
# and Metal rendering to a texture. No window reaches the window server, nothing takes focus,
# keys stay in memory and the model is the core's fake one.
set -euo pipefail
cd "$(dirname "$0")"
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
OUT=$(cd ../.. && pwd)/docs/screenshots/gpui-fast
mkdir -p "$OUT"
cargo build --quiet --features snap --bin snap
DATA=$(mktemp -d)
INKLING_FAKE_AI=1 INKLING_DATA_DIR="$DATA/ai" target/debug/snap "$OUT" ai
env -u INKLING_FAKE_AI INKLING_DATA_DIR="$DATA/missing" target/debug/snap "$OUT" missing-key
rm -rf "$DATA"

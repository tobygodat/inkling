#!/bin/sh
# Renders the slice's proof screenshots headless into docs/screenshots/rune, driving the app
# with scripted key events. No window opens and nothing takes focus. Every run gets a fresh
# temp data folder; only 07 runs without INKLING_FAKE_AI.
set -eu
cd "$(dirname "$0")"
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build -q -p inkling-mac
bin=target/debug/inkling
out=../../docs/screenshots/rune
scripts=$(mktemp -d)
python3 scripts/gen.py "$scripts"
mkdir -p "$out"

snap() { # name, script or "-", then env assignments
  name=$1 script=$2
  shift 2
  data=$(mktemp -d)
  [ -n "${SETTINGS:-}" ] && printf '%s' "$SETTINGS" > "$data/settings.json"
  if [ "$script" = - ]; then
    env "$@" INKLING_DATA_DIR="$data" "$bin" --snap "$out/$name.png" --at 0.5 >/dev/null 2>&1
  else
    env "$@" INKLING_DATA_DIR="$data" "$bin" --script "$scripts/$script.json" --snap "$out/$name.png" --at mark:end >/dev/null 2>&1
  fi
  echo "$out/$name.png"
}

snap 01-empty - INKLING_FAKE_AI=1
snap 02-typed typed INKLING_FAKE_AI=1
snap 03-continue-streaming continue-streaming INKLING_FAKE_AI=1
snap 04-continue-accepted continue-accepted INKLING_FAKE_AI=1
snap 05-rewrite-proposal rewrite-proposal INKLING_FAKE_AI=1
snap 06-settings settings INKLING_FAKE_AI=1
SETTINGS='{"provider":"Gemini"}' snap 07-missing-key missing-key -u INKLING_FAKE_AI

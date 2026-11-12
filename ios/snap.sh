#!/bin/bash
# Drives Inkling's main flow on the iPhone Simulator and saves each step to
# docs/screenshots/ios/snap/. Exits non-zero when a step fails.
# Usage: snap.sh <agent-device>, or AGENT_DEVICE=<agent-device> snap.sh
# Get <agent-device> from the T3 device tools (device_open). It reinstalls the app, so every
# run starts from an empty page, with the fake model and keys in memory.
set -euo pipefail
cd "$(dirname "$0")"
ad="${1:-${AGENT_DEVICE:-}}"
[ -n "$ad" ] || { echo "usage: snap.sh <agent-device> (or set AGENT_DEVICE)" >&2; exit 2; }
app=com.tobygodat.inkling
shots="$(cd .. && pwd)/docs/screenshots/ios/snap"

# Where to tap, in points on the iPhone 18 Pro (402x874). A layout change is one edit here.
# Taps on the review's text depend on the line below; change both together.
line="the light held"
pasted=" and i dont know wich way teh river went"
key_text="sk-ant-snap-FAKE-0000"
good_link="https://hooks.example/inkling#k=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
# The fake transport fails any .invalid host as offline.
failing_link="https://hooks.invalid/inkling#k=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
point() {
  case "$1" in
    page) echo "200 300" ;;
    header-done) echo "364 84" ;;     # header, while the page has focus
    add-key) echo "351 84" ;;         # header, "Claude · add a key"
    fix-grammar) echo "72 752" ;;     # keyboard down
    change-teh) echo "176 182" ;;     # "teh", second line of the review
    keep) echo "106 746" ;;
    use) echo "295 746" ;;
    accept-rest) echo "324 93" ;;
    back) echo "80 105" ;;
    key-field) echo "200 357" ;;
    reveal-key) echo "355 357" ;;
    open-keys) echo "360 84" ;;       # header, "Claude" once a key is saved
    feedback-link) echo "200 690" ;;  # key screen, scrolled to the bottom
    open-feedback) echo "282 84" ;;   # header, left of the provider
    feedback-field) echo "200 250" ;;
    feedback-done) echo "355 105" ;;
    feedback-back) echo "80 105" ;;
    send-feedback) echo "200 337" ;;
    *) echo "no point named $1" >&2; exit 1 ;;
  esac
}

step=0
name=setup
fail() { echo "FAIL step $step ($name): $*" >&2; exit 1; }

# Starts the next step. Its screenshot is NN-<name>.png.
begin() {
  step=$((step + 1))
  name="$1"
}

# Taps `$1` and checks Inkling is still the foreground app.
tap() {
  # shellcheck disable=SC2046
  "$ad" press $(point "$1") >/dev/null
  sleep "${2:-1}"
  "$ad" appstate --json | grep -q "\"appBundleId\": \"$app\"" || fail "Inkling is not in the foreground after tapping $1"
}

shot() {
  "$ad" screenshot "$shots/$(printf %02d "$step")-$name.png" --pixel-density 3 >/dev/null
  echo "ok $step $name"
}

# A fresh agent-device session has no app open yet, and appstate fails until one is.
udid_now() { "$ad" appstate --json 2>/dev/null | sed -n 's/.*"device_udid": "\(.*\)".*/\1/p' || true; }
udid="$(udid_now)"
if [ -z "$udid" ]; then
  "$ad" open com.apple.Preferences >/dev/null 2>&1 || true
  udid="$(udid_now)"
fi
[ -n "$udid" ] || fail "no Simulator udid from agent-device"

# Scrolls the key screen to its link section, or back to the top.
scroll() {
  case "$1" in
    down) "$ad" gesture pan 380 700 0 -400 600 >/dev/null ;;
    up) "$ad" gesture pan 380 300 0 400 600 >/dev/null ;;
  esac
  sleep 1
}

# Opens the key screen, pastes `$1` into the link field with the Paste control and saves it
# with Return, then goes back to the page.
save_link() {
  tap open-keys 1.5
  scroll down
  tap feedback-link
  "$ad" clipboard write "$1" >/dev/null
  "$ad" press 'label="Paste"' >/dev/null
  sleep 1
  ./keys.sh "$ad" "/"
  sleep 1.5
}

data_dir() {
  echo "$(xcrun simctl get_app_container "$udid" "$app" data)/Library/Application Support/Inkling"
}

draft() {
  cat "$(data_dir)/draft.md" 2>/dev/null || true
}

# Waits up to 5 s for the saved draft to equal `$1`.
expect_draft() {
  local got
  for _ in $(seq 25); do
    got="$(draft)"
    [ "$got" = "$1" ] && return 0
    sleep 0.2
  done
  fail "draft is \"$got\", expected \"$1\""
}

rm -rf "$shots"
mkdir -p "$shots"
"$ad" uninstall "$app" >/dev/null 2>&1 || true
mkdir -p target
./run.sh "$ad" --fake-ai >target/snap-build.log 2>&1 || { tail -20 target/snap-build.log >&2; exit 1; }
sleep 2

begin empty
expect_draft ""
shot

begin written
tap page
./keys.sh "$ad" "$line"
"$ad" clipboard write "$pasted" >/dev/null
"$ad" press 'label="Paste"' >/dev/null
expect_draft "The light held$pasted"
shot

begin keyboard-down
tap header-done
shot

begin review
tap fix-grammar 3
shot

begin change-tapped
tap change-teh
shot

begin used
tap use
shot

begin kept
tap keep
shot

begin accepted
tap accept-rest 1.5
expect_draft "The light held and I don't know which way the river went"
shot

begin keys
tap add-key 1.5
shot

begin key-pasted
tap key-field
"$ad" clipboard write "$key_text" >/dev/null
"$ad" press 'label="Paste"' >/dev/null
sleep 1
tap reveal-key
shot

begin key-saved
[ ! -f "$(data_dir)/settings.json" ] || fail "settings.json existed before the key was saved"
./keys.sh "$ad" "/"
sleep 1.5
[ -f "$(data_dir)/settings.json" ] || fail "Return in the key field saved no settings"
shot

begin back-with-key
tap back 1.5
shot

begin link-saved
save_link "$good_link"
! grep -rl "#k=" "$(data_dir)" >/dev/null || fail "the link is in a data file"
shot
scroll up
tap back 1.5

begin feedback-open
tap open-feedback 1.5
shot

begin feedback-sent
sent="$(data_dir)/sent-feedback.json"
tap feedback-field
./keys.sh "$ad" "bigger margins"
tap feedback-done
tap send-feedback 1.5
for _ in $(seq 25); do [ -f "$sent" ] && break; sleep 0.2; done
grep -q '"text": "Bigger margins"' "$sent" || fail "sent-feedback.json has no \"Bigger margins\""
! grep -qi "the light held" "$sent" || fail "sent-feedback.json holds the draft"
shot

begin feedback-failed
tap feedback-back 1.5
save_link "$failing_link"
scroll up
tap back 1.5
tap open-feedback 1.5
tap feedback-field
./keys.sh "$ad" "a darker caret"
tap feedback-done
tap send-feedback 1.5
[ "$(grep -c '"id"' "$sent")" = 1 ] || fail "a failed send was added to sent-feedback.json"
shot

echo "all $step steps passed; screenshots in $shots"

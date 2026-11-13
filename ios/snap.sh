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
    # The live lanes. Their drafts are below; a change to either moves these.
    line-one-end) echo "300 148" ;;   # after "river", first line of the two-block draft
    word-light) echo "70 148" ;;      # "light", first line
    word-paragraph) echo "140 190" ;; # "paragraph", second block
    line-two-end) echo "380 190" ;;   # after "here", second block
    start-handle) echo "97 170" ;;    # the knob over the start of "paragraph", once selected
    # The library and reading. The Write and Read toggle sits at the top left.
    show-writing) echo "30 84" ;;
    show-reading) echo "74 84" ;;
    find-books) echo "276 140" ;;
    found-pdf) echo "150 341" ;;      # the second row Find books lists, the PDF
    add-found) echo "201 414" ;;
    book-pdf) echo "90 296" ;;        # first tile, the newest: the PDF
    book-epub) echo "274 296" ;;      # second tile
    page-forward) echo "370 450" ;;   # the page's right third
    contents) echo "343 90" ;;
    chapter-one) echo "120 189" ;;    # Contents' second row
    back-to-library) echo "148 90" ;;
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

# Waits up to 5 s for the saved draft to equal `$1` whatever its capitals.
expect_draft_letters() {
  local got
  for _ in $(seq 25); do
    got="$(draft | tr '[:upper:]' '[:lower:]')"
    [ "$got" = "$1" ] && return 0
    sleep 0.2
  done
  fail "draft is \"$got\", expected \"$1\" in any capitals"
}

# Relaunches on a draft of `$1`, with the page unfocused and the keyboard down.
open_draft() {
  printf '%s' "$1" >"$(data_dir)/draft.md"
  "$ad" open "$app" --relaunch --launch-args --fake-ai >/dev/null
  sleep 2.5
}

# Presses `$1` in UIKit's edit menu.
menu() {
  "$ad" press "role=\"cell\" label=\"$1\"" >/dev/null || fail "the edit menu has no $1"
  sleep 0.8
}

expect_clipboard() {
  local got
  got="$("$ad" clipboard read)"
  [ "$got" = "$1" ] || fail "the clipboard is \"$got\", expected \"$1\""
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

# The live editor on iPhone: the ten lanes of TOB-444's L3. The sixteen steps above are lane 1.
two_blocks="The light held over the river

Second paragraph with **bold** text here"
maths='# The river

The light held and i dont know wich way teh river went. Euler wrote $e^{i\pi} + 1 = 0$ and it **held**.

$$
\int_0^1 x^2\,dx = \frac{1}{3}
$$'

begin live-typed
open_draft ""
tap page
./keys.sh "$ad" "the light held"
expect_draft "The light held"
"$ad" is visible 'role="button" label="Paste"' >/dev/null || fail "the keyboard is not up"
shot

begin live-heading
open_draft ""
tap page
./keys.sh "$ad" "# title//the end"
expect_draft_letters "# title

the end"
shot

begin live-autocorrect
./keys.sh "$ad" " becuase "
expect_draft_letters "# title

the end because "
shot

begin live-suggestion
./keys.sh "$ad" "rive"
sleep 0.8
"$ad" press 'role="button" label="river"' >/dev/null || fail "the keyboard suggests no \"river\""
expect_draft_letters "# title

the end because river "
shot

begin live-select
open_draft "$two_blocks"
tap line-one-end 1.5
"$ad" press $(point word-light) --double-tap >/dev/null
sleep 1.5
shot
menu Copy
expect_clipboard "light"

begin live-handles
"$ad" press $(point word-paragraph) --double-tap >/dev/null
sleep 1.5
"$ad" gesture pan $(point start-handle) -30 -38 900 >/dev/null
sleep 1.5
shot
menu Copy
expect_clipboard "light held over the river

Second paragraph"

begin live-paste
tap line-two-end
"$ad" press 'role="button" label="Paste"' >/dev/null
expect_draft "${two_blocks}light held over the river

Second paragraph"
shot

begin live-maths
open_draft "$maths"
[ "$(draft)" = "$maths" ] || fail "opening the draft changed it"
shot
[ "$(sips -g pixelWidth "$shots/$(printf %02d "$step")-$name.png" | awk '/pixelWidth/ {print $2}')" = 1206 ] || fail "the screenshot is not at 3x"

begin live-review
open_draft "The light held and i dont know wich way teh river went"
tap fix-grammar 3
tap use
tap keep
tap keep
tap keep 1.5
expect_draft "The light held and I don't know wich way teh river went"
shot

# VoiceOver on iPhone: lanes 8 and 9 of TOB-444's L4. gpui-mobile hands UIKit no accessibility
# tree, so the page is read through the text-input bridge, which UIKit reports once the page has
# focus, as it did for the text-area page.
# Prints `<value>\t<focused>\t<height>` of the page's node in the accessibility snapshot.
page_node() {
  "$ad" snapshot --json | python3 -c '
import json, sys
nodes = json.load(sys.stdin)
nodes = nodes.get("data", nodes).get("nodes", nodes)
page = next((n for n in nodes if n.get("role") == "GPUITextInputView"), None)
if page:
    print(json.dumps(page.get("value", "")), page.get("focused", False), page["rect"]["height"], sep="\t")
'
}

begin a11y-ios
open_draft "$two_blocks"
tap line-one-end 1.5
node="$(page_node)"
[ -n "$node" ] || fail "the focused page is not in the accessibility snapshot"
want="$(printf '%s' "$two_blocks" | python3 -c 'import json, sys; print(json.dumps(sys.stdin.read()))')"
[ "$(cut -f1 <<<"$node")" = "$want" ] || fail "the page reads $(cut -f1 <<<"$node"), expected the draft"
[ "$(cut -f2 <<<"$node")" = True ] || fail "the page is not the focused element"
echo "ok   a11y-08 the focused page reads the draft's source"
shot

begin a11y-target
height="$(cut -f3 <<<"$node")"
python3 -c "import sys; sys.exit(0 if float('$height') >= 44 else 1)" || fail "the page's target is $height pt tall"
echo "ok   a11y-09 the page's target is $height pt tall"
shot

# The library and reading (TOB-460). Find books always searches the app's Documents, where the
# fixture books are copied with simctl. The system pickers, for Add and for Search another folder,
# are driven by hand: see README.md.
library_json() {
  echo "$(data_dir)/library/library.json"
}

# Prints `<chapter> <fraction> <opened>` of the book whose title starts with `$1`.
book_state() {
  python3 - "$(library_json)" "$1" <<'PY'
import json, sys
books = json.load(open(sys.argv[1]))["books"] if __import__("os").path.exists(sys.argv[1]) else []
book = next((b for b in books if b["title"].startswith(sys.argv[2])), None)
print(*(book["position"]["chapter"], book["position"]["fraction"], book["opened"]) if book else ("none",))
PY
}

book_count() {
  python3 -c 'import json, os, sys; p = sys.argv[1]; print(len(json.load(open(p))["books"]) if os.path.exists(p) else 0)' "$(library_json)"
}

begin books-find
../books/fetch-fixtures.sh >/dev/null
docs="$(xcrun simctl get_app_container "$udid" "$app" data)/Documents"
mkdir -p "$docs"
cp ../target/fixtures/pride-and-prejudice.epub ../target/fixtures/rfc2616.pdf "$docs/"
reading_draft="The draft from before the books."
printf '%s' "$reading_draft" >"$(data_dir)/draft.md"
# The iPhone has no Appearance panel, so paged mode is set in settings.json.
printf '{"provider":"Claude","reading":{"pages":"paged"}}' >"$(data_dir)/settings.json"
"$ad" open "$app" --relaunch --launch-args --fake-ai >/dev/null
sleep 2.5
tap show-reading 1.5
[ "$(book_count)" = 0 ] || fail "the library is not empty"
tap find-books 4
tap found-pdf
shot

begin books-library
tap add-found 3
for _ in $(seq 25); do [ "$(book_count)" = 2 ] && break; sleep 0.4; done
[ "$(book_count)" = 2 ] || fail "Find books added $(book_count) books, expected 2"
shot

begin books-epub
tap book-epub 3
[ "$(book_state Pride | cut -d' ' -f3)" != 0 ] || fail "the EPUB was not opened"
shot

begin books-page-turned
tap page-forward 1.5
read -r chapter fraction _ <<<"$(book_state Pride)"
[ "$chapter" = 0 ] && python3 -c "import sys; sys.exit(0 if float('$fraction') > 0 else 1)" || fail "turning the page left the place at $chapter $fraction"
shot

begin books-contents
tap contents
shot

begin books-chapter
tap chapter-one 1.5
[ "$(book_state Pride | cut -d' ' -f1-2)" = "1 0.0" ] || fail "Contents did not open Chapter I: $(book_state Pride)"
shot

begin books-write
tap show-writing 1.5
expect_draft "$reading_draft"
shot

begin books-read
tap show-reading 2
[ "$(book_state Pride | cut -d' ' -f1-2)" = "1 0.0" ] || fail "Read did not go back to Chapter I: $(book_state Pride)"
shot

begin books-pdf
tap back-to-library 1.5
tap book-pdf 3
pdf_opened="$(book_state Microsoft | cut -d' ' -f3)"
epub_opened="$(book_state Pride | cut -d' ' -f3)"
[ "$pdf_opened" -gt "$epub_opened" ] || fail "the PDF was not opened after the EPUB"
shot

echo "all $step steps passed; screenshots in $shots"

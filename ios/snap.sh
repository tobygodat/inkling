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
# Every call, keys.sh's and run.sh's too, goes through device.sh, which outlasts the daemon's gaps.
export INKLING_AGENT_DEVICE="$ad"
ad="$(pwd)/device.sh"
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
    add-key) echo "300 84" ;;         # header, "Claude · add a key", left of the magnifier
    fix-grammar) echo "72 752" ;;     # keyboard down
    change-teh) echo "176 182" ;;     # "teh", second line of the review
    keep) echo "106 746" ;;
    use) echo "295 746" ;;
    accept-rest) echo "324 93" ;;
    back) echo "80 105" ;;
    key-field) echo "200 357" ;;
    reveal-key) echo "355 357" ;;
    open-keys) echo "316 84" ;;       # header, "Claude" once a key is saved
    feedback-link) echo "200 690" ;;  # key screen, scrolled to the bottom
    open-feedback) echo "261 84" ;;   # header, the message icon left of the provider
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
    # The library and reading. The cog and the Write and Read toggle sit at the top left.
    cog) echo "26 84" ;;
    show-writing) echo "72 84" ;;
    show-reading) echo "116 84" ;;
    find-books) echo "276 140" ;;
    found-pdf) echo "150 341" ;;      # the second row Find books lists, the PDF
    add-found) echo "201 414" ;;
    # A book's tile, by the start of its title: books added together are listed in the order
    # their imports finish, so the place is read from library.json.
    tile:*) tile "${1#tile:}" ;;
    page-forward) echo "370 450" ;;   # the page's right third
    contents) echo "305 90" ;;        # left of the magnifier
    chapter-one) echo "120 237" ;;    # Contents' second chapter, under the Chapters and Highlights tabs
    back-to-library) echo "192 90" ;;  # "‹ Library", right of the 150 pt corner
    add-one-found) echo "200 343" ;;  # Add, under a single book Find books found
    footnote-marker) echo "190 344" ;; # the raised 1 after "what it had to teach."
    reading-middle) echo "200 650" ;; # the page's middle third, on text
    page-back) echo "30 450" ;;       # the page's left third
    walden-paragraph) echo "200 300" ;; # Walden's first paragraph, "I went to the woods…"
    pill-highlight) echo "178 757" ;; # the pill at the bottom while a passage is selected
    pill-quote) echo "224 757" ;;
    contents-highlights) echo "297 145" ;; # Contents' Highlights tab
    first-highlight) echo "200 206" ;;
    # The Appearance sheet, from its top; Pages once the sheet is panned to its end.
    sheet-done) echo "363 402" ;;
    sync) echo "353 458" ;;
    tone-sepia) echo "127 582" ;;
    pages-scroll) echo "110 450" ;;
    pages-paged) echo "290 450" ;;
    # The documents: "‹ Documents" after the corner, then the list's New and rows, newest first.
    open-documents) echo "200 84" ;;
    new-document) echo "316 84" ;;    # left of the magnifier
    row-one) echo "200 136" ;;
    row-two) echo "200 182" ;;
    undo-remove) echo "201 810" ;;
    # Account: the last row of the Appearance sheet once it is panned to its end, then its rows.
    sheet-account) echo "150 806" ;;
    sign-in) echo "201 451" ;;
    copy-address) echo "328 201" ;;  # Agents, the second row
    try-again) echo "355 145" ;;     # the first row, while sync has stopped
    # Find: the magnifier at the top right and Done beside the field, in a 44 pt header (the page
    # and the documents) or a 56 pt one (the library and a book).
    open-find) echo "372 84" ;;
    find-done) echo "367 84" ;;
    open-find-tall) echo "378 90" ;;
    find-done-tall) echo "367 90" ;;
    # Delete, over a held tile, and Undo under the shelf.
    delete:*) echo "$(tile "${1#delete:}" | cut -d' ' -f1) 249" ;;
    undo-remove-book) echo "201 810" ;;
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

# The document open last: settings.json's `document` while its file is there, else the newest.
open_document() {
  local docs id
  docs="$(data_dir)/documents"
  id="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1])).get("document") or "")' "$(data_dir)/settings.json" 2>/dev/null || true)"
  if [ -n "$id" ] && [ -f "$docs/$id.md" ]; then echo "$docs/$id.md"; else ls -t "$docs"/*.md 2>/dev/null | head -1; fi
}

draft() {
  local file
  file="$(open_document)"
  [ -n "$file" ] && cat "$file" 2>/dev/null || true
}

document_count() {
  ls "$(data_dir)/documents"/*.md 2>/dev/null | wc -l | tr -d ' '
}

# Starts the next launch on one document holding `$1`: the old draft.md, which the app moves in.
seed_draft() {
  rm -rf "$(data_dir)/documents"
  printf '%s' "$1" >"$(data_dir)/draft.md"
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
  seed_draft "$1"
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
# appstate names the Simulator once Inkling is open. Opening another app first to ask would put
# its back link in the status bar of every screenshot.
udid="$("$ad" appstate --json 2>/dev/null | sed -n 's/.*"device_udid": "\(.*\)".*/\1/p' || true)"
[ -n "$udid" ] || fail "no Simulator udid from agent-device"
# The same status bar on every run, so a screenshot changes only when the app does.
xcrun simctl status_bar "$udid" override --time 9:41 --batteryState charged --batteryLevel 100 \
  --wifiBars 3 --cellularMode active --cellularBars 4 --operatorName "" --dataNetwork wifi
trap 'xcrun simctl status_bar "$udid" clear' EXIT

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

# Documents (TOB-484): draft.md moved in, New, switching, a swipe to delete and Undo.
begin docs-list
ferry="# The ferry

It leaves at seven."
open_draft "$ferry"
[ ! -f "$(data_dir)/draft.md" ] && [ "$(draft)" = "$ferry" ] || fail "draft.md was not moved into documents/"
tap open-documents 1.5
shot

begin docs-new
tap new-document 1.5
"$ad" is visible 'role="button" label="Paste"' >/dev/null || fail "New did not put the caret on the page"
./keys.sh "$ad" "the second piece"
expect_draft "The second piece"
[ "$(document_count)" = 2 ] || fail "there are $(document_count) documents, expected 2"
shot

begin docs-switch
tap header-done
tap open-documents 1.5
tap row-two 1.5
expect_draft "$ferry"
shot

begin docs-delete
tap open-documents 1.5
"$ad" gesture pan 300 136 -220 0 400 >/dev/null
sleep 1.5
[ "$(document_count)" = 1 ] || fail "a swipe left $(document_count) documents, expected 1"
shot

begin docs-undo
tap undo-remove 1.5
[ "$(document_count)" = 2 ] || fail "Undo left $(document_count) documents, expected 2"
grep -rqx "The second piece" "$(data_dir)/documents" || fail "Undo did not bring the second piece back whole"
shot

# Find (TOB-489) on the page and the list. The live editor gives UIKit no accessibility tree, so the
# page's matches are checked by what the screen shows: the Search key moves the current one.
begin find-page
tap row-two 1.5
expect_draft "$ferry"
tap open-find
./keys.sh "$ad" "t"
sleep 1
found="$(mktemp -d)"
"$ad" screenshot "$found/first.png" >/dev/null
shot

begin find-page-next
./keys.sh "$ad" "/"
sleep 1
"$ad" screenshot "$found/next.png" >/dev/null
cmp -s "$found/first.png" "$found/next.png" && fail "the Search key did not go to the next match"
shot
tap find-done
"$ad" is visible 'role="button" label="Paste"' >/dev/null && fail "Done left the keyboard up"
expect_draft "$ferry"
rm -rf "$found"

begin find-list
tap open-documents 1.5
tap open-find
./keys.sh "$ad" "ferry"
sleep 1
shot
# Filtered, the first row is the ferry, though the second piece is newer.
tap row-one 1.5
expect_draft "$ferry"

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

# The first two tiles, left to right, for the book whose title starts with `$1`.
tile() {
  python3 - "$(library_json)" "$1" <<'PY'
import json, sys
books = json.load(open(sys.argv[1]))["books"]
at = next(i for i, b in enumerate(books) if b["title"].startswith(sys.argv[2]))
print(*[(90, 296), (274, 296)][at])
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
seed_draft "$reading_draft"
# Paged mode from settings.json, so the lane starts in pages; the Appearance steps below switch it.
printf '{"provider":"Claude","reading":{"pages":"paged"}}' >"$(data_dir)/settings.json"
"$ad" open "$app" --relaunch --launch-args --fake-ai >/dev/null
sleep 2.5
tap show-reading 1.5
[ "$(book_count)" = 0 ] || fail "the library is not empty"
tap find-books 4
shot
# The PDF starts unticked (the shot above, and mac/snap.sh asserts it); tick it.
tap found-pdf

begin books-library
tap add-found 3
for _ in $(seq 25); do [ "$(book_count)" = 2 ] && break; sleep 0.4; done
[ "$(book_count)" = 2 ] || fail "Find books added $(book_count) books, expected 2"
shot

begin books-epub
tap tile:Pride 3
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
tap tile:Microsoft 3
pdf_opened="$(book_state Microsoft | cut -d' ' -f3)"
epub_opened="$(book_state Pride | cut -d' ' -f3)"
[ "$pdf_opened" -gt "$epub_opened" ] || fail "the PDF was not opened after the EPUB"
shot

# Appearance on iPhone: the sheet beside the page and a book, a reading-only tone with sync off,
# sync carrying it across, and the place through Paged, Scroll and Paged.
setting() {
  python3 -c 'import json, sys; s = json.load(open(sys.argv[1])); print(eval(sys.argv[2], {}, {"s": s}))' "$(data_dir)/settings.json" "$1"
}

begin appearance-page
tap show-writing 1.5
tap cog 1.5
shot
tap sheet-done

begin appearance-book
tap show-reading 2
tap cog 1.5
shot

begin appearance-tone
tap sync
tap tone-sepia 1.5
[ "$(setting "s['reading']['look']['mode'], s['reading']['synced'], s['appearance'].get('mode', 'light')")" = "('sepia', False, 'light')" ] || fail "with sync off Sepia changed $(setting 's')"
shot

begin appearance-sync
tap sync 1.5
[ "$(setting "s['appearance']['mode']")" = sepia ] || fail "turning sync on did not carry Sepia to writing"
shot

begin appearance-pages
tap sheet-done
tap page-forward 1.5
held="$(book_state Microsoft | cut -d' ' -f1-2)"
tap cog 1.5
# Past the sheet's end, so where it stops doesn't hang on the pan's speed. x = 392 is right of the
# sliders.
"$ad" gesture pan 392 800 0 -700 1200 >/dev/null
sleep 1.5
tap pages-scroll 1.5
[ "$(setting "s['reading']['pages']")" = scroll ] || fail "Scroll did not switch the book to scrolling"
tap pages-paged 1.5
tap sheet-done
tap back-to-library 1.5
[ "$(book_state Microsoft | cut -d' ' -f1-2)" = "$held" ] || fail "the place moved from $held to $(book_state Microsoft) through Scroll and back"
shot

# A book with italics, a picture with its caption and a footnote (books/examples/walden.rs).
begin books-walden
cp ../target/fixtures/walden.epub "$docs/"
tap find-books 4
tap add-one-found 3
for _ in $(seq 25); do [ "$(book_count)" = 3 ] && break; sleep 0.4; done
[ "$(book_count)" = 3 ] || fail "Find books did not add Walden"
tap tile:Walden 3
[ "$(book_state Walden | cut -d' ' -f3)" != 0 ] || fail "Walden was not opened"
shot

begin books-footnote
plain="$(mktemp -d)"
"$ad" screenshot "$plain/before.png" >/dev/null
tap footnote-marker 1.5
"$ad" screenshot "$plain/note.png" >/dev/null
cmp -s "$plain/before.png" "$plain/note.png" && fail "a tap on the footnote marker showed nothing"
[ "$(book_state Walden | cut -d' ' -f1-2)" = "0 0.0" ] || fail "the tap on the marker turned the page: $(book_state Walden)"
shot
tap reading-middle 1.5
[ "$(book_state Walden | cut -d' ' -f1-2)" = "0 0.0" ] || fail "the tap that closes the note turned the page: $(book_state Walden)"
rm -rf "$plain"

# Highlights and quotes: holding a paragraph selects it and shows the pill at the bottom.
highlights() { cat "$(data_dir)/library/highlights/"*.json 2>/dev/null || true; }
hold() {
  # shellcheck disable=SC2046
  "$ad" longpress $(point "$1") 900 >/dev/null
  sleep 1.5
}
walden_paragraph="I went to the woods because I wished to live deliberately, to front only the essential facts of life, and see if I could not learn what it had to teach."

begin books-hold
hold walden-paragraph
shot

begin books-highlight
tap pill-highlight
highlights | grep -qF "\"words\": \"$walden_paragraph" || fail "Highlight kept nothing: $(highlights)"
tap reading-middle 1.5
[ "$(book_state Walden | cut -d' ' -f1-2)" = "0 0.0" ] || fail "the tap that clears the selection turned the page: $(book_state Walden)"
shot
tap page-forward 1.5
[ "$(book_state Walden | cut -d' ' -f1)" = 0 ] && python3 -c "import sys; sys.exit(0 if float('$(book_state Walden | cut -d' ' -f2)') > 0 else 1)" || fail "after a highlight a tap no longer turns the page: $(book_state Walden)"
tap page-back 1.5

begin books-remove
hold walden-paragraph
tap pill-highlight
highlights | grep -qF "$walden_paragraph" && fail "pressing the highlighter again kept the highlight: $(highlights)"
tap reading-middle 1.5
shot
hold walden-paragraph
tap pill-highlight
tap reading-middle 1.5

begin books-highlights-tab
tap page-forward 1.5
tap page-forward 1.5
[ "$(book_state Walden | cut -d' ' -f1)" = 1 ] || fail "two turns did not reach the next chapter: $(book_state Walden)"
tap contents
tap contents-highlights 1.2
shot
tap first-highlight 1.5
[ "$(book_state Walden | cut -d' ' -f1)" = 0 ] || fail "choosing the highlight did not go to it: $(book_state Walden)"

begin books-quote
hold walden-paragraph
tap pill-quote 1.5
quoted="$(printf '%s\n\n> %s\n>\n> — Henry David Thoreau, *Walden*\n\n' "$reading_draft" "${walden_paragraph/deliberately/*deliberately*}")"
expect_draft "$quoted"
[ "$(book_state Walden | cut -d' ' -f1)" = 0 ] || fail "quoting left the book: $(book_state Walden)"
shot
tap show-writing 1.5
"$ad" screenshot "$shots/$(printf %02d "$step")-$name-page.png" --pixel-density 3 >/dev/null

# Find in a book, across its chapters, and in the library; then a book deleted and brought back.
begin find-book
tap show-reading 2
tap open-find-tall
./keys.sh "$ad" "woods"
sleep 1
shot

begin find-book-next
./keys.sh "$ad" "/"
sleep 1.5
shot
tap find-done-tall 1.5
[ "$(book_state Walden | cut -d' ' -f1)" = 1 ] || fail "the Search key and Done did not leave Walden at the match in the next chapter: $(book_state Walden)"

begin find-library
tap back-to-library 1.5
tap open-find-tall
./keys.sh "$ad" "pride"
sleep 1
shot
before="$(book_state Pride | cut -d' ' -f3)"
# Filtered, Pride and Prejudice is the first tile, though Walden is newer.
"$ad" press 90 296 >/dev/null
sleep 3
[ "$(book_state Pride | cut -d' ' -f3)" -gt "$before" ] || fail "the first tile under \"pride\" did not open Pride and Prejudice"

stored() {
  find "$(data_dir)/library" -path '*/.removed' -prune -o -name "$1*" -print | wc -l | tr -d ' '
}

begin delete-book
tap back-to-library 1.5
walden_id="$(python3 -c 'import json, sys; print(next(b["id"] for b in json.load(open(sys.argv[1]))["books"] if b["title"] == "Walden"))' "$(library_json)")"
files="$(stored "$walden_id")"
marked="$(data_dir)/library/highlights/$walden_id.json"
[ "$files" -ge 4 ] && grep -q '"words"' "$marked" || fail "Walden has $files files, expected its book, original, images and the highlight made above"
before_marks="$(cat "$marked")"
# shellcheck disable=SC2046
"$ad" longpress $(point tile:Walden) 800 >/dev/null
sleep 1.5
shot

begin book-deleted
tap delete:Walden 1.5
[ "$(book_count)" = 2 ] && [ "$(stored "$walden_id")" = 0 ] && [ ! -f "$marked" ] || fail "Delete left $(book_count) books and $(stored "$walden_id") of Walden's files, its highlights included"
shot

begin undo-book
tap undo-remove-book 1.5
[ "$(book_count)" = 3 ] && [ "$(stored "$walden_id")" = "$files" ] || fail "Undo left $(book_count) books and $(stored "$walden_id") of Walden's $files files"
[ "$(cat "$marked" 2>/dev/null)" = "$before_marks" ] || fail "Undo did not bring Walden's highlights back"
[ "$(book_state Walden | cut -d' ' -f1)" = 1 ] || fail "Walden came back without its place: $(book_state Walden)"
shot

# Account and sync (TOB-490), against the fake server in fake-server.json: no Google, no network.
fake_server() {
  echo "$(data_dir)/fake-server.json"
}

# Runs `$1` as Python on the fake server's state `s`, saving it back.
server_edit() {
  python3 - "$(fake_server)" "$1" <<'PY'
import json, os, sys
path = sys.argv[1]
s = json.load(open(path)) if os.path.exists(path) else {}
s.setdefault("documents", {})
exec(sys.argv[2])
json.dump(s, open(path, "w"))
PY
}

begin account-signed-out
account_draft="# On keeping accounts

Every list I keep grows until it is the work itself."
rm -f "$(fake_server)" "$(data_dir)/sync.json"
server_edit 's["documents"]["1700000000000"] = {"text": "# Letter to the board\n\nWritten on the Mac.", "saved_at": 1700000000000, "device": "mac-a1b2", "version": 1, "deleted": False}'
printf '{"provider":"Claude"}' >"$(data_dir)/settings.json"
open_draft "$account_draft"
tap cog 1.5
"$ad" gesture pan 200 700 0 -600 500 >/dev/null
sleep 1
"$ad" gesture pan 200 700 0 -600 500 >/dev/null
sleep 1
tap sheet-account 1.5
shot

begin account-signed-in
tap sign-in 3
for _ in $(seq 20); do grep -q "Every list I keep" "$(fake_server)" && break; sleep 0.5; done
grep -q "Every list I keep" "$(fake_server)" || fail "signing in did not send the document up"
grep -rq "Written on the Mac." "$(data_dir)/documents" || fail "the Mac's document did not come down"
tap copy-address
[ "$("$ad" clipboard read)" = "https://inkling.invalid/mcp" ] || fail "Copy address copied $("$ad" clipboard read)"
shot

begin account-sync-stopped
server_edit 's["down"] = True'
"$ad" home >/dev/null
sleep 1
"$ad" open "$app" >/dev/null
sleep 3
# While it is down, the Mac saves again; Try again must bring that down.
server_edit 'd = s["documents"]["1700000000000"]; d.update(text="# Letter to the board\n\nWritten again on the Mac.", version=d["version"] + 1)'
shot

begin account-try-again
server_edit 's["down"] = False'
tap try-again 3
grep -rq "Written again on the Mac." "$(data_dir)/documents" || fail "Try again did not sync"
shot

echo "all $step steps passed; screenshots in $shots"

# Inkling for iPhone

The iPhone app. A Swift UIKit host (`app/`) mounts the shared views (`inkling_ui::Inkling` with
`FormFactor::Phone`) from a Rust static library (`inkling-ios`, `src/lib.rs`), on GPUI Kit through
a patched `longbridge/gpui-mobile`. This folder is its own Cargo workspace.

Proof is in `docs/screenshots/ios/`, from the iPhone 18 Pro Simulator, iOS 27.2: the scripted run
on 2026-11-12, the hand-driven shots on 2026-11-11.

## Run

```
ios/run.sh <agent-device> --fake-ai
```

`run.sh` fetches and patches gpui-mobile, builds the static library, generates the Xcode project
with xcodegen, builds the Simulator app, then installs and launches it through the `agent-device`
command the T3 device tools return (`device_open`). With no `agent-device` it only builds and prints
the app's path.

Launch flags work in Debug builds only; Release ignores them:

- `--fake-ai` sets `INKLING_FAKE_AI=1`: canned answers, keys in memory. No provider is called and
  the Keychain is never touched.
- `--memory-keys` sets `INKLING_MEMORY_KEYS=1`: keys in memory and the real providers. Use it for
  the missing-key path. Asking without a key opens the key screen and calls nothing.
- `--probe-typing` sets `INKLING_PROBE_TYPING=1`: the app logs at Debug, which includes the time
  from each typed character to its painted frame. `bench.sh` reads it.

With neither flag the app reads keys from the Keychain through `inkling_core::default_keys()`.

`testflight.sh [build number]` archives a Release build for a real iPhone and uploads it to
TestFlight. The build number defaults to the UTC time (`20261111.2220`), so each upload is higher
than the last. Release builds read keys from the Keychain and call the real providers.
Before uploading it checks every framework symbol the binary imports against the public iPhoneOS
SDK headers, because App Store Connect rejects private API use (ITMS-90338). Build 20261111.2229
was rejected for `CGContextSetFontSmoothingStyle`, which core-graphics links through an unused
wrapper. Fat LTO in the release profile drops it. Xcode's signed-in account signs the build, because only
an account may use the cloud-managed distribution certificate; an API key gets `Cloud signing
permission error`. `altool` then uploads it with the App Store Connect API key stored in the login
Keychain under `inkling-app-store-connect` (`key-id`, `issuer-id` and `p8`, the .p8 file in base64).

`snap.sh <agent-device>` (or `AGENT_DEVICE=<agent-device> snap.sh`) is the scripted proof; see
[The scripted run](#the-scripted-run).

`keys.sh <agent-device> "<text>"` types on the English on-screen keyboard by tapping keys (lowercase
letters, space, `<` for delete, `/` for Return, and `#` when a space follows it).

`bench.sh <agent-device>` types 50 characters into a 5,000-word draft in the installed Debug build
and prints the median and the worst time from a typed character to its painted frame. On
2026-11-12 the live page measured a 14.7 ms median over three runs, and the text area it
replaced 13.6 ms. Both are Debug builds on the Simulator, so they are a floor for a real iPhone.

## What the host does

- The data folder is `Application Support/Inkling` inside the app's sandbox. Swift passes it to
  `inkling_ios_register` before GPUI starts.
- The GPUI view sits between the top safe area and `keyboardLayoutGuide`, so the page, the quick
  actions and the ask field stay above the keyboard, and above the home indicator when it is down.
- Source Serif 4 and Inter come from `inkling-ui`. GPUI Kit's icons come from `gpui-kit-assets`,
  registered with `gpui_mobile::ios::ffi::set_asset_source` (patch 0003).
- A `CADisplayLink` drives frames.

## Patches to gpui-mobile

gpui-mobile is pinned to `7548ebc4b24f00494af9976af8852c7163ded9ce`. `fetch-mobile.sh` clones it into
`target/gpui-mobile`, discards local edits there and applies `patches/*.patch` in order.

- `0001-gpui-pre-0.3.12.patch` moves it to the GPUI that GPUI Kit 0.9.1 is built on.
- `0002-uitextinput-bridge.patch` is the text-input bridge below.
- `0003-assets-and-return-key.patch` adds `set_asset_source`, so the kit's icons load, and sends
  the keyboard's Return as GPUI's Enter key. Before it, Return arrived as `insertText:"\n"`, which a
  single-line kit `Input` drops, so Return in the ask field never sent and Return in the key field
  never saved.
- `0004-paste-control.patch` puts a system `UIPasteControl` above the keyboard, so text copied in
  another app pastes. See [Paste from another app](#paste-from-another-app).
- `0005-system-selection-ui.patch` makes UIKit's own caret, handles and edit menu work over a GPUI
  input. See [The page's selection](#the-pages-selection).
- `0006-clipboard-write.patch` fixes Copy. gpui-mobile handed the pasteboard a Rust string as a C
  string, with no NUL at its end, so a copy longer than a few words read past it and cleared the
  pasteboard.
- `0007-typing-probe.patch` logs, at Debug, the time from the keyboard's `insertText:` to the frame
  that shows it.

To change gpui-mobile, edit `target/gpui-mobile` in place, run `patches/refresh.sh` (it rewrites
the last patch from the working tree, so a change to an earlier patch needs the later ones taken
out first), then build. `run.sh` re-fetches, so an edit not yet captured
by `refresh.sh` is lost.

## The text-input bridge

Before TOB-430, gpui-mobile typed through a hidden 1x1 `UITextView` that it cleared after each
commit, so iOS never saw the document. Autocorrect and auto-capitalisation were forced off, and a
tapped suggestion replaced a range in the empty scratch buffer, so only the trailing space arrived.

The bridge replaces that view with `GPUITextInputView`, a `UIView` that conforms to `UIKeyInput`,
`UITextInputTraits` and `UITextInput` and answers every call from GPUI's `PlatformInputHandler`,
the same handler GPUI uses for `NSTextInputClient` on the Mac. Positions are UTF-16 offsets into the
GPUI document (`GPUITextPosition`, `GPUITextRange`). It lives in
`src/ios/window/text_input_view.rs` in gpui-mobile.

| UIKit asks | The bridge answers with |
| --- | --- |
| `textInRange:`, `beginningOfDocument`, `endOfDocument` | `text_for_range`, `text_length_utf16` (or a clamped `text_for_range`) |
| `selectedTextRange`, `setSelectedTextRange:` | `selected_text_range`, `set_selected_text_range` |
| `markedTextRange`, `setMarkedText:selectedRange:`, `unmarkText` | `marked_text_range`, `replace_and_mark_text_in_range`, `unmark_text` |
| `insertText:`, `replaceRange:withText:` | `replace_text_in_range` |
| `deleteBackward` | GPUI's Backspace key, or a delete inside the marked range |
| `firstRectForRange:`, `caretRectForPosition:`, `selectionRectsForRange:` | `bounds_for_range`, `selection_bounds_for_range`, `element_bounds` |
| `closestPositionToPoint:`, `characterRangeAtPoint:` | `character_index_for_point` |
| `autocorrectionType`, `autocapitalizationType`, `spellCheckingType`, `returnKeyType` | the focused input's `TextInputConfiguration`, which GPUI pushes through `set_text_input_configuration` |

Three rules keep the keyboard and the document in step:

- UIKit has to hear about changes it did not make: a tap that moves the caret, a paste from the
  kit's menu, focus moving to another input. After each drawn frame the bridge compares the
  document (focus, length, selection, marked range) with what UIKit last saw and calls the input
  delegate's `textWillChange`/`textDidChange` or `selectionWillChange`/`selectionDidChange`.
- Edits UIKit makes itself record what UIKit now sees, so they never echo back. An echo resets
  the keyboard and drops its pending correction.
- Told that the selection moved, UIKit collapses it to its end from inside that same call. The
  bridge ignores selection writes UIKit makes while it is being notified. Without this, a word
  the kit selected by double tap or long press collapsed to a caret.

GPUI calls `set_text_input_configuration`, `show_soft_keyboard` and `hide_soft_keyboard` from inside
its own update, where the input handler cannot run. UIKit reads the document back while it
reloads, so `becomeFirstResponder`, `resignFirstResponder` and `reloadInputViews` run on the next
run-loop turn.

The app opts in per input. The page asks for autocorrection, suggestions and sentence capitals in
`text_input_configuration` (`ui/src/live/input.rs`), and the feedback field with the kit's
`.autocorrect(true).autocapitalize(Autocapitalize::Sentences)`. The kit's default is off.

Inline predictions (iOS's grey ghost text) are off (`inlinePredictionType = No`). UIKit would send
them as marked text, which GPUI would draw as a real composition.

## The page's selection

The page is the live editor, not a kit input, so nothing in the kit draws its handles. UIKit
does: `ios/src/lib.rs` calls `gpui_ios_set_system_selection_ui(window, true)` when the page takes
focus and `false` when it loses it, because the kit's inputs still draw their own. Patch 0005 is
what that needed.

- **Where UIKit draws.** UIKit puts its caret, highlight and handles in the view its
  `UITextInteraction` is on, and a GPUI input is only a region of one view. So the interaction
  goes on a canvas the size of the GPUI view, inside a clip view that takes the focused input's
  bounds (`element_bounds`). The canvas keeps GPUI's coordinates, and nothing shows over the
  header when the selection scrolls out of the page.
- **Keeping it in step.** GPUI lays text out when it draws. After each drawn frame the bridge
  calls `setNeedsSelectionUpdate` on UIKit's `UITextSelectionDisplayInteraction`, so the caret
  follows an edit and the handles follow a scroll. It also activates that interaction when it
  attaches, since the view is first responder by then and UIKit would show no caret. After a
  text change GPUI made (a paste), it tells UIKit the selection changed as well as the text.
- **The edit menu.** UIKit looks for each action's target up the responder chain of the view the
  interaction is on. The bridge's text input view is a subview of the GPUI view, so it is not on
  that chain, and the menu had no Cut, Copy or Paste. The GPUI view now answers `targetForAction:withSender:` with the
  text input view, which performs Cut, Copy and Paste as GPUI's `ctrl-x`, `ctrl-c` and `ctrl-v`
  and Select All through the input handler. When the same selection moves (a scroll), the bridge
  dismisses the menu, as a text view does.
- **Whose touch it is.** Both UIKit's recognizers and GPUI see every touch. For a tap UIKit acts
  first and GPUI's tap lands last, in the same event, so the editor places the caret on the
  tapped character and selects the double-tapped word. UIKit would snap a tap to a word boundary
  and would take a short scroll for a tap, so the bridge turns down a caret UIKit sets while the
  finger is making a tap or a pan, and tells UIKit to read the selection again. A finger that
  rests 400 ms is UIKit's long press, and its caret is kept. The editor claims a touch that
  starts on a handle and a long press, so GPUI neither scrolls nor taps under them.

## Paste from another app

A writer copies their API key on the provider's site and pastes it. iOS lets an app read text
another app copied only when the user asked for the paste through system UI: the edit menu's
Paste, a `UIPasteControl`, or a hardware ⌘V. Any other read goes to the `pasted` daemon without an
authentication message (`AuthenticationMessageMissing`), which prompts on a device and, on the
Simulator, fails to show the prompt and refuses the read (`PBErrorDomain Code=13 "Operation not
authorized."`, then `UserRecentlyDeniedItemAccess`, "Paste denied silently"). The kit draws its
own Paste in GPUI and reads `UIPasteboard` from its action, so iOS never sees that tap as consent.

Patch 0004 gives the bridge view a `UIPasteControl` as its `inputAccessoryView`. It shows above
the keyboard for every GPUI input: the page, the ask field and the key field. Its tap calls the
view's `pasteItemProviders:`, and the bridge runs the focused input's own Paste action right
there (`ctrl-v`, the kit's binding off the Mac). That read carries the control's consent
(`AuthMessageAuthentic`, "Paste allowed silently"), and the kit's rules still apply: a
single-line input drops newlines, a read-only one ignores the paste.

The page also has Paste in UIKit's edit menu now (see [The page's selection](#the-pages-selection)).
The control stays, because the ask field, the key fields and the feedback field are kit inputs
with the kit's own menu, and because it is one tap with no selection to make first.

## Taps in the review

GPUI's `InteractiveText::on_click` never fires on a touch screen. GPUI's gesture recognizer
dispatches a tap's mouse down and mouse up in one call (`gpui-pre` `window.rs`,
`dispatch_recognized_touch_gesture`), and `InteractiveText` only registers its mouse-up listener on
the paint after it sees the mouse down. So `ui/src/review.rs` gives the review an ordinary
`on_click` and hit-tests the change ranges with `TextLayout::index_for_position`.

## Proved on the Simulator

| Behaviour | Shot |
| --- | --- |
| Empty page with its placeholder, header with the way to the key screen | `01-empty` |
| Typing with autocorrect ("teh", "becuase") and auto-capitalisation ("i", "she") | `02-typed` |
| Done puts the keyboard away; actions and ask field stay | `03-keyboard-down` |
| Ask field above the keyboard | `04-ask-typed` |
| Asking: version bar with progress and Cancel, keyboard down | `05-asking` |
| Review: version bar, changes inline, sheet on the first change | `06-review` |
| Next steps to the following change | `07-next` |
| Tapping a change selects it in the sheet | `08-change-tapped` |
| Use accepts it and the sheet moves to the next pending change | `09-used` |
| Keep rejects it and the sheet moves on | `10-kept` |
| Accept the rest applies the rest and returns to the draft | `11-accept-rest` |
| Double tap selects a word (the text-area page, with the kit's handles; the live page's are in `snap/21-live-select`) | `12-word-selected` |
| Find a word with the keyboard up | `13-find-a-word` |
| Other wordings in the sheet, 44 pt rows | `14-other-words` |
| Choosing a wording, then Use | `15-wording-chosen`, `16-wording-used` |
| A pure insertion offers "Leave it out" | `17-fix-grammar` |
| Nothing to change | `18-nothing-to-change` |
| Key screen | `19-keys` |
| Typing a key, Return saves it and puts the keyboard away | `20-key-typed`, `21-key-saved` |
| Back on the page, the header names the provider | `22-back-with-key` |
| Return in the page makes a new line | `23-return-newline` |
| Return in the ask field sends; a long change truncates in the sheet | `24-return-sends` |
| The draft survives a relaunch | `25-no-key-page` |
| Asking with no key opens the key screen | `26-missing-key` |
| A key copied in Safari's address bar (fake) | `28-copy-in-safari` |
| The system Paste control above the keyboard on the key screen | `29-key-field-paste-control` |
| Paste puts the key copied in Safari into the key field, shown revealed | `30-key-pasted-from-safari` |
| The same Paste works in the page | `31-page-pasted-from-safari` |

Driven by hand with `agent-device` and `keys.sh`: every row above, on the text-area page the
live editor replaced. The scripted run below proves the same page behaviour on the live editor.

## The scripted run

`snap.sh <agent-device>` uninstalls the app, builds and installs it through `run.sh` with
`--fake-ai`, drives the main flow through `agent-device` and saves each step to
`docs/screenshots/ios/snap/`. It stops with a non-zero exit at the first failed check. A run takes
about three and a half minutes, the build included.

| Step | Checked | Screenshot only |
| --- | --- | --- |
| Launch on an empty page | the saved draft is empty | `01-empty` |
| Type a line on the keyboard, paste a second part with the Paste control | the saved draft is exactly the typed and pasted text | `02-written` |
| Done | | `03-keyboard-down` |
| Fix grammar | | `04-review` |
| Tap the "teh" change, Use it, Keep the next one | | `05-change-tapped`, `06-used`, `07-kept` |
| Accept the rest | the saved draft has the used and accepted changes and not the kept one | `08-accepted` |
| Open the key screen | | `09-keys` |
| Paste a fake key with the Paste control, reveal it | | `10-key-pasted` |
| Return saves | `settings.json` appears only now | `11-key-saved` |
| Back to the page | | `12-back-with-key` |
| Scroll the key screen, paste a fake feedback link, Return saves it | no file in the data folder holds `#k=` | `13-link-saved` |
| Back, then Feedback in the header | | `14-feedback-open` |
| Type "bigger margins", Done, Send | `sent-feedback.json` holds "Bigger margins" and not the draft | `15-feedback-sent` |
| Save a link on an `.invalid` host, send "a darker caret" | the failed send adds nothing to `sent-feedback.json` | `16-feedback-failed` |
| Relaunch on an empty draft, tap the page, type a line | the saved draft is the line, and the Paste control shows, so the keyboard is up | `17-live-typed` |
| Type `# title`, Return twice, a line | the saved draft is the heading, a blank line and the line; the heading is drawn large (by eye) | `18-live-heading` |
| Type "becuase" and a space | the saved draft says "because" | `19-live-autocorrect` |
| Type "rive", tap "river" in the suggestion bar | the saved draft ends "river " | `20-live-suggestion` |
| Relaunch on two blocks, double tap "light" | Copy in UIKit's menu puts "light" on the clipboard; the handles show (by eye) | `21-live-select` |
| Double tap "paragraph" in the second block, drag its start handle into the first | Copy puts the source from "light" to "paragraph" on the clipboard, blank line included | `22-live-handles` |
| Tap the end of the second block, Paste | the saved draft is the two blocks and then the copied source | `23-live-paste` |
| Relaunch on a draft with a heading, an inline formula and a block formula | the draft is unchanged and the shot is 1206 px wide (3x); both formulas are drawn and sharp (by eye) | `24-live-maths` |
| Relaunch on a misspelt line, Fix grammar, Use the first change, Keep the other three | the saved draft holds that one change | `25-live-review` |

After every tap it also checks that Inkling is still the foreground app, so a crash fails the
step. The pasted part carries the misspellings the fake Fix grammar corrects (`i`, `dont`,
`wich`, `teh`), because the keyboard's autocorrect would fix them while typing. The script's
clipboard is set with `agent-device clipboard write`, which iOS treats as another app's copy.
GPUI draws everything inside one view, so the accessibility tree has no handles for the page,
the chips or the review; those taps are coordinates, all in `point()` at the top of the script.
The Paste control, UIKit's edit menu and the suggestion bar are pressed by their labels. The live
steps open their drafts by writing `draft.md` and relaunching, because the keyboard has no `$` or
`*` within `keys.sh`'s reach.

Still driven by hand on the live page: a long press and its loupe, a drag on the end handle, Cut,
Select All, a tap inside the selection (it brings the menu back), scrolling with a selection, and
the two switches (set in `settings.json`, since the iPhone has no Appearance panel).

Done and declined feedback and Restart are proved by `mac/snap.sh`, which also renders the
phone layout of the list (`19-iphone-feedback`). The iPhone lists its items as waiting until a new
iPhone build carries their records.

`cargo-ios.sh` sets `INKLING_BUILD` to the commit count, so the feedback body names the build.


## Gaps

- **The switches have no control on iPhone.** The page draws by `markdown` and `maths` in
  `settings.json`, but the Appearance panel that changes them is on the Mac only.
- **The end handle under the edit menu.** UIKit shows the menu just under the selection. A drag
  that starts on the end handle's knob there can stop after a few points: the touch began inside
  the menu's own window, UIKit closes the menu when a drag begins, and iOS then starts the touch
  again. A drag on the start handle, or on the end handle once the menu is closed, goes through.
  It needs a real iPhone to say how often a finger hits it.
- **No scroll while a handle is dragged.** Dragging a handle to the page's edge does not scroll
  the page. Lift, scroll, and drag again.
- **The menu after Select All.** UIKit closes its menu when the selection changes and does not
  open it for the new one. A tap inside the selection opens it.
- **Marked text and dictation on the live page** are untested. The Simulator's English keyboard
  makes neither. The Mac proof covers marked text in the same editor.
- **The kit's own Paste.** The Paste in the kit's long-press menu, in the ask field, the key
  fields and the feedback field, still reads the pasteboard without consent, so it pastes only
  what Inkling copied. Text from another app goes through the Paste control above the keyboard.
  Removing the kit's item needs a kit option.
- **The masked key is readable by accessibility.** The bridge answers UIKit's `textInRange:` with
  the document's real text, and the accessibility tree reads it back, so the key field's value
  is visible there (and to VoiceOver) while the field shows dots. The bridge needs to know a field
  is masked (`isSecureTextEntry`), which `TextInputConfiguration` does not carry yet.
- **The feedback link field is under the keyboard while it has focus.** The key screen does not
  scroll a focused field into view, so the link is pasted with the Paste control above the
  keyboard without the field in sight. Return saves it and puts the keyboard away.
- **The Paste control shows for every input**, also when the clipboard holds nothing to paste,
  where iOS dims it. It takes 52 pt above the keyboard.
- **No Ideas action.** The phone reference shows a fourth quick action, Ideas. The core has no
  such ask yet, so the phone shows the Mac's three.
- **Keyboard state.** The views know focus, not the keyboard. Done shows while the page or the ask
  field has focus. The page is not focused at launch, so the keyboard comes up on the first tap.
- **Loupe.** A long press moves the page's caret to the finger. Whether UIKit shows its loupe
  over it is unproved: the Simulator's capture may wait for the hold to end.
- **Text changes of the same length** (for example accepting a same-length AI change) do not tell
  the keyboard that the text changed, only that the selection did, because the bridge compares
  length, selection and marked range rather than content.
- **Dictation** cannot be tested on the Simulator. From the code: dictation goes through
  `insertText:` and `replaceRange:withText:` against the real document, plus UIKit's dictation
  placeholder calls, which the view does not implement (`insertDictationResultPlaceholder`,
  `removeDictationResultPlaceholder:willInsertResult:`). UIKit treats those as optional, so
  dictated text should land, but without the placeholder while it is being recognised. That needs
  a device to confirm.
- **Hardware keyboards** still go through gpui-mobile's existing key-event path, unchanged.


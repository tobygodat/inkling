# Inkling for iPhone

The iPhone app. A Swift UIKit host (`app/`) mounts the shared views (`inkling_ui::Inkling` with
`FormFactor::Phone`) from a Rust static library (`inkling-ios`, `src/lib.rs`), on GPUI Kit through
a patched `longbridge/gpui-mobile`. This folder is its own Cargo workspace.

Proof is in `docs/screenshots/ios/`, from the iPhone 18 Pro Simulator, iOS 27.2, on 2026-11-11.

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

With neither flag the app reads keys from the Keychain through `inkling_core::default_keys()`.

`snap.sh <agent-device>` (or `AGENT_DEVICE=<agent-device> snap.sh`) is the scripted proof; see
[The scripted run](#the-scripted-run).

`keys.sh <agent-device> "<text>"` types on the English on-screen keyboard by tapping keys (lowercase
letters, space, `<` for delete, `/` for Return).

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

To change gpui-mobile, edit `target/gpui-mobile` in place, run `patches/refresh.sh` (it rewrites
the last patch from the working tree), then build. `run.sh` re-fetches, so an edit not yet captured
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

The app opts in per input. `ui/src/app.rs` builds the page with
`.autocorrect(true).autocapitalize(Autocapitalize::Sentences)`; the kit's default is off.

`gpui_ios_set_system_selection_ui(window, enabled)` attaches UIKit's own `UITextInteraction` to the
GPUI view, so UIKit draws its handles and edit menu. Inkling leaves it off; see Gaps.

Inline predictions (iOS's grey ghost text) are off (`inlinePredictionType = No`). UIKit would send
them as marked text, which GPUI would draw as a real composition.

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

Why not UIKit's edit menu: attaching `UITextInteraction` and implementing `paste:` and
`canPerformAction:withSender:` was tried again. UIKit asks about `paste:` only once the view
answers `isEditable` (iOS 18), and still left Paste out of the caret menu, while a stock
`UITextField` in the same app offered it for the same clipboard. `UIPasteboard.hasStrings` also
answers NO for text another app copied. The same experiment crashed the app when UIKit's own
cursor got an infinite caret rect (`CALayerInvalidGeometry`). Handing selection to UIKit is a
larger change than paste needs, so it is not kept.

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
| Double tap selects a word, kit handles and menu | `12-word-selected` |
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

Driven by hand with `agent-device` and `keys.sh`: every row above.

## The scripted run

`snap.sh <agent-device>` uninstalls the app, builds and installs it through `run.sh` with
`--fake-ai`, drives the main flow through `agent-device` and saves each step to
`docs/screenshots/ios/snap/`. It stops with a non-zero exit at the first failed check. A run takes
about a minute.

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

After every tap it also checks that Inkling is still the foreground app, so a crash fails the
step. The pasted part carries the misspellings the fake Fix grammar corrects (`i`, `dont`,
`wich`, `teh`), because the keyboard's autocorrect would fix them while typing. The script's
clipboard is set with `agent-device clipboard write`, which iOS treats as another app's copy.
GPUI draws everything inside one view, so the accessibility tree has no handles for the page,
the chips or the review; those taps are coordinates, all in `point()` at the top of the script.
The Paste control is pressed by its label.


## Gaps

- **The kit's own Paste.** The Paste in the kit's long-press menu still reads the pasteboard
  without consent, so it pastes only what Inkling copied. Text from another app goes through the
  Paste control above the keyboard. Removing the kit's item needs a kit option.
- **The masked key is readable by accessibility.** The bridge answers UIKit's `textInRange:` with
  the document's real text, and the accessibility tree reads it back, so the key field's value
  is visible there (and to VoiceOver) while the field shows dots. The bridge needs to know a field
  is masked (`isSecureTextEntry`), which `TextInputConfiguration` does not carry yet.
- **The Paste control shows for every input**, also when the clipboard holds nothing to paste,
  where iOS dims it. It takes 52 pt above the keyboard.
- **No Ideas action.** The phone reference shows a fourth quick action, Ideas. The core has no
  such ask yet, so the phone shows the Mac's three.
- **Keyboard state.** The views know focus, not the keyboard. Done shows while the page or the ask
  field has focus. The page is not focused at launch, so the keyboard comes up on the first tap.
- **Marked-text underline after scrolling.** Once the text area has scrolled, a composition has no
  underline (log: marked `51..53`, nothing drawn). The bridge reports the range correctly. The bug
  is in gpui-base 0.9.1 `src/input/base/element.rs` `prepaint`: on the plain-text path it builds
  the runs over the whole document from offset 0 (`split_run_for_ime_underline(run,
  0..display_text.len(), …)`), while the shaped lines start at the first visible line. Every
  colour in a plain text area is the same, so only the underline shows the misalignment. It is a
  GPUI Kit fix.
- **Loupe.** Not there with the kit's handles. With `UITextInteraction` attached, a held long
  press gave UIKit's handles but no loupe in the screenshot. The capture may wait for the hold to
  end, so this is unproved either way.
- **System selection UI is an experiment.** With `UITextInteraction` attached, UIKit's edit menu
  lacks Cut, Copy and Paste, and implementing them did not bring Paste back (see
  [Paste from another app](#paste-from-another-app)). A long press with it attached crashed the
  app once: UIKit's cursor view got an infinite caret rect. One run also lost auto-capitalisation on the first letter, probably because UIKit's tap
  makes the view first responder before GPUI focuses the input. Both gesture systems see every
  touch (`cancelsTouchesInView = NO`), so GPUI keeps focus and its own taps.
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


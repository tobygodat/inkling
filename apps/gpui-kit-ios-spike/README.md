# GPUI Kit iOS keyboard spike (TOB-428, TOB-430)

A spike that asks whether a GPUI Kit `Textarea` on iPhone can take the system keyboard,
with selection handles and the edit menu, well enough to write in.

The answer after TOB-430 is yes. A text-input bridge in gpui-mobile lets iOS read and edit the
real document. Autocorrect, auto-capitalisation, tapping a suggestion and the marked-text underline
now work. Typing, Backspace, Return, Japanese input, the kit's selection handles and menu, and the
review paragraph still work. The loupe is still missing with the kit's own selection UI.

Run on the iPhone 18 Pro Simulator, iOS 27.2, on 2026-11-11. Proof is in
`docs/screenshots/gpui-kit-ios/`.

## Revisions

| Piece | Revision |
| --- | --- |
| GPUI Kit | `gpui-kit = "=0.9.1"` from crates.io, `component` feature only |
| GPUI | `gpui-pre = "=0.3.12"`, the only `gpui-pre` in `Cargo.lock` |
| Renderer | `gpui-pre-wgpu = "=0.3.12"` |
| Platform | `longbridge/gpui-mobile` at `7548ebc4b24f00494af9976af8852c7163ded9ce` (Nov 3, on 0.3.10), plus `patches/` |

`7548ebc` is still the newest gpui-mobile commit on 2026-11-11. `fetch-mobile.sh` clones it into
`target/gpui-mobile`, discards local edits there and applies `patches/*.patch` in order.

`0001-gpui-pre-0.3.12.patch` (19 changed lines) moves gpui-mobile to the GPUI that GPUI Kit 0.9.1
is built on:

1. Every `=0.3.10` becomes `=0.3.12`.
2. The `offscreen` feature and the `[patch.crates-io]` table go. They need gpui-mobile's vendored
   0.3.10 renderer, which a dependency's patch table cannot reach anyway.
3. `WgpuContext::new` gets the surface config, a new argument in `gpui-pre-wgpu` 0.3.12.
4. `IosWindow` gets `show_soft_keyboard` and `hide_soft_keyboard`. gpui-pre 0.3.12 asks for the
   keyboard through those `PlatformWindow` methods when a text input is tapped. gpui-mobile
   predates them, so the trait's no-op default swallowed the request.

`0002-uitextinput-bridge.patch` is the text-input bridge described below.

To change gpui-mobile, edit `target/gpui-mobile` in place, run `patches/refresh.sh` (it rewrites
the last patch from the working tree), then build. `build.sh` re-fetches, so an edit not yet
captured by `refresh.sh` is lost.

The spike also turns on gpui-mobile's `video_player` and `camera` features, because its iOS
`platform_view.rs` does not compile without them.

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

The app opts in per input. `src/lib.rs` builds the page with
`.autocorrect(true).autocapitalize(Autocapitalize::Sentences)`; the kit's default is off.

`gpui_ios_set_system_selection_ui(window, enabled)` attaches UIKit's own `UITextInteraction` to the
GPUI view, so UIKit draws its handles and edit menu. The spike turns it on with the "Native touch
selection" switch. It is an experiment; see Gaps.

Inline predictions (iOS's grey ghost text) are off (`inlinePredictionType = No`). UIKit would send
them as marked text, which GPUI would draw as a real composition.

## Proved behaviour

Native touch selection OFF unless the row says ON. "Before" is the TOB-428 run without the bridge.
Rune is the result from `apps/rune-ios-spike`.

| Behaviour | Before | After (bridge) | Shot | Rune |
| --- | --- | --- | --- | --- |
| Tap focuses, keyboard appears | Works | Works | `01-keyboard-up` | Works |
| Typing | Works | Works | `02-typed` | Works |
| Auto-capitalisation | Off | Works. Shift is lit on an empty page and after Return; "hello" became "Hello", "second" became "Second" | `01-keyboard-up`, `02-typed`, `07-second-line` | Works |
| Backspace | Works | Works. Eleven deletes took "…arrives tomorrow’s " back to "…arrives" | `06-backspaced` | Works |
| Return | Works | Works | `07-second-line` | Works |
| Text stays above the keyboard | Works | Works. After 18 Returns the caret line sits just above it | `08-caret-above-keyboard` | Works |
| Autocorrect | Off | Works. UIKit highlights "iphone", and Space replaced it with "iPhone" (`replaceRange 9..15`). "teh" became "the" after a paste | `04-predictive-offered`, `03-autocorrect` | Works |
| Predictions offered | Work for the word in progress | Work, including after a space and for a selected word | `04-predictive-offered`, `11-double-tap-off` | Works |
| Tapping a prediction | Broken. Only `" "` reached GPUI | Works. "tomor" became "tomorrow’s " (`replaceRange 24..29`, then a space) | `05-predictive-tapped` | Works |
| Marked text (Japanese Romaji) | Preedit without an underline | Works, with the kit's underline under にほんご | `09-marked-text` | Works |
| Committing a candidate | Works | Works. 日本語 replaced the preedit | `10-marked-committed` | Works |
| Double tap selects a word | Works, kit handles and menu | Works, kit handles and menu | `11-double-tap-off` | Not there |
| Long press selects a word | Works, on release | Works, on release | `15-loupe-hold-off` | Only moves the caret |
| Drag a selection handle | Works | Works. Dragging the start handle extended "line" to "last line" | `12-handle-drag-off` | Not there |
| Copy | Works | Works. The clipboard read back "last line" | `12-handle-drag-off` | Not there |
| Cut | Not run | Works. The kit's Cut removed "very" and the clipboard read "very" | (log only) | Not there |
| Paste | Works | Works, and the keyboard's suggestions follow the pasted text | `13-menu-caret-off`, `14-pasted` | Not there |
| Loupe | Not there | Not there with the kit's selection UI | `15-loupe-hold-off` | Not there |
| Double tap, native selection ON | Selection, no handles, no menu | UIKit's own handles and edit menu (AutoFill, Look Up, Translate, …) | `16-double-tap-on` | n/a |
| Long press, native selection ON | Selection, no handles, no menu | UIKit's own handles. No loupe captured | `17-long-press-on` | n/a |
| Review paragraph: strike and highlight inline | Works | Works | `18-review-pending` | n/a |
| Tap outside the change | Does nothing | Does nothing | `19-review-tap-outside` | n/a |
| Tap the change toggles it | Works | Works, both ways | `20-review-accepted`, `21-review-undone` | n/a |
| Emoji keyboard | Not tested | Not tested | | Works |

Every shot is from the final build, rebuilt from `patches/` by `build.sh`, except the Cut row,
which came from an earlier build of the same bridge.

## Gaps

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
  lacks Cut, Copy and Paste, because the view does not implement `UIResponderStandardEditActions`
  yet. One run also lost auto-capitalisation on the first letter, probably because UIKit's tap
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

## What TOB-428 found without the bridge

The TOB-428 builder predicted from the source that iOS never sees the document. The run before the
bridge settled it:

- Confirmed. Autocorrect is off. Auto-capitalisation is off. There is no loupe and no system edit
  menu with either setting.
- Overturned. Predictions are not weak. The keyboard keeps its own context and offers good
  suggestions for the word in progress. The real failure is that accepting one loses it: UIKit
  replaces a range in the scratch buffer, which gpui-mobile has already cleared, so only the
  trailing space arrives.
- Overturned. With native selection OFF the kit's own handles and menu work well enough to select,
  extend, copy and paste. That is the whole selection story Rune is missing.

### The review paragraph

GPUI's `InteractiveText::on_click` never fires on a touch screen (`22-interactive-text-tap-ignored`,
the first run). GPUI's gesture recognizer dispatches a tap's mouse down and mouse up in one call
(`gpui-pre` `window.rs`, `dispatch_recognized_touch_gesture`). `InteractiveText` only registers its
mouse-up listener on the paint after it sees the mouse down, so the up always finds no listener.
`src/lib.rs` now gives the paragraph an ordinary `on_click` and hit-tests the change's range with
`TextLayout::index_for_position`. That fires on every tap and ignores taps outside the range.

## Run

- `./build.sh` fetches and patches gpui-mobile, builds the Rust static library, generates the
  Xcode project, builds the Simulator app and prints its path.
- Install it and drive it with the T3 device tools (`agent-device install`, `agent-device open`).
- `./keys.sh <agent-device> "<text>"` taps keys on the English on-screen keyboard (copied from the
  Rune spike). The globe key (42, 833) switches between English and Japanese Romaji. The Romaji
  layout's second and third rows sit at different x positions, so `keys.sh` cannot type on it;
  tap those keys by coordinates.
- To read the bridge's `debug!` logs, raise the level in `src/lib.rs` and stream the app's log with
  `xcrun simctl spawn <udid> log stream --predicate 'process == "GpuiKitSpike"'`.

## Size

`0002-uitextinput-bridge.patch` adds 1212 lines and removes 377. `text_input_view.rs` is the new
file, 1158 lines after rustfmt; about a third is the `add_method` table and one-line `extern "C"`
shims. It deletes the 265-line `UITextView` scratch-buffer class from `window.rs` and the trait
setter from `text_input.rs`, and adds 15 lines to `ffi.rs` (the post-frame sync call and
`gpui_ios_set_system_selection_ui`). `src/lib.rs` is 220 lines.

## Layout

- `src/lib.rs` holds the view and the `gpui_ios_register_app` callback. A header row switches
  the kit's `set_native_touch_selection` and gpui-mobile's `gpui_ios_set_system_selection_ui`
  together. Below it, a read-only review paragraph draws a
  struck-through deletion and a highlighted insertion with `StyledText` highlights, and a tap on the
  change toggles it. The rest of the screen is a kit `Textarea`.
- `ios/Spike/App.swift` is the example's UIKit host on the scene lifecycle. It pins the GPUI view's
  bottom to `keyboardLayoutGuide` and drives frames from a `CADisplayLink`.
- `ios/Spike/Spike.h` declares gpui-mobile's C entry points.
- `patches/` holds the gpui-mobile patch set and `refresh.sh`.

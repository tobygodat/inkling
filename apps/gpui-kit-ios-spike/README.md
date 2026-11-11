# GPUI Kit iOS keyboard spike (TOB-428)

A throwaway spike. It asks whether a GPUI Kit `Textarea` on iPhone can take the system keyboard,
with selection handles and the edit menu, well enough to write in.

The answer from the run is partly yes. Typing, Backspace, Return, marked text, double tap and long
press to select, draggable handles, and Copy and Paste all work. The kit draws the handles and the
menu itself. Autocorrect is off, and tapping a prediction loses the word. There is no loupe. The
review paragraph works after one change to how it takes taps.

Run on the iPhone 18 Pro Simulator, iOS 27.2, on 2026-11-11. Proof is in
`docs/screenshots/gpui-kit-ios/`.

## Revisions

| Piece | Revision |
| --- | --- |
| GPUI Kit | `gpui-kit = "=0.9.1"` from crates.io, `component` feature only |
| GPUI | `gpui-pre = "=0.3.12"`, the only `gpui-pre` in `Cargo.lock` |
| Renderer | `gpui-pre-wgpu = "=0.3.12"` |
| Platform | `longbridge/gpui-mobile` at `7548ebc4b24f00494af9976af8852c7163ded9ce` (Nov 3, on 0.3.10), patched by `fetch-mobile.sh` |

`7548ebc` is still the newest gpui-mobile commit on 2026-11-11. `fetch-mobile.sh` clones it into
`target/gpui-mobile` and makes four changes:

1. Every `=0.3.10` becomes `=0.3.12`.
2. The `offscreen` feature and the `[patch.crates-io]` table go. They need gpui-mobile's vendored
   0.3.10 renderer, which a dependency's patch table cannot reach anyway.
3. `WgpuContext::new` gets the surface config, a new argument in `gpui-pre-wgpu` 0.3.12.
4. `IosWindow` gets `show_soft_keyboard` and `hide_soft_keyboard`. gpui-pre 0.3.12 asks for the
   keyboard through those `PlatformWindow` methods when a text input is tapped
   (`Window::request_virtual_keyboard`). gpui-mobile predates them, so the trait's no-op default
   swallowed the request. Before this patch a tap drew a caret and no keyboard appeared.

The spike also turns on gpui-mobile's `video_player` and `camera` features, because its iOS
`platform_view.rs` does not compile without them.

Getting a set that builds cost two compile errors, each fixed in one line. Getting a set that works
cost a third fix that compiled cleanly and only showed up on the device.

## Proved behaviour

Native touch selection OFF unless the row says ON. Rune is the result from `apps/rune-ios-spike`.

| Behaviour | GPUI Kit | Shot | Rune |
| --- | --- | --- | --- |
| Tap focuses, keyboard appears | Works, after patch 4 | `01-keyboard-up` | Works |
| Typing | Works | `02-typed` | Works |
| Auto-capitalisation | Off. Shift is never lit and "hello" stays lowercase | `02-typed` | Works |
| Backspace | Works. Seven deletes took "…iphone tomor " back to "…iphone" | `06-backspaced` | Works |
| Return | Works | `07-second-line` | Works |
| Text stays above the keyboard | Works. After 18 Returns the caret line sits just above it | `08-caret-above-keyboard` | Works |
| Autocorrect | Off. Space left "iphone" as typed | `03-autocorrect` | Works |
| Predictions offered | Work for the word in progress, even across words ("iPhone tomorrow"). None after a space | `04-predictive-offered` | Works |
| Tapping a prediction | Broken. The bar clears and the document is unchanged. The log shows only `" "` reached GPUI | `05-predictive-tapped` | Works |
| Marked text (Japanese Romaji) | Works. にほんご shows as a preedit with candidates, without an underline | `09-marked-text` | Works |
| Committing a candidate | Works. 日本語 replaced the preedit | `10-marked-committed` | Works |
| Double tap selects a word | Works, with kit handles and a kit Cut, Copy, Paste, Select All menu | `11-double-tap-off` | Not there |
| Long press selects a word | Works, with handles and menu, on release | (same as above) | Only moves the caret |
| Drag a selection handle | Works. Dragging the start handle extended "line" to "last line" | `12-handle-drag-off` | Not there |
| Copy | Works. The system clipboard read back "last line" | `12-handle-drag-off` | Not there |
| Paste | Works. Long press at the caret shows Paste and Select All, and Paste inserted the text | `13-menu-caret-off`, `14-pasted` | Not there |
| Loupe | Not there. Nothing is drawn while a long press is held | `15-loupe-hold-off` | Not there |
| Double tap, native selection ON | Selects the word. No handles, no menu | `16-double-tap-on` | n/a |
| Long press, native selection ON | Selects the word. No handles, no menu, no loupe | `17-long-press-on` | n/a |
| Review paragraph: strike and highlight inline | Works | `18-review-pending` | n/a |
| Tap outside the change | Does nothing, as it should | `19-review-tap-outside` | n/a |
| Tap the change toggles it | Works, both ways, after the change below | `20-review-accepted`, `21-review-undone` | n/a |
| Emoji keyboard | Not tested | | Works |

`set_native_touch_selection(true)` is strictly worse. It hands handles and the menu to a platform
layer that gpui-mobile does not provide, so the user gets a selection and no way to act on it.

### What the prediction got right and wrong

The earlier builder predicted from the source that iOS never sees the document.

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
- `xcrun simctl install <udid> <app>`, then drive it with the T3 device tools.
- `./keys.sh <agent-device> "<text>"` taps keys on the English on-screen keyboard (copied from the
  Rune spike). The keyboard opens on Japanese Romaji, so tap the globe key (42, 833) first.

## Size

`src/lib.rs` is 208 lines, of which about 120 are the screen and the review paragraph. The rest is
logging and the C entry point. `ios/Spike/App.swift` is 104 lines and `Spike.h` is 11. The scripts
and `project.yml` are 127 more.

## Layout

- `src/lib.rs` holds the view and the `gpui_ios_register_app` callback. A header row switches
  `set_native_touch_selection` on and off. Below it, a read-only review paragraph draws a
  struck-through deletion and a highlighted insertion with `StyledText` highlights, and a tap on the
  change toggles it. The rest of the screen is a kit `Textarea`.
- `ios/Spike/App.swift` is the example's UIKit host on the scene lifecycle. It pins the GPUI view's
  bottom to `keyboardLayoutGuide` and drives frames from a `CADisplayLink`.
- `ios/Spike/Spike.h` declares gpui-mobile's C entry points.

# Rune iOS keyboard spike (TOB-428)

A throwaway spike. It answers one question: can a Rune view on iPhone take the system keyboard
well enough to write in? The answer is yes for typing. Selection and the editing chrome around it
are not there yet.

Rune rev `cd36f5a9917ec4a65786caf69213c9cc32919984`, the same rev the reader app uses. Rune `dev`
at `e9c851c1` (2026-11-11, 364 commits later) has no iOS or text-input work that changes this. It
only turns IME composition into a private state machine (`a6a59e69`), which makes `TextEdit::preedit`
a method and would break this bridge's direct read of it.

## Run

- `./build.sh` builds the Rust static library, generates the Xcode project, builds the Simulator app
  and prints its path.
- `xcrun simctl install <udid> <app>`, then drive the app with the T3 device tools.
- `./keys.sh <agent-device> "<text>"` taps keys on the on-screen QWERTY keyboard (iPhone 18 Pro,
  portrait). `agent-device type` sends hardware key events instead: it hides the software keyboard
  and dropped characters in this run.

## Layout

- `src/lib.rs` holds the Rune view (one `text_input` in a scroll view) and the C ABI.
- `ios/Spike/Spike.h` is that ABI. `ios/Spike/SpikeView.swift` hosts the `CAMetalLayer` and
  implements `UITextInput`. `ios/Spike/App.swift` pins the view's bottom to `keyboardLayoutGuide`.

## How the bridge works

All text, selection and marked text live in Rune's `TextEdit`. Swift keeps no copy. UIKit uses
UTF-16 offsets into a document that includes the marked text. Rune keeps the preedit beside the
text, so `doc_view` in `src/lib.rs` splices it in and converts both ways. Each UIKit call queues
Rune's own input (`Runtime::key`, `Runtime::ime`) and then runs `Runtime::frame(0.0)`, so the
change is applied before UIKit reads the text back. When Rune changes the text or caret by itself,
for example after a tap, the view tells `inputDelegate`.

## Proof

The proof is in `docs/screenshots/rune-ios/`, captured on iPhone 18 Pro with iOS 27.2 in the Simulator.

| Shot | What it shows |
| --- | --- |
| `01-keyboard-up.png` | A tap focuses the field, the view becomes first responder, the keyboard and predictive bar appear, and Rune draws the caret. |
| `02-typed.png` | "Hello from rune on iphone" typed by tapping keys. Auto-capitalisation made the "H". |
| `05-autocorrect.png` | Space autocorrected "iphone" to "iPhone" through `replace(_:withText:)`. |
| `06-predictive.png` | Tapping the "and" suggestion inserted it. |
| `03-backspaced.png` | Six backspaces took the text back to "…on iPhon". |
| `04-second-line.png` | Return started a second line, and the keyboard capitalised its first letter. |
| `07-caret-above-keyboard.png` | After 18 returns the caret line sits just above the keyboard. |
| `08-emoji.png` | Emoji from the emoji keyboard, drawn in colour. |
| `09-marked-text.png` | The Japanese Romaji keyboard's にほんご, drawn as an underlined preedit, with candidates. |
| `10-marked-committed.png` | Picking 日本語 committed it in place of the preedit. |
| `11-long-press.png` | A long press only moves the caret. There is no loupe, selection or menu. |

# Inkling on Rune (prototype)

The writing slice from `docs/prototype-slice.md`, built on [Rune](https://github.com/anomalyco/rune)
for the Mac. Throwaway. Linear issue TOB-428.

Rune is pinned to `dev` at `cd36f5a9917ec4a65786caf69213c9cc32919984`, the rev Toby's reader app
uses. The 363 later `dev` commits add no editor feature this slice needs (no inline-completion or
virtual-text support), and this rev was already in the local cargo cache.

## Layout

- `app` is the whole UI as a library, with no Mac-only code. Its host contract is two calls:
  `configure(paths)` once, then `spec()` for the `AppSpec` to run. An iPhone shell would pass its own
  `Paths` and host the same view.
- `mac` is the Mac binary, `inkling`.
- `scripts/gen.py` writes the scripted-input files `snap.sh` plays.

## Commands

Run `export PATH=/opt/homebrew/opt/rustup/bin:$PATH` first.

- `cargo run -p inkling-mac` opens the app. It reads and writes `~/Library/Application Support/Inkling`.
- `./snap.sh` renders the seven proof screenshots into `docs/screenshots/rune/`, headless.
- `./verify.sh` drives the headless app through a paste, a mouse drag and a relaunch, and checks the
  element tree.
- `cargo test -p inkling-app` runs the app's tests on Rune's test runtime: marked text, Esc, typing
  over a suggestion, accept and undo, rewrite, and the clipboard.
- One scene by hand:
  `INKLING_FAKE_AI=1 INKLING_DATA_DIR=$(mktemp -d) target/debug/inkling --script <file> --snap out.png --at mark:end`.
  Add `--tree tree.json` for the element tree.

Headless runs and runs with `INKLING_FAKE_AI` keep keys in memory, never in the Keychain. Rune's
headless runtime never touches the system clipboard.

## How the suggestion is drawn

Rune's editor (`text_input`) draws only its own buffer. Its spans recolour text that is already
there, and it has no way to show text that isn't. So while a suggestion is open, the editor stays
mounted and focused but draws its glyphs clear, and a plain text element with the same style sits
in its place showing the draft with the suggestion spliced in. The editor still draws the caret and
still gets every key first, through `on_input_key`. The draft itself never holds the suggestion.

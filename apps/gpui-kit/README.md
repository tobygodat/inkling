# Inkling on GPUI Kit (TOB-428 prototype)

Throwaway. The writing slice from `docs/prototype-slice.md` as a Mac app on GPUI Kit `=0.9.1`, which
pulls GPUI as `gpui-pre 0.3.12`, `gpui-base 0.9.1` and `gpui-component 0.9.1`.

## Run

```sh
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo run --manifest-path apps/gpui-kit/Cargo.toml --release
```

The app opens a normal window and takes focus, so run it yourself, not from a headless agent.
Without a key, Cmd-J opens settings. `INKLING_FAKE_AI=1` streams canned text instead of calling a model.

Keys: Cmd-J continues at the cursor, Cmd-K rewrites the selection (or continues with none),
Cmd-Comma opens settings. Tab accepts a proposal, Esc dismisses it, typing dismisses ghost text,
Return also accepts a rewrite.

## Snap

```sh
apps/gpui-kit/snap.sh
```

It builds with the `snap` feature (`gpui-kit/test-support`) and runs `inkling --snap <dir>` twice:
once with `INKLING_FAKE_AI=1` for `01` to `06` and once without it for `07`, each with
`INKLING_DATA_DIR` in a temp folder. The driver (`src/snap.rs`) opens the real view in GPUI's
`HeadlessAppContext` with `show: false, focus: false`, renders with the offscreen Metal renderer,
dispatches keys and text inside the app through `gpui_kit::test::TestWindowExt`, and writes PNGs
with `capture_screenshot`. No window appears and nothing reaches the system event tap.

The driver also asserts, and prints `ok` per line: typing, autosave after 500 ms, streaming without
touching the document, the overlay matching the editor pixel for pixel above the cursor line, Tab
accept, Cmd-Z and Shift-Cmd-Z, typing dismissing ghost text, Backspace, Cmd-C, the rewrite range,
Esc, a relaunch showing the saved draft, and the missing key opening settings on Claude.
`05a-command-bar.png` is an extra shot of the command bar before Return.

## How it is built

- The page is the kit's `Textarea` (`TextareaState`, soft wrap, no chrome) in a 680 pt column.
- Proposals are drawn by the app. The kit's own ghost text lives only in the code `Editor`, is fed by
  an LSP `CompletionProvider` on a typing debounce, has no public setter and shapes one unwrapped
  line. So while `Assist` holds a proposal, an opaque `StyledText` covers the textarea's text box at
  its own origin and width: the text before, the ghost (muted) or the dimmed selection and its
  replacement (accent), then the rest. The document is untouched until `Assist::accept`.
- Tab, Esc and Return reach the app through `cx.intercept_keystrokes`, which runs before any key
  binding, so the textarea's Tab (indent) and Esc never see them while a proposal is open.
- Accept selects the replaced range and calls `TextareaState::replace`, so it is one undo step.
- The command bar and settings use the kit's `Input` (masked key with its reveal toggle, model field
  with the provider's default as placeholder), `RadioGroup` and `Button`.

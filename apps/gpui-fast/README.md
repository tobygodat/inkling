# Inkling on bare gpui-fast

The writing slice from `docs/prototype-slice.md` on gpui-fast 0.1.14, with no component library.
Throwaway: it exists to compare stacks.

## Run

```sh
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo run --manifest-path apps/gpui-fast/Cargo.toml --bin inkling
```

This opens a real, focused window and uses `default_keys()`. Run it yourself, not from an agent.

## Snap

```sh
apps/gpui-fast/snap.sh
```

It writes the seven images to `docs/screenshots/gpui-fast/` and prints one `ok` line per check.
It runs the `snap` binary (feature `snap`) twice: once with `INKLING_FAKE_AI=1` for images 01 to 06,
and once without it for `07-missing-key.png`. Both runs use a temp `INKLING_DATA_DIR` and in-memory keys.

The snap binary uses gpui-fast's `HeadlessAppContext`: GPUI's test platform with the real CoreText
text system and Metal rendering to a texture (`Window::render_to_image`). No window reaches the
window server, so nothing takes focus. Input is dispatched inside the app with
`Window::dispatch_keystroke` and `Window::dispatch_event`. The checks cover typing, autosave timing,
Continue, Rewrite, settings, undo and redo, the clipboard, mouse selection, marked text and a relaunch.

`INKLING_SMOKE=1 target/debug/inkling` opens the real Mac window unfocused on the rightmost display,
prints `first frame drawn` and quits. Run it with a virtual display up (the eval bundle's
`harness/vdisplay.m`) so the window lands off Toby's screens.

## Layout

- `src/editor.rs` is the multi-line editor: text, selection, undo, keys, mouse, input method and
  wrapped painting. It also serves the one-line command bar and settings fields.
- `src/app.rs` is the window: the AI flow on `inkling_core::Assist`, autosave, settings and the footer.
- `src/bin/snap.rs` is the headless proof.

# Inkling

A minimal writing app for Mac and iPhone, in Rust, with bring-your-own-key AI (Claude, Gemini, ChatGPT).
"Inkling" is a working name.

The UI is GPUI Kit on both devices (see `docs/adr/0001-gpui-kit-on-mac-and-iphone.md`) and the design
is in `DESIGN.md`. The Mac app and the iPhone app mount the same views (`ui/`).

## Run the Mac app

```
export PATH=/opt/homebrew/opt/rustup/bin:$PATH && cargo run --release -p inkling-mac
```

Press Cmd-Comma, pick Claude, Gemini or ChatGPT, paste your API key and Save. The key goes to the
Keychain (service `com.tobygodat.inkling`), never to a file. The draft lives in
`~/Library/Application Support/Inkling/draft.md`.

- Write in the page. It saves itself half a second after you stop.
- Fix grammar, Find a word (select a word first), Next line, or type an ask and press Send.
  Esc cancels while it is asking.
- In the version, tick or cross each change, or pick "n more" for other words. Tab or Return
  accepts the next change. "Accept the rest" takes all that are left. Esc or "Back to draft" keeps
  only what you accepted. Cmd-Z undoes each applied change.

`INKLING_FAKE_AI=1` answers with canned text instead of calling a model.

## Run the iPhone app

```
ios/run.sh <agent-device> --fake-ai
```

It builds for the iPhone Simulator, installs and launches the app; see `ios/README.md`. The header
on the page opens the key screen, and Done puts the keyboard away. In a version, tap a change, then
Keep or Use, or Accept the rest.

## Check

```
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
mac/snap.sh
```

`mac/snap.sh` renders the app offscreen with the fake model and keys in memory, drives it with
in-app input, asserts as it goes, and writes `docs/screenshots/mac/`. No window opens.

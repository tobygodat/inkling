# Inkling

Work is tracked in Linear project Inkling, team TOB. Repo: https://github.com/tobygodat/inkling
Research and plans that don't belong in the repo are on the Notion page `eudaimonia/projects/inkling`:
https://app.notion.com/p/415935c51fac81acaefac497b9010370

## Read order

1. This file.
2. The Linear issue.
3. `PRODUCT.md`, `DESIGN.md` and `docs/ARCHITECTURE.md`. `docs/adr/` holds the decisions and why.

## Layout

- The root `Cargo.toml` is a workspace of `core`, `ui` and `mac`.
- `core/` is `inkling-core`: the requests, the providers, the review model, settings and key storage.
  No UI framework may be a dependency of it.
- `ui/` is `inkling-ui`: every view, shared by the Mac app and the iPhone host. It takes its form
  factor as a value and never reads the platform.
- `mac/` is the Mac app, on GPUI Kit's `gpui-fast` feature.
- `ios/` is the iPhone app: a Swift UIKit host (`ios/app/`) over the static library `inkling-ios`,
  in its own Cargo workspace, with gpui-mobile patched from `ios/patches/`. The prototypes are at
  tag `prototypes-tob-428`.
- `docs/screenshots/<app>/` holds each app's proof.

## Commands

Homebrew's rustup keeps its proxies off `PATH`. Run `export PATH=/opt/homebrew/opt/rustup/bin:$PATH` first.
`rust-toolchain.toml` pins Rust 1.99.1, which Rune needs.

- `cargo test --workspace` must pass before any PR.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` must pass before any PR.
- `mac/snap.sh` rebuilds `docs/screenshots/mac/` headless and asserts the app's behaviour. Any change
  under `ui/` or `mac/` reruns it.
- `cargo run --release -p inkling-mac` opens the Mac app. It takes focus, so only Toby runs it.
- `ios/run.sh <agent-device> --fake-ai` builds the iPhone app, installs it on the Simulator and
  launches it with the fake model and keys in memory. Get `<agent-device>` from the T3 device tools'
  `device_open`. Any change under `ui/` or `ios/` reruns the iPhone proof by hand (`ios/README.md`).
- `INKLING_KEY=<key> cargo run -p inkling-core --example ask -- <claude|gemini|chatgpt> "<instruction>" "<draft>"`
  streams one live answer and prints its changes. It calls a paid API, so only Toby runs it.

## Rules

- Never take focus or move the pointer on Toby's Mac. Prove UI headless: an offscreen render, a test
  context, or a window opened unfocused on a virtual display. Never post input to the system event tap.
  Boot a Simulator without opening Simulator.app.
- Headless runs set `INKLING_FAKE_AI=1` and `INKLING_DATA_DIR` to a temp folder. They never read or
  write the real Keychain and never call a paid API.
- API keys live only in the Keychain. Never write one to a file, a log, a screenshot or a commit.
- On the Simulator, use the T3 device tools and call `device_close` when done. A session left open
  blocks the next agent.

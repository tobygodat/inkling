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
`rust-toolchain.toml` pins Rust 1.99.1.

- `cargo test --workspace` must pass before any PR.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` must pass before any PR.
- `mac/snap.sh` rebuilds `docs/screenshots/mac/` headless and asserts the app's behaviour. Any change
  under `ui/` or `mac/` reruns it.
- `cargo run --release -p inkling-mac` opens the Mac app. It takes focus, so only Toby runs it.
- `mac/install.sh` builds `Inkling.app` in release, signs it with the Developer ID identity and
  installs it in `/Applications` (or the folder it is given) without opening it.
- `ios/run.sh <agent-device> --fake-ai` builds the iPhone app, installs it on the Simulator and
  launches it with the fake model and keys in memory. Get `<agent-device>` from the T3 device tools'
  `device_open`.
- `ios/snap.sh <agent-device>` reinstalls the iPhone app and drives the main flow on the Simulator,
  with 12 checked steps. Any change under `ui/` or `ios/` reruns it. `ios/README.md` lists what is
  still driven by hand.
- `ios/testflight.sh` archives the iPhone app in Release for a real device, signs it for the App
  Store (team Q6NAWA42Y2), refuses any build that imports a symbol missing from the public SDK
  headers, and uploads it to TestFlight. Xcode's signed-in account signs it, and `altool` uploads it
  with the App Store Connect API key in the login Keychain (service `inkling-app-store-connect`). It
  sends a build to Apple, so run it only when Toby asks for a TestFlight build.
- `swift icon/make-icon.swift .` redraws the placeholder app icons: the iPhone one into
  `ios/app/Inkling/Assets.xcassets` and `icon/Inkling.icns` for the Mac.
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

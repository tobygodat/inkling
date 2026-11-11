# Inkling

Work is tracked in Linear project Inkling, team TOB. The current issue is TOB-428.
Research and plans that don't belong in the repo are on the Notion page `eudaimonia/projects/inkling`:
https://app.notion.com/p/415935c51fac81acaefac497b9010370

## Read order

1. This file.
2. The Linear issue.
3. `docs/prototype-slice.md` for what every prototype must do and how it is proved.

## Layout

- `core/` is `inkling-core`: the AI request, the streaming proposal and its accept or reject, settings,
  key storage and the three providers. No UI framework may be a dependency of it.
- `apps/<stack>/` is one prototype per UI stack. Each is its own Cargo workspace with its own lockfile
  and depends on `core` by path. They share no code with each other.
- `docs/screenshots/<stack>/` holds each prototype's proof.

## Commands

Homebrew's rustup keeps its proxies off `PATH`. Run `export PATH=/opt/homebrew/opt/rustup/bin:$PATH` first.
`rust-toolchain.toml` pins Rust 1.99.1, which Rune needs.

- `cargo test --manifest-path core/Cargo.toml` must pass before any PR.
- `cargo clippy --manifest-path core/Cargo.toml -- -D warnings` must pass before any PR.
- Each `apps/<stack>/snap.sh` rebuilds that prototype's screenshots headless.
- `INKLING_KEY=<key> cargo run --manifest-path core/Cargo.toml --example ask -- <claude|gemini|chatgpt> "<instruction>" "<text>"`
  streams one live answer. It calls a paid API, so only Toby runs it.

## Rules

- Never take focus or move the pointer on Toby's Mac. Prove UI headless: an offscreen render, a test
  context, or a window opened unfocused on a virtual display. Never post input to the system event tap.
  Boot a Simulator without opening Simulator.app.
- Headless runs set `INKLING_FAKE_AI=1` and `INKLING_DATA_DIR` to a temp folder. They never read or
  write the real Keychain and never call a paid API.
- API keys live only in the Keychain. Never write one to a file, a log, a screenshot or a commit.
- The prototypes are throwaway. Don't polish them past what `docs/prototype-slice.md` asks.

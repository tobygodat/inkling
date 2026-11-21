# Inkling

Work is tracked in Linear project Inkling, team TOB. Repo: https://github.com/tobygodat/inkling
Research and plans that don't belong in the repo are on the Notion page `eudaimonia/projects/inkling`:
https://app.notion.com/p/415935c51fac81acaefac497b9010370

## Read order

1. This file.
2. The Linear issue.
3. `PRODUCT.md`, `DESIGN.md` and `docs/ARCHITECTURE.md`. `docs/adr/` holds the decisions and why.

## Layout

- The root `Cargo.toml` is a workspace of `core`, `books`, `mcp`, `sync`, `ui` and `mac`.
- `core/` is `inkling-core`: the requests, the providers, the review model, settings and key storage.
  No UI framework may be a dependency of it.
- `books/` is `inkling-books`: EPUBs and text PDFs parsed into one `Book`, the library on disk and
  the folder search. No UI framework may be a dependency of it. Its real-file tests read books that
  `books/fetch-fixtures.sh` downloads into `target/fixtures`, so run it once before `cargo test`.
- `mcp/` is `inkling-mcp`: the MCP server an agent reads the library and the draft through
  (ADR 0004). `inkling-mcp --stdio` serves the Mac's data folder and never writes to it. No UI
  framework may be a dependency of it.
- `sync/` is `inkling-sync`: signing a device in to the server and keeping its documents, books and
  reading positions in step with it (ADR 0004, TOB-490). Every headless run talks to its fake
  server, kept in `fake-server.json` in the data folder, never to the real one. No UI framework
  may be a dependency of it.
- `server/` is `inkling-server`, the Cloudflare Worker of ADR 0004: OAuth with Sign in with Google,
  the remote MCP endpoint, the devices' sync API, the Readwise inbox and the iPhone builds' signed
  install links (TOB-518). TypeScript on Bun, not in
  the Cargo workspace. `server/README.md` has its setup.
- `ui/` is `inkling-ui`: every view, shared by the Mac app and the iPhone host. It takes its form
  factor as a value and never reads the platform.
- `mac/` is the Mac app, on GPUI Kit's `gpui-fast` feature.
- `ios/` is the iPhone app: a Swift UIKit host (`ios/app/`) over the static library `inkling-ios`,
  in its own Cargo workspace, with gpui-mobile patched from `ios/patches/`. The prototypes are at
  tag `prototypes-tob-428`.
- `feedback/` holds one record per feedback sent from inside the app, `ship.sh`, which ships one, and
  `AGENT.md`, the instructions for the thread that handles one.
- `docs/screenshots/<app>/` holds each app's proof.
- The old Rune reader (TOB-402) was ported into `books/` and `ui/` (TOB-460, ADR 0003) and its
  `reader/` folder deleted. Its commits are still in this repo's history, and `tobygodat/ios-reader`
  keeps the original, including the `reader-sync` Worker's source.

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
- `ios/snap.sh <agent-device> [step…]` reinstalls the iPhone app and drives the main flow and the
  live page on the Simulator. Any change under `ui/` or `ios/` reruns it. It writes its screenshots
  to `target/snap/ios/` and copies into `docs/screenshots/ios/snap/` only new steps and the steps
  you name, so name the steps whose screen your change alters and commit only those.
  `ios/README.md` lists what is still driven by hand.
- `ios/bench.sh <agent-device>` types 50 characters into a 5,000-word draft in the installed Debug
  build and prints the median and worst time from a typed character to its painted frame.
- `ios/testflight.sh` archives the iPhone app in Release for a real device, signs it for the App
  Store (team Q6NAWA42Y2), refuses any build that imports a symbol missing from the public SDK
  headers, and uploads it to TestFlight. Xcode's signed-in account signs it, and `altool` uploads it
  with the App Store Connect API key in the login Keychain (service `inkling-app-store-connect`). It
  sends a build to Apple, so run it only when Toby asks for a TestFlight build.
- `ios/ota.sh` archives the iPhone app in Release for a real device, exports it ad hoc
  (`release-testing`) for the iPhones registered on team Q6NAWA42Y2, uploads it to `inkling-server`
  and prints an install link that works for 24 hours. It sends nothing to Apple, so any thread
  working on Toby's Mac may run it without asking, and hands Toby the link. The upload token is
  read from the login Keychain (service `inkling-ota`, account `upload-token`). It needs the
  one-time setup in `ios/README.md` (R2 bucket, upload token, registered UDID) and the Worker
  deployed with its `/builds` routes. Feedback threads don't use it.
- `feedback/ship.sh done|declined <id> "<one line>"` ships one feedback from the standing checkout:
  tests, clippy, `mac/snap.sh`, the record, a commit on `main`, `mac/install.sh` and a push. Only a
  feedback thread runs it.
- `cargo install --path mcp --locked` installs `inkling-mcp` in `~/.cargo/bin`, and
  `claude mcp add inkling -- ~/.cargo/bin/inkling-mcp --stdio` connects Claude Code to it. Claude
  Desktop takes the same command in `claude_desktop_config.json`.
- `cd server && bun install && bun run lint && bun run check && bun run test` lints the Worker with
  oxlint and anti-slop, checks its types and runs its tests in workerd. Any change under `server/`
  runs them.
- `cd server && bun run deploy` deploys the Worker and applies its migrations. Only Toby runs it.
- `swift icon/make-icon.swift .` redraws the placeholder app icons: the iPhone one into
  `ios/app/Inkling/Assets.xcassets` and `icon/Inkling.icns` for the Mac.
- `INKLING_KEY=<key> cargo run -p inkling-core --example ask -- <claude|gemini|chatgpt> "<instruction>" "<draft>"`
  streams one live answer and prints its changes. It calls a paid API, so only Toby runs it.

## Tests

- Test behaviour that can break: the review model, the provider protocols, parsing a model's answer,
  key handling and redaction, persistence, and the logic a bug fix changed.
- Leave out tests for what the types already guarantee, trivial logic, copy text, and input no caller
  can produce. An edge case outside the issue's "done when" becomes a line on the issue, not a test.
- Extend before you add. When an existing test or property covers the behaviour, add a case or an
  assertion there.
- Test at one layer: the lowest one that fails without the change. In `core`, prefer one property
  over a list of hand-written cases.
- `ui/`, `mac/` and `ios/` have no unit tests. Their snap script is the proof, and a visual change
  needs nothing more.
- A bug in logic, data or parsing gets a failing test first, then the fix.

## Review

- CI runs clippy and `cargo test --workspace` on every PR and on `main`, and the Worker's lint, types and
  tests. Run what you changed before you push.
  Nobody runs them again by hand after that.
- The final check runs once per PR. It waits for CI to be green on the final head, then runs
  `mac/snap.sh` when `core/`, `ui/` or `mac/` changed and `ios/snap.sh` when `ui/` or `ios/` changed.
  It does not repeat the tests or clippy.
- A PR that changes only docs or CI config needs green CI and no final check.
- A final check fails only on a "Done when" line that breaks in a journey a user would take, on
  lost data, or on a security hole. Any other finding is a note in a `PASS+NOTES` and a line on the
  issue. That includes input no writer produces, such as an unclosed code fence.
- One `ios/snap.sh` run is enough for a check. A run that stops on the Simulator or the machine
  before it reaches the PR's code is run again, and is never reported as a `FAIL`.
- After a `FAIL`, check the fix against the step that failed, not the whole proof. The re-check
  runs the repro that failed and does not look for new inputs.
- A PR gets one fix round. After a second `FAIL`, the thread that started the author files the
  finding on the issue and merges, unless it is one of the three kinds above.
- The thread that started the author fixes a finding of one or two files itself, a doc line
  included, and does not send it back to the author.
- A snap step goes by its name, never a number, so a new step renames nothing.
- No review panel on an ordinary diff. `interrogate` is for a contested design.

## Feedback threads

- A thread that a T3 webhook starts with feedback from inside the app follows `feedback/AGENT.md`.
- It works in the standing checkout on `main` and ships with `feedback/ship.sh`. It opens no Linear
  issue, no worktree, no PR and no final check, and it installs before CI runs.
- It never runs `ios/testflight.sh`.
- Every other thread follows the rest of this file.

## Rules

- Never take focus or move the pointer on Toby's Mac. Prove UI headless: an offscreen render, a test
  context, or a window opened unfocused on a virtual display. Never post input to the system event tap.
  Boot a Simulator without opening Simulator.app.
- Headless runs set `INKLING_FAKE_AI=1` and `INKLING_DATA_DIR` to a temp folder. They never read or
  write the real Keychain and never call a paid API.
- API keys live only in the Keychain. Never write one to a file, a log, a screenshot or a commit.
- On the Simulator, use the T3 device tools and call `device_close` when done. A session left open
  blocks the next agent.

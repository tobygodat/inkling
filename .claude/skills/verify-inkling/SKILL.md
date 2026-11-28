---
name: verify-inkling
description: Drive Inkling the way a user does and keep the proof - the native iPhone app on this run's own Simulator, the native Mac app rendered offscreen, and the Worker (server/) run locally with wrangler. Use to prove a change to apple/, ffi/ or server/ before a PR or in a final check, or to reproduce a bug in tabs and the Ask bar, sign-in and sync, the OTA install path, or the Inbox and reader.
---

# Verify Inkling

`scripts/verify` composes the repo's own proof: `apple/snap.sh` and its XCUITest and offscreen
journeys in `apple/Snapshots`, the Worker's workerd tests, and `apple/ota.sh`. Every run uses the fake
model (`INKLING_FAKE_AI=1`), a temp data folder and the sync crate's fake server. It never reads the
Keychain, never reaches the production Worker and sends nothing to Apple. Nothing takes focus on the Mac.

```sh
v=.claude/skills/verify-inkling/scripts/verify
```

## Launch

`$v launch` builds InklingCore (`apple/core.sh`), installs the Worker's packages, starts
`wrangler dev --local` on a free port with throwaway `SECRET_KEY` and upload token and local D1, KV
and R2, then creates and boots a Simulator named `verify-inkling-<run>` with `xcrun simctl`, never
Simulator.app. It is ready when it prints `launched run <run>`. In a fresh checkout the first launch
takes about 8 minutes, most of it `apple/core.sh`; later ones under a minute. One run per checkout: a second launch refuses while
`target/verify/current` exists. If launch fails, run cleanup before trying again.

## Doctor

`$v doctor` is read-only. Each line is `ok` or `FAIL`: the Simulator is this run's and booted, the
Worker's process is up and holds the port, the Worker's issuer is `http://127.0.0.1:<port>` and not
production, and `ffi`, `core`, `sync` and `books` are unchanged since launch built InklingCore. Run
it first and whenever anything looks off. A changed crate needs cleanup and a new launch.

## Drive

| Command | What it runs |
| --- | --- |
| `$v iphone <test>…` | `apple/snap.sh iphone <this run's udid> <test>…` (XCUITest, both appearances) |
| `$v mac <test>…` | `apple/snap.sh mac <test>…` (offscreen, Swift Testing) |
| `$v worker <file>…` | `bun run test <file>…` in `server/` (workerd, only Google and Readwise faked) |
| `$v ota` | `apple/ota.sh` against the local Worker, then the link chain it prints |

Name tests as `apple/snap.sh` does: iPhone ones as `testAskBarAnswersWithACitationAcrossTabs`, Mac
ones with parentheses, as `signInSyncInboxFeedsAndReadwise()`. With no names, each surface runs all
its journeys. [`features/README.md`](features/README.md) maps each feature to its journeys and what
proves it.

## Evidence

Everything goes to `target/verify/<run>/evidence/`: `iphone/*.png` and `mac/*.png` named by journey
step, `iphone.log`, `mac.log` and `worker-tests.log` with each assertion that failed, and `ota/`
with the build log, the install page, manifest and headers, `checks.txt` and `install-page.png` from
the Simulator's Safari. A proof is a journey that passed plus the screenshots of the steps it names.
Look at the screenshots: a journey asserts behaviour, not how a screen looks.

## Cleanup

`$v cleanup` stops the Worker by the process group launch made, shuts down and deletes this run's
Simulator (only if its name is `verify-inkling-<run>`), and removes the scratch folder. It prints where
the evidence is kept. Run it after every run, failed ones included. Never touch another Simulator, and
never stop a process this run didn't start.

## Keep the map current

When a change adds, renames or removes a journey or a user-facing flow, update its file in
`features/` in the same PR.

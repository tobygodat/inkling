# 0002. Feedback from inside the app, shipped by an agent on the Mac

Status: Accepted, 2026-11-11

## Context

Toby wants to type what should change inside Inkling, have an agent on his Mac implement it and
install the next build within minutes, and see in the app what happened to each thing he sent
(TOB-435). Five facts cross four places: the id, the send time, the build number, the records and
the link's secret, across the app on two devices, two Cargo workspaces, the repo and the installed
bundle. `core` has no UI dependency, `ui/` never reads the platform, headless runs never touch the
network or the Keychain, and the body must never carry draft text (PRODUCT.md).

## Decision

- **One owner.** `core::feedback::Desk` holds all feedback state, the way `Revision` holds AI
  state: the sent log, the send in flight (`Delivery`) and its retry id, and the running `Build`.
  The UI renders it and keeps no copy. Status (waiting, done, declined) is derived, never stored.
- **The id carries the send time**: `<unix seconds>-<8 base32 chars>`. It is the one source for the
  body's `sent_at`, the list and `seconds_to_ship`. A retry of the same text keeps the id; changed
  text gets a new one.
- **One signed HTTPS POST** to a T3 webhook. The link is `https://<webhook>#k=<secret>`, pasted
  once per device into the Keychain. The secret leaves only as `x-inkling-signature:
  sha256=<hex HMAC-SHA256>`. Plain http, redirects, and the URL or secret in an error are refused.
  HMAC and the random source come from `ring`, already in both lockfiles through rustls.
- **Records compile in.** `feedback/ship.sh` writes `feedback/<id>.json` through
  `core/examples/feedback-record.rs`. `core/build.rs` validates every record with the parser the
  app uses and compiles them into `inkling-core`, so a bad record fails the build and both
  workspaces get the records with no step of their own.
- **One build number**, the commit count, compiled in and written to CFBundleVersion by
  `mac/install.sh`. The Mac offers Restart only for a newer installed build; the writer presses it.
  The agent never relaunches the app.
- **One ship script**, idempotent, serialised by a kernel lock (`lockf`): checks, record, commit to
  `main`, staged install, push. A committed record is never rewritten.

## Alternatives rejected

- **Records read at run time** from the repo or from a file in the bundle. The app would have to
  find the repo, or the install would own a second artifact that must match the binary, and the
  build could no longer prove a record readable.
- **A host trait** (`installed_build`, `restart`, `send`) driven by `ui/`. `ui/` would gain timing
  policy, the iPhone a trait of no-ops, and the snap a fake host. A number in and an event out
  does the same.
- **Sending owned by `ui/`.** The body rule, the retry rule and the secret would sit in a view file,
  and "AI state lives in core" would have no feedback twin to copy.
- **A persisted outbox** so failed text survives a quit. It costs a second file per item and a
  reconciliation on every launch; only surviving a failed send was asked for.
- **One shared `feedback/records.json`.** Two ships would edit one file.
- **Crash-recovery machinery** proposed by a cross-judge: a fsynced ship journal, a frozen tree,
  commit discovery by tree and trailer, an installer journal, `--print-build-info`, UUID ids with a
  separate `--sent-at`, an "unconfirmed" outbox state and a record schema version. One person uses
  this loop, an agent watches each run, and each stage of `ship.sh` already reads the state that
  exists, so a rerun converges.

## Consequences

- Every shipped record recompiles `inkling-core` and everything above it. On a warm `target/` a
  ship takes about 40 s, almost all of it the tests, clippy, the Mac snap and the install.
- `seconds_to_ship` runs from the send to when the record is written, after the checks.
- Each device lists only what it sent. The iPhone shows its items as waiting until a new iPhone
  build carries their records.
- Failed text survives closing the feedback screen, not quitting the app.
- The Mac host reads the exact CFBundleVersion line `install.sh` writes, with no plist parser.

# TOB-435 synthesis: the contract for the implementation

The base is this folder's sketch (candidate A). Where this file and the sketch disagree, this file wins.
Delete `design/` in the last commit. The lasting rationale goes to `docs/adr/` and `docs/ARCHITECTURE.md`.

## Why this base

One `core::feedback` module owns all feedback state, the way `Revision` owns AI state. Status is a lookup
in the running build's records. The feedback id carries the send time, so no second timestamp can
disagree. Records compile into `inkling-core`, so both Cargo workspaces get them with no extra step. The
ship script is short enough to read in one screen.

The cross-judge (GPT 6 Astra) recommended candidate B for its crash recovery: a ship journal with fsync,
a frozen tree, commit discovery by tree and trailer, and an installer journal. That machinery is rejected.
One person uses this loop, an agent watches each run and can read a failed state, and the goal is a loop
that takes seconds and that the next agent can change safely. The judge's findings against A are real and
are fixed by the grafts below.

## Grafts into the base (all required)

1. **HTTPS only, no redirects.** `FeedbackLink::parse` accepts only `https://`. The one exception is
   `http://127.0.0.1` under `#[cfg(test)]`, for the mock server. The POST never follows a redirect. Send
   errors shown to the writer never include the URL or the secret.
2. **Fake transport in every headless mode.** The fake is used when `INKLING_FAKE_AI` or
   `INKLING_MEMORY_KEYS` is set, the same rule as `default_keys()`. `mac/snap.sh`'s second pass sets only
   `INKLING_MEMORY_KEYS`, and it must never reach the network. Put the rule in one function that both
   `default_keys()` and feedback call.
3. **Kernel lock, not a pid folder.** `feedback/ship.sh` re-executes itself under `/usr/bin/lockf -t 0`
   on a lock file under `$(git rev-parse --git-common-dir)`. The kernel releases it when the process dies,
   so there is no stale lock to judge.
4. **Staged install.** `mac/install.sh` copies the signed app to `Inkling.app.new` next to the
   destination, then swaps with two renames and removes the old one. The running app's bundle is never
   half-copied and never missing for more than the two renames. A leftover `.new` or `.old` from a crash
   is removed at the start of the next run.
5. **Seconds measured after the gates.** Order in `ship.sh`: preflight, tests, clippy, snap, write the
   record, `cargo build -p inkling-core` (proves the record parses, about 2 s), commit, install, push.
   `seconds_to_ship` is "now minus the id's send time" taken when the record is written. Once the record
   is in `HEAD` it is never rewritten.
6. **iPhone snap shows a failed send.** `ios/snap.sh` adds: paste a link, send, see it pending, and send
   through the fake's failing host and see the text kept with "Try again". Done and declined are proved on
   the Mac snap only, in both the Mac and the phone layout, because `ui/` is shared.

## Decisions on the sketch's open questions

- **Header.** `x-inkling-signature: sha256=<lowercase hex>`. HMAC-SHA256 over the raw body bytes, keyed by
  the secret's UTF-8 bytes.
- **Link.** `https://<webhook url>#k=<secret>`. Toby gets a ready-made link from a helper, outside this PR.
- **Push.** After a successful install, `ship.sh` runs `git push origin main`. If the push fails it says so
  and exits 3 ("installed, not pushed"). Preflight runs `git fetch origin main` and stops with exit 2 when
  `origin/main` is not an ancestor of `HEAD`, naming `git pull --rebase origin main` as the fix.
- **Declined.** Same pipeline as done. The record is the change.
- **Lists are per device.** Each device lists what it sent.
- **Clean start.** `ship.sh` does not police what the agent edited. `git add -A` is right in a checkout
  that only feedback threads use. The agent's instructions require a clean tree before editing.
- **Crypto dependency.** Use whichever of `ring`, or `hmac` + `sha2` + `getrandom`, is already in BOTH
  `Cargo.lock` and `ios/Cargo.lock`. Add no crate that is in neither. Say which you chose and why.

## Rejected from candidate B

Ship journal and fsync, tree freezing, a paths allowlist file, the installer journal, `--print-build-info`,
UUID ids with a separate `--sent-at`, a durable outbox with an "unconfirmed" state, and a record schema
version.

## Not in this PR

`feedback/AGENT.md` and the CLAUDE.md rules for feedback threads (the parent writes them), the T3 webhook
task, the standing checkout, the link helper, installing to a real iPhone.

## Known gap, tracked on TOB-435

The iPhone text bridge returns a masked field's plain text through `textInRange:`
(`ios/patches/0002-uitextinput-bridge.patch`). It already applies to provider keys. Do not fix it here.

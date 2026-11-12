# TOB-435: feedback from inside Inkling

## Problem

Toby wants to type what should change inside Inkling. An agent on his Mac should implement it,
install the next build, and the app should show what happened to each thing he sent. The
decisions are fixed (grounding doc): one signed HTTPS POST to a T3 webhook, one pasted link per
device kept in the Keychain, a sent log in the data folder, one record file per feedback compiled
into the app, status derived from records, a Restart button on the Mac only, and one idempotent
ship script. What makes the shape non-obvious is where each fact lives. Five facts cross four
places: the app on two devices, two Cargo workspaces, the repo and the installed bundle. Those
facts are the id, the send time, the build number, the records and the link's secret. The
constraints: `core` has no UI dependency; `ui/` never reads the platform; anything compiled in
must build in `ios/`'s separate workspace too; headless runs never touch the network or the
Keychain; the body must never carry draft text (PRODUCT.md); no stage may cost more than a few
seconds.

## Usage (caller's view)

Toby, once per device: copy the webhook link from T3 Code, add `#k=<secret>`, paste it into
**Feedback link** on the key screen (⌘, on Mac, the provider button on iPhone). A **Feedback**
button then appears in the Mac footer and in the iPhone status row. He opens it, types "Make the
margins wider", and presses **Send**. The row "Waiting for a build" appears. A few minutes later the
Mac footer says "Build 313 is installed. Restart". He presses it, and the row now reads "Done in 7
min · The page margins are 32 pt wider on the Mac."

The feedback agent, after making the change:

```sh
feedback/ship.sh done 1762899330-k7q2m9xd "The page margins are 32 pt wider on the Mac."
feedback/ship.sh declined 1762899871-p4w2c6ta "The title is already 36 pt, the largest DESIGN.md allows."
```

The UI, the only caller of `core::feedback`:

```rust
// ui/src/feedback.rs: Send
let from = SentFrom { form_factor: self.form.id(), window: (w, h), screen: screen.from };
let Some(post) = self.feedback.send(&text, self.keys.as_ref(), from) else { return cx.notify() };
let run = cx.background_spawn(async move { post.run() });
self.sending = cx.spawn_in(window, async move |this, cx| {
    let result = run.await;
    this.update_in(cx, |this, window, cx| this.feedback_settled(result, window, cx))
});

// rendering the list: status is computed here, never stored
for item in self.feedback.items() {
    match item.status { Status::Pending => …, Status::Done { summary, seconds_to_ship } => …, Status::Declined { reason } => … }
}

// key screen: Save link
let status = Desk::save_link(self.keys.as_ref(), pasted);
self.feedback_on = Desk::has_link(self.keys.as_ref());
```

The Mac host:

```rust
let (_, view) = open(cx, FormFactor::Mac, true, true, default_keys(), Build::running())?;
if let Some(bundle) = running_bundle() {
    watch_bundle(&view, bundle.clone(), cx);                 // set_installed_build every 5 s
    cx.subscribe(&view, move |_, _: &Restart, cx| restart(&bundle, cx)).detach();
}
```

## Shape

**Data first.** Seven types carry everything (`design/tob-435/core/src/feedback/`):

| Type | Owner | Lives in |
| --- | --- | --- |
| `FeedbackId` = send time + 8 random base32 chars, `1762899330-k7q2m9xd` | app, at first Send | sent log, body, record file name |
| `FeedbackLink` { url, secret } (private to `feedback`) | key store | Keychain account `feedback-link` |
| `Sent` { id, text } | `Desk` | `sent-feedback.json` |
| `Record` { id, `Outcome::Done{summary}` \| `Declined{reason}`, seconds_to_ship } | `ship.sh` | `feedback/<id>.json`, compiled in |
| `Build` { version, number: `Option<BuildNumber>`, records } | the binary | `core/build.rs` output + `INKLING_BUILD` |
| `Status` = Pending \| Done \| Declined | nobody: derived | `Build::status(id)` |
| `Delivery` = Idle \| Sending \| Stopped(CantSend) \| Failed(SendError) | `Desk` | memory |

**The id carries the send time.** It is the one source for the body's `sent_at`, the list's time
and `seconds_to_ship`, so `ship.sh` takes no `--sent-at` flag that could disagree with the body
(single source of truth, per minimize-reader-load).

**One deep module, `core::feedback::Desk`**, is the UI's whole interface. Its public surface is
`open`, `has_link`, `save_link`, `send`, `settle`, `delivery`, `items` and `build`, plus
`Post::run`. Behind it are the link format and its parsing, id minting, the rule that a retry
keeps its id only for unchanged text (`Attempt::next`, pure), the body schema, the HMAC, the
transport and its fake, the sent log on disk, and status derivation. This mirrors the existing
`Revision` (docs/CONVENTIONS.md, "AI state lives in core"), so an agent copying the nearest
example lands on the same pattern. `send`/`settle` is a split, not a pass-through: the network
wait sits between them on GPUI's executor. `#[must_use]` on `Post` and on `run`'s result keeps
either half from being dropped. `FeedbackLink`, `Attempt` and `Body` are not exported (they are
internals a caller can't import).

**Records compile in through `core/build.rs`.** Every app links `inkling-core`, both workspaces
build it from `core/`'s manifest dir, and `../feedback` resolves the same in both. So no host and
no second workspace needs a step of its own. The build script includes
`src/feedback/record.rs` by `#[path]`, so the parser that reads records at run time also
validates them at build time. A bad record fails `cargo test` inside `ship.sh`, before the commit.
That is encode-lessons-in-structure: a build error replaces a "write valid JSON" instruction.
Bash never writes JSON: `ship.sh` calls `core/examples/feedback-record.rs`, which uses the same
`record::to_json`.

**Build number.** `mac/install.sh` exports `INKLING_BUILD=$build` to the same `cargo build` and
writes the same `$build` to CFBundleVersion. `core/build.rs` validates it into
`INKLING_BUILD_NUMBER`. Debug builds (tests, snaps, `cargo run`) leave it unset, so `number` is
`None`, and a development build never offers a restart.

**The body can't carry the draft.** `post::Body`'s only runtime-text field is `text`. Every other
field is a number, a `&'static str` (`SentFrom.form_factor` and `screen` are `&'static str` for
exactly this reason) or comes from `Build`. A golden test pins the key set and says why in its
doc comment, so adding a field means editing a test that names the product rule.

**The secret leaves only as an HMAC.** It rides in the link's URL fragment, which no HTTP client
sends. `Post` is built from `url()` (no fragment) and `sign()`. `FeedbackLink`'s `Debug` redacts
it, and it has no `Display`, no `Serialize` and no accessor. The Keychain is a boundary, so the
stored string is parsed again at send (per boundary-discipline).

**`KeyStore` becomes keyed by `Secret`** (`ApiKey(Provider)` | `FeedbackLink`), one list of what
the Keychain holds. The existing accounts keep their names, so nothing migrates.

**A failed send keeps the text.** The field lives on `Inkling`, not on the screen, so it survives
closing the screen. `Delivery::Failed` labels the button "Try again", and the retry reuses the id
when the text is unchanged. If the first POST reached T3 and only the answer was lost, T3's dedupe
drops the retry. Edited text gets a new id, so T3 never drops a different message as a duplicate.

**Restart, Mac only.** The host polls CFBundleVersion of the bundle it was launched from every 5 s
and calls `set_installed_build`. The view offers Restart only while that number is greater than
`Build::number`. Pressing it saves the draft synchronously and emits `inkling_ui::Restart`. The
host's handler starts a detached `sh` that waits for this pid to exit, `open`s the bundle, and
then quits. `ui/` gets a number in and sends an event out, and never touches the platform.
`open()` never wires the watcher or the restart, so a snap can't quit itself.

**Entry points** (`ui/src/app.rs`, `ui/src/keys.rs`):

- The key screen gets a **Feedback link** section under Save key. It is the only feedback UI
  shown before a link exists.
- With a link saved, the Mac footer gets a **Feedback** text button between "Saved" and the
  provider, and the iPhone status row gets a 44 pt one.
- The feedback screen is a full-window screen built like the key screen: "What should change?",
  the field, Send, then **Feedback & fixes**.
- Status is in words. DESIGN.md keeps green and red for changes only.

**Ship script.** `feedback/ship.sh done|declined <id> "<line>"` runs these steps: preflight
(lock, main, no other uncommitted record, already committed?), record, tests, clippy, snap,
commit, install. It stops at the first failure. Each step reads the state that exists, and the
script's crash table covers every stop point (per make-operations-idempotent). The lock and the
"no other uncommitted record" check enforce one-at-a-time structurally
(separate-before-serializing-shared-state). The record files are per feedback, so they are never
shared; the checkout, `target/` and `/Applications` are shared, so ship.sh serializes on them.

**Tests (core, one per behaviour that can break):**

- the link round trip and redaction (a property)
- the HMAC against RFC 4231
- the body key set and its product rule
- retry-id reuse (a property)
- send end to end against the providers' mock server, moved to a shared test module, with the
  status mapping
- the sent log surviving a reload, duplicates and garbage
- the record parser's rules, and the status they decide

**Snaps.**

- `mac/snap.sh` gains about 20 checks and 6 screenshots: hidden until set up, a bad link refused,
  the link in no data file, send → pending, the sent log holding no draft text, a failed send
  keeping the text, Try again, a relaunch "as the next build" showing done, declined and pending,
  44 pt rows in the phone layout, and Restart offered only for a newer build, saving the draft and
  emitting once.
- `ios/snap.sh` goes from 12 to 15 steps: paste the link, open feedback, send → pending (with
  `sent-feedback.json` checked).

## Synthesis decision

## Tradeoffs accepted

- We accept that every shipped record recompiles `inkling-core` and everything above it in
  release, in exchange for records reaching both workspaces with no per-host step. Core is small,
  and a ship changes code above core anyway.
- We accept that `seconds_to_ship` measures send → last ship attempt (the record is written
  before the checks), in exchange for the build proving the record readable before the commit.
  Checks plus install add about 30 s to an agent's minutes.
- We accept that a failed feedback's text survives closing the screen but not quitting the app,
  in exchange for no outbox file and no second persisted state. The sent log only ever holds what
  T3 accepted.
- We accept that the Mac host's plist reader knows the exact line `install.sh` writes, in exchange
  for no plist dependency and no subprocess every 5 s. Both files point at each other.
- We accept the bash id regex in `ship.sh` duplicating part of the parser. It exists only for
  path safety; the Rust parser in step 2 is the authority.
- We accept that the iPhone shows its items as pending until a new iPhone build, which this issue
  doesn't ship (TestFlight is out of scope). The Mac's build holds the records, but the Mac lists
  only what the Mac sent.
- We accept an `.invalid`-host rule in the fake transport (RFC 2606 guarantees no real link
  matches) in exchange for both snap scripts showing a failed send without a hook into the app.

## Alternatives considered

- **Records read at run time from the repo or from a file install.sh drops in the bundle.** This
  removes the recompile. But the running app would have to find the repo, or `ship.sh` would own
  a second artifact that has to match the binary, and the build could no longer prove a record
  readable. "Contained in the running build" would become "on a disk somewhere", which is a
  second source of truth.
- **The host passes `ui` a trait (`trait Host { fn installed_build(); fn restart(); fn send() }`)
  and `ui` drives the polling and the network.** This gives one injection point. But `ui` would
  gain timing policy it doesn't need, iPhone would get a trait full of no-ops, and the snap would
  need a fake host. A number in and an event out is a smaller surface with the same capability.
- **Sending owned by `ui`: it builds the JSON, signs it and keeps the in-flight state, with core
  only parsing records.** This is shallow. The body rule, the retry-id rule and the link secret
  would sit in a view file an agent edits casually, and "AI state lives in core" would have no
  feedback twin to copy.
- **A persisted outbox (`outbox/<id>.json` before the send, moved to the sent log after a 2xx).**
  Failed text would survive a quit, but at the cost of a second file per item and a reconciliation
  step on every launch. The grounding asks only that typed text survive a failed send.
- **A shared `feedback/records.json` appended by `ship.sh`.** This is rejected by decision 7 and
  by separate-before-serializing: two ships would edit one file.

## Open questions and risks

- What header name does T3's webhook verifier read? The sketch uses `x-inkling-signature-256`
  with `sha256=<hex>`, keyed by the secret string's UTF-8 bytes, the way GitHub does it.
- Does T3 show the webhook URL and the secret separately? If so, is "append `#k=<secret>`" an
  acceptable way to make the link, or should T3 Code (or a one-line example) print a ready-made
  link?
- Should `ship.sh` push to `origin main` after committing, so CI runs on each shipped commit?
- Is a declined feedback worth the full ship? The record alone changes the app, so the sketch
  runs the same checks and install. That costs about 30 s.
- Should iPhone feedback on a running Mac also appear in the Mac's list, by syncing sent logs?
  The sketch says no: each device lists what it sent.
- Risk: `ring` as a direct dependency of core. It is already in the tree through rustls and gives
  both HMAC and the random source. If rustls moves to aws-lc-rs, switch to `hmac` + `sha2` +
  `getrandom`, which are already in Cargo.lock.
- Risk: GPUI's `background_spawn` in the headless snap context runs on the test dispatcher. The
  fake `Post::run` never blocks, so `run_until_parked` should settle it. If it doesn't, use a std
  thread and a channel, as the AI pump does.

## Next implementation step

Write `core/src/feedback/record.rs` and `core/build.rs` with the `records_decide_status` test,
compiling the example record from `design/tob-435/feedback/` in both workspaces.

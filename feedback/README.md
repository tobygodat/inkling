# Feedback records

One file per feedback, `<id>.json`, written only by `feedback/ship.sh`. `core/build.rs` compiles
every record here into `inkling-core`, so the running build knows what happened to each thing the
writer sent. A sent item with no record in the running build reads "Waiting for a build".

- The id comes from the webhook body, such as `1762899330-k7q2m3xd`: the send time in Unix seconds,
  a dash, and 8 characters of `a-z` and `2-7`.
- A record is done (with a summary of what changed) or declined (with the reason). Either is one
  line the writer reads in Feedback & fixes: plain words, at most 160 characters.
- `seconds_to_ship` runs from the id's send time to when `ship.sh` wrote the record.
- Never edit a committed record. A new decision is a new feedback.
- One file per feedback, so two ships never edit the same file.
- `core/src/feedback/record.rs` is the one parser. A file it rejects fails the build.

```json
{ "id": "1762899330-k7q2m3xd", "outcome": "done", "summary": "…", "seconds_to_ship": 412 }
{ "id": "1762899871-p4w2c6ta", "outcome": "declined", "reason": "…", "seconds_to_ship": 95 }
```

## ship.sh

The feedback agent runs it from the standing checkout on `main`, after it has made the change or
decided not to:

```sh
feedback/ship.sh done     <id> "<what changed, in the writer's words>"
feedback/ship.sh declined <id> "<why not>"
```

It takes a lock, checks the checkout, runs the tests, clippy and `mac/snap.sh`, writes the record,
builds `inkling-core` to prove the record reads, commits everything to `main`, installs the app
with `mac/install.sh` and pushes. It prints each stage's seconds. A rerun skips what is already
done: a committed record goes straight to install and push, an installed build skips the install,
and a pushed commit skips the push.

`INKLING_INSTALL_DIR` sets where the app is installed (default `/Applications`).

| Exit | Meaning |
| --- | --- |
| 0 | Shipped, or already shipped |
| 1 | A stage failed; the message names it. Fix it and rerun the same command |
| 2 | Bad usage, or a checkout `ship.sh` won't touch; the message says why |
| 3 | Installed, not pushed |
| 75 | Another `ship.sh` holds the lock; rerun when it ends |

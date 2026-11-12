# Feedback records (sketch of feedback/README.md)

One file per feedback, `<id>.json`, written only by `feedback/ship.sh`. `core/build.rs` compiles
every file here into the app, so the running build knows what happened to each thing the writer
sent. A sent item with no record here is "Waiting for a build".

- Never edit a committed record. A new decision is a new feedback.
- One file per feedback, so two ships never edit the same file.
- The format and its rules are in `core/src/feedback/record.rs`, the one parser. A file it
  rejects fails the build.

```json
{ "id": "1762899330-k7q2m9xd", "outcome": "done", "summary": "…", "seconds_to_ship": 412 }
{ "id": "1762899871-p4w2c6ta", "outcome": "declined", "reason": "…", "seconds_to_ship": 95 }
```

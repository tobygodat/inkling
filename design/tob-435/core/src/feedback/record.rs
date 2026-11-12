//! SKETCH: core/src/feedback/record.rs
//!
//! The record file format: one `feedback/<id>.json` per feedback, written by `feedback/ship.sh`,
//! compiled into the app by `core/build.rs`.
//!
//! This file is the ONE parser for that format. `core/build.rs` includes it with
//! `#[path = "src/feedback/record.rs"] mod record;`, so a record the app couldn't read fails the
//! build (and so fails `ship.sh` at its `cargo test` step, before anything is committed).
//! Keep it to `std`, `serde` and `serde_json`: those are the build script's only dependencies.

use serde::{Deserialize, Serialize};
use std::fmt;

/// One feedback's identity, made by the app when the writer first presses Send.
///
/// Text form: `<unix seconds>-<8 chars of [a-z2-7]>`, e.g. `1762899330-k7q2m9xd`.
/// The seconds are the send time, so the id is the single source for "sent at": the body's
/// `sent_at`, the list's time and `ship.sh`'s `seconds_to_ship` all read it from here.
/// Ids sort by send time as plain strings, which is also the order of `feedback/` on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeedbackId {
    sent_at: u64,
    nonce: [u8; 8], // ASCII, each in [a-z2-7]
}

impl FeedbackId {
    // `FeedbackId::new(now)` lives in `feedback/mod.rs`: it needs `ring` for the nonce, and the
    // build script that includes this file has no `ring`.

    pub(crate) fn from_parts(sent_at: u64, nonce: [u8; 8]) -> Self {
        FeedbackId { sent_at, nonce }
    }

    pub fn sent_at(self) -> u64 {
        self.sent_at
    }

    /// Validates at the boundary: the sent log, a record file name, a record's `id` field.
    pub fn parse(s: &str) -> Result<Self, String> {
        // TODO: split once on '-'; left is 1..=20 ASCII digits (u64), right is exactly 8 chars
        // of [a-z2-7]. Anything else: Err(format!("\"{s}\" is not a feedback id")).
        todo!()
    }
}

impl fmt::Display for FeedbackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("{sent_at}-{nonce}")
    }
}

// Serialize/Deserialize as the text form, through `parse`, so no unchecked id is ever built.
impl Serialize for FeedbackId {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        todo!()
    }
}
impl<'de> Deserialize<'de> for FeedbackId {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        todo!("String, then FeedbackId::parse, mapping Err to D::Error::custom")
    }
}

/// What the agent did with one feedback. A record exists only once the agent has decided, so
/// there is no `Pending` here: pending is the absence of a record (see `feedback::Status`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Shipped. `summary` is one line, in the writer's terms, of what changed.
    Done { summary: String },
    /// Not done. `reason` is one line saying why.
    Declined { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: FeedbackId,
    pub outcome: Outcome,
    /// From the id's send time to the moment `ship.sh` last wrote this record.
    pub seconds_to_ship: u64,
}

/// The longest summary or reason, in chars. The list shows it on one or two lines.
pub const MAX_LINE: usize = 160;

/// Parses one record file. `stem` is the file name without `.json`; it must equal the id inside,
/// so a copied-and-edited record can't shadow another feedback.
///
/// Wire shape (private; the domain type above is what callers see):
/// ```json
/// { "id": "1762899330-k7q2m9xd", "outcome": "done", "summary": "…", "seconds_to_ship": 412 }
/// { "id": "1762899330-k7q2m9xd", "outcome": "declined", "reason": "…", "seconds_to_ship": 95 }
/// ```
/// Rules: unknown fields are an error; `done` needs `summary` and no `reason`, `declined` the
/// reverse; the line is trimmed, non-empty, has no newline and is at most `MAX_LINE` chars.
pub fn parse(stem: &str, bytes: &[u8]) -> Result<Record, String> {
    // TODO: serde_json::from_slice::<Wire>(bytes) with #[serde(deny_unknown_fields)], where
    //   struct Wire { id: FeedbackId, outcome: WireOutcome, summary: Option<String>,
    //                 reason: Option<String>, seconds_to_ship: u64 }
    //   enum WireOutcome { done, declined }   (#[serde(rename_all = "lowercase")])
    // then check stem == id.to_string(), the one-line rule, and the summary/reason pairing.
    // Every Err names the field: "summary has a line break", "id 1… doesn't match the file name".
    todo!()
}

/// The inverse of `parse`. Its one caller outside tests is `core/examples/feedback-record.rs`,
/// which `ship.sh` runs to write the file, so bash never hand-escapes JSON.
pub fn to_json(record: &Record) -> String {
    todo!("pretty JSON in the wire shape, keys in the order above, trailing newline")
}

/// Writes `<dir>/<id>.json` through `<id>.json.tmp` and a rename, after checking the line with the
/// same rules as `parse`. `seconds_to_ship` is `now - id.sent_at()`, saturating at 0. Returns the
/// path. The only caller is `core/examples/feedback-record.rs`, which `feedback/ship.sh` runs.
pub fn write_record(dir: &std::path::Path, id: FeedbackId, outcome: Outcome, now: std::time::SystemTime) -> Result<std::path::PathBuf, String> {
    todo!()
}

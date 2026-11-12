//! The record file format: one `feedback/<id>.json` per feedback, written by `feedback/ship.sh`
//! through `core/examples/feedback-record.rs`, and compiled into the app by `core/build.rs`.
//!
//! This file is the one parser for that format. `core/build.rs` includes it by `#[path]`, so a
//! record the app couldn't read fails the build. Keep it to `std`, `serde` and `serde_json`, the
//! build script's only dependencies.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One feedback's identity, made by the app when the writer first presses Send.
///
/// Text form: `<unix seconds>-<8 chars of [a-z2-7]>`, such as `1762899330-k7q2m3xd`. The seconds
/// are the send time, so the id is the one source for "sent at": the body's `sent_at`, the list's
/// time and `seconds_to_ship` all read it from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeedbackId {
    sent_at: u64,
    nonce: [u8; 8],
}

/// RFC 4648 base32, lowercased: the nonce's alphabet.
pub(crate) const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

impl FeedbackId {
    /// `nonce` holds 8 base32 digits as indexes, each below 32.
    pub(crate) fn from_parts(sent_at: u64, nonce: [u8; 8]) -> Self {
        FeedbackId { sent_at, nonce: nonce.map(|digit| BASE32[usize::from(digit & 31)]) }
    }

    pub fn sent_at(self) -> u64 {
        self.sent_at
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || format!("\"{s}\" is not a feedback id");
        let (seconds, nonce) = s.split_once('-').ok_or_else(bad)?;
        let digits = !seconds.is_empty() && seconds.len() <= 20 && seconds.bytes().all(|b| b.is_ascii_digit());
        let sent_at = seconds.parse().ok().filter(|_| digits).ok_or_else(bad)?;
        let nonce: [u8; 8] = nonce.as_bytes().try_into().map_err(|_| bad())?;
        if !nonce.iter().all(|b| BASE32.contains(b)) {
            return Err(bad());
        }
        Ok(FeedbackId { sent_at, nonce })
    }
}

impl fmt::Display for FeedbackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nonce = std::str::from_utf8(&self.nonce).map_err(|_| fmt::Error)?;
        write!(f, "{}-{nonce}", self.sent_at)
    }
}

impl Serialize for FeedbackId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FeedbackId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        FeedbackId::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// What the agent did with one feedback. Pending is the absence of a record.
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
    /// From the id's send time to the moment `ship.sh` wrote this record.
    pub seconds_to_ship: u64,
}

/// The longest summary or reason, in chars. The list shows it on one or two lines.
pub const MAX_LINE: usize = 160;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    id: FeedbackId,
    outcome: WireOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    seconds_to_ship: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireOutcome {
    Done,
    Declined,
}

impl Wire {
    fn record(self) -> Result<Record, String> {
        let outcome = match (self.outcome, self.summary, self.reason) {
            (WireOutcome::Done, Some(summary), None) => Outcome::Done { summary: one_line("summary", &summary)? },
            (WireOutcome::Declined, None, Some(reason)) => Outcome::Declined { reason: one_line("reason", &reason)? },
            (WireOutcome::Done, _, _) => return Err("a done record has a summary and no reason".into()),
            (WireOutcome::Declined, _, _) => return Err("a declined record has a reason and no summary".into()),
        };
        Ok(Record { id: self.id, outcome, seconds_to_ship: self.seconds_to_ship })
    }

    fn from(record: &Record) -> Wire {
        let (outcome, summary, reason) = match &record.outcome {
            Outcome::Done { summary } => (WireOutcome::Done, Some(summary.clone()), None),
            Outcome::Declined { reason } => (WireOutcome::Declined, None, Some(reason.clone())),
        };
        Wire { id: record.id, outcome, summary, reason, seconds_to_ship: record.seconds_to_ship }
    }
}

/// The line the writer reads: trimmed, not empty, no line break, at most `MAX_LINE` chars.
fn one_line(field: &str, line: &str) -> Result<String, String> {
    let line = line.trim();
    if line.is_empty() {
        Err(format!("{field} is empty"))
    } else if line.contains(['\n', '\r']) {
        Err(format!("{field} has a line break"))
    } else if line.chars().count() > MAX_LINE {
        Err(format!("{field} is longer than {MAX_LINE} characters"))
    } else {
        Ok(line.to_string())
    }
}

/// Parses one record file. `stem` is the file name without `.json` and must equal the id inside,
/// so a copied-and-edited record can't shadow another feedback.
///
/// ```json
/// { "id": "1762899330-k7q2m3xd", "outcome": "done", "summary": "…", "seconds_to_ship": 412 }
/// { "id": "1762899871-p4w2c6ta", "outcome": "declined", "reason": "…", "seconds_to_ship": 95 }
/// ```
/// Outside tests its caller is `core/build.rs`; the app reads `parse_all`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse(stem: &str, bytes: &[u8]) -> Result<Record, String> {
    let wire: Wire = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if wire.id.to_string() != stem {
        return Err(format!("id {} doesn't match the file name", wire.id));
    }
    wire.record()
}

/// The records as `core/build.rs` writes them for the app: one JSON array of `to_json` outputs.
pub fn parse_all(bytes: &[u8]) -> Result<Vec<Record>, String> {
    let wires: Vec<Wire> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    wires.into_iter().map(Wire::record).collect()
}

/// The inverse of `parse`: pretty JSON with a trailing newline.
pub fn to_json(record: &Record) -> String {
    let mut json = serde_json::to_string_pretty(&Wire::from(record)).expect("a record serializes");
    json.push('\n');
    json
}

/// Writes `<dir>/<id>.json` through a temp file and a rename, so a crash leaves the old file or
/// the new one. `seconds_to_ship` is `now` minus the id's send time. Returns the path.
pub fn write_record(dir: &Path, id: FeedbackId, outcome: Outcome, now: SystemTime) -> Result<PathBuf, String> {
    let outcome = match outcome {
        Outcome::Done { summary } => Outcome::Done { summary: one_line("summary", &summary)? },
        Outcome::Declined { reason } => Outcome::Declined { reason: one_line("reason", &reason)? },
    };
    let now = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let record = Record { id, outcome, seconds_to_ship: now.saturating_sub(id.sent_at()) };
    let path = dir.join(format!("{id}.json"));
    let tmp = dir.join(format!("{id}.json.tmp"));
    let write = |e: std::io::Error| format!("couldn't write {}: {e}", path.display());
    std::fs::write(&tmp, to_json(&record)).map_err(write)?;
    std::fs::rename(&tmp, &path).map_err(write)?;
    Ok(path)
}

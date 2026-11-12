//! Feedback from inside Inkling: the writer says what should change, an agent on the Mac ships
//! it, and the app shows what happened to each thing sent.
//!
//! All feedback state lives in one owner, `Desk`, the way all AI state lives in `Revision`. The
//! UI renders `Desk` and keeps no copy.
//!
//! ```text
//!  key screen ── save_link(pasted) ──▶ KeyStore[Secret::FeedbackLink]
//!  feedback screen ── send(text, keys, from) ──▶ Post ──(background)──▶ run() ──▶ settle(result)
//!                                                                         │ Ok: append sent log
//!  items() = sent log ⨝ Build::records ──▶ Pending | Done(summary) | Declined(reason)
//! ```
//!
//! Status is never stored. A sent item is pending until the running build holds a record with
//! its id. The app writes only the sent log; `feedback/ship.sh` writes the records.

mod link;
mod post;
mod record;
#[cfg(test)]
mod tests;

use link::FeedbackLink;
pub use post::{Post, SendError};
pub use record::{FeedbackId, MAX_LINE, Outcome, Record, write_record};

use crate::store::{KeyStore, Paths, Secret, write_atomic};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

/// `git rev-list --count HEAD` when the binary was built. Ordered, so "the bundle on disk is
/// newer" is `installed > running`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildNumber(pub u32);

/// What this binary is: its version, its build number and the records compiled into it.
#[derive(Clone, Debug)]
pub struct Build {
    /// inkling-core's version. `mac/install.sh` writes the same one to Info.plist.
    pub version: &'static str,
    /// `None` for a development build (`cargo run`, the snaps, tests), which never offers a restart.
    pub number: Option<BuildNumber>,
    records: BTreeMap<FeedbackId, Record>,
}

impl Build {
    /// This binary. `core/build.rs` validated every `feedback/*.json` into `records.json` and
    /// turned `INKLING_BUILD` into `INKLING_BUILD_NUMBER`.
    pub fn running() -> Self {
        const RECORDS: &str = include_str!(concat!(env!("OUT_DIR"), "/records.json"));
        let records = record::parse_all(RECORDS.as_bytes()).expect("core/build.rs validated the records");
        let number = option_env!("INKLING_BUILD_NUMBER").map(|n| BuildNumber(n.parse().expect("core/build.rs checked it")));
        Build::with_records(number, records)
    }

    /// A build with exactly these records, for the Mac snap's "next build" and for tests.
    pub fn with_records(number: Option<BuildNumber>, records: impl IntoIterator<Item = Record>) -> Self {
        Build { version: env!("CARGO_PKG_VERSION"), number, records: records.into_iter().map(|r| (r.id, r)).collect() }
    }

    fn status(&self, id: FeedbackId) -> Status<'_> {
        let Some(record) = self.records.get(&id) else { return Status::Pending };
        match &record.outcome {
            Outcome::Done { summary } => Status::Done { summary, seconds_to_ship: record.seconds_to_ship },
            Outcome::Declined { reason } => Status::Declined { reason },
        }
    }
}

/// One thing the writer sent, as the sent log keeps it. Nothing else is kept.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sent {
    pub id: FeedbackId,
    pub text: String,
}

/// Derived from the running build, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status<'a> {
    /// No record in the running build yet.
    Pending,
    Done { summary: &'a str, seconds_to_ship: u64 },
    Declined { reason: &'a str },
}

#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub sent: &'a Sent,
    pub status: Status<'a>,
}

/// Where the writer was when they sent it. Every field is a number or a `&'static str`, so it
/// can't carry draft text.
#[derive(Clone, Copy, Debug)]
pub struct SentFrom {
    /// "mac" or "phone".
    pub form_factor: &'static str,
    /// Window size in points.
    pub window: (u32, u32),
    /// "page", "review" or "keys": the screen open before the feedback screen.
    pub screen: &'static str,
}

/// The longest feedback, in chars. A paragraph, not a pasted draft.
pub const MAX_TEXT: usize = 2_000;

/// Why Send stopped before the network. The text stays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CantSend {
    Empty,
    TooLong,
    /// The stored link is gone or unreadable: removed from the Keychain behind the app's back.
    NoLink,
}

impl fmt::Display for CantSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CantSend::Empty => "Write what should change first.",
            CantSend::TooLong => "That's more than 2,000 characters. Say it shorter.",
            CantSend::NoLink => "Paste the feedback link on the key screen first.",
        })
    }
}

/// The one send the desk allows at a time, and what became of the last one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Ready to send. Also the state after a success.
    Idle,
    /// A `Post` is out. A second `send` returns no `Post`.
    Sending,
    /// Stopped before the network.
    Stopped(CantSend),
    /// The network said no. The same text retried keeps its id.
    Failed(SendError),
}

/// The text the desk is sending and the id bound to it. A retry of the same text keeps the id,
/// so if the first attempt arrived and only the answer was lost, the webhook drops the retry as a
/// duplicate. Changed text gets a new id, so a different message is never dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Attempt {
    id: FeedbackId,
    text: String,
}

impl Attempt {
    fn next(last: Option<&Attempt>, text: &str, now: SystemTime) -> Attempt {
        match last {
            Some(last) if last.text == text => last.clone(),
            _ => Attempt { id: FeedbackId::new(now), text: text.to_string() },
        }
    }
}

impl FeedbackId {
    /// A new id sent at `now`. Lives here rather than in record.rs, which the build script shares
    /// and which has no `ring`.
    pub fn new(now: SystemTime) -> Self {
        let mut bytes = [0u8; 8];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes[3..])
            .expect("the system random source works");
        let bits = u64::from_be_bytes(bytes);
        let nonce = std::array::from_fn(|i| (bits >> (35 - 5 * i)) as u8 & 31);
        FeedbackId::from_parts(now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()), nonce)
    }
}

/// The UI's whole interface to feedback.
pub struct Desk {
    paths: Paths,
    build: Build,
    /// Oldest first, as on disk.
    sent: Vec<Sent>,
    /// The attempt in flight or the last one that failed. `None` after a success.
    attempt: Option<Attempt>,
    delivery: Delivery,
    /// A headless run's posts never touch the network (see `Post::run`).
    fake: bool,
}

impl Desk {
    /// Reads the sent log. A missing or unreadable log is an empty one.
    pub fn open(paths: Paths, build: Build) -> Self {
        let sent = load_sent(&paths);
        Desk { paths, build, sent, attempt: None, delivery: Delivery::Idle, fake: crate::store::headless() }
    }

    pub fn build(&self) -> &Build {
        &self.build
    }

    /// Whether feedback is set up on this device, asked without reading the link.
    pub fn has_link(keys: &dyn KeyStore) -> bool {
        keys.has(Secret::FeedbackLink)
    }

    /// Parses the pasted link and stores it. Nothing is stored when it doesn't parse.
    pub fn save_link(keys: &dyn KeyStore, pasted: &str) -> Result<(), String> {
        let link = FeedbackLink::parse(pasted).map_err(|bad| bad.to_string())?;
        keys.set(Secret::FeedbackLink, &link.to_stored())
    }

    /// Starts a send of `text` and returns the request to run off the UI thread, or `None` when
    /// nothing should go out (`delivery()` says why). Pass `Post::run`'s result to `settle`.
    pub fn send(&mut self, text: &str, keys: &dyn KeyStore, from: SentFrom) -> Option<Post> {
        if self.delivery == Delivery::Sending {
            return None;
        }
        let text = text.trim();
        let stop = if text.is_empty() {
            Some(CantSend::Empty)
        } else if text.chars().count() > MAX_TEXT {
            Some(CantSend::TooLong)
        } else {
            None
        };
        let link = keys.get(Secret::FeedbackLink).and_then(|s| FeedbackLink::parse(&s).ok());
        let link = match (stop, link) {
            (None, Some(link)) => link,
            (stop, _) => {
                self.delivery = Delivery::Stopped(stop.unwrap_or(CantSend::NoLink));
                return None;
            }
        };
        let attempt = Attempt::next(self.attempt.as_ref(), text, SystemTime::now());
        let body = post::Body {
            id: attempt.id,
            text: &attempt.text,
            sent_at: post::rfc3339(attempt.id.sent_at()),
            form_factor: from.form_factor,
            window: post::Window { width: from.window.0, height: from.window.1 },
            screen: from.screen,
            version: self.build.version,
            build: self.build.number.map(|n| n.0),
        };
        let post = Post::new(&link, &body, self.fake);
        self.attempt = Some(attempt);
        self.delivery = Delivery::Sending;
        Some(post)
    }

    /// The result of the `Post` that `send` returned. On success the item joins the sent log as
    /// pending; on failure the attempt is kept for a retry. `Err` only when the log couldn't be
    /// written: the item is still listed for this session.
    pub fn settle(&mut self, result: Result<(), SendError>) -> io::Result<()> {
        if self.delivery != Delivery::Sending {
            return Ok(());
        }
        let attempt = match result {
            Err(e) => {
                self.delivery = Delivery::Failed(e);
                return Ok(());
            }
            Ok(()) => self.attempt.take().expect("send set the attempt"),
        };
        self.delivery = Delivery::Idle;
        if self.sent.iter().any(|s| s.id == attempt.id) {
            return Ok(());
        }
        self.sent.push(Sent { id: attempt.id, text: attempt.text });
        save_sent(&self.paths, &self.sent)
    }

    pub fn delivery(&self) -> &Delivery {
        &self.delivery
    }

    /// Everything this device sent, newest first, each with its status in the running build.
    pub fn items(&self) -> impl Iterator<Item = Item<'_>> {
        self.sent.iter().rev().map(|sent| Item { sent, status: self.build.status(sent.id) })
    }
}

/// `<data dir>/sent-feedback.json`: a JSON array of `Sent`, oldest first, rewritten whole. Loading
/// drops duplicate ids and entries it can't read, so a hand-edited file never stops the app.
fn load_sent(paths: &Paths) -> Vec<Sent> {
    let Ok(bytes) = std::fs::read(paths.sent_feedback()) else { return Vec::new() };
    let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap_or_default();
    let mut sent: Vec<Sent> = Vec::new();
    for entry in entries {
        if let Ok(s) = serde_json::from_value::<Sent>(entry)
            && !sent.iter().any(|seen| seen.id == s.id)
        {
            sent.push(s);
        }
    }
    sent
}

fn save_sent(paths: &Paths, sent: &[Sent]) -> io::Result<()> {
    write_atomic(&paths.sent_feedback(), &serde_json::to_vec_pretty(sent)?)
}

//! SKETCH: core/src/feedback/mod.rs
//!
//! Feedback from inside Inkling: the writer says what should change, an agent on the Mac ships
//! it, and the app shows what happened to each thing sent.
//!
//! All feedback state lives here, in one owner, `Desk`, the way all AI state lives in
//! `Revision` (docs/CONVENTIONS.md). The UI renders `Desk` and keeps no copy. `Desk` hides:
//! the id and when a retry reuses it, the body and what it may hold, the signature, the link
//! format, the sent log on disk, the compiled-in records, and how status is derived.
//!
//! ```text
//!  key screen ── save_link(pasted) ──▶ KeyStore[Secret::FeedbackLink]
//!  feedback screen ── send(text, keys, from) ──▶ Post ──(background)──▶ run() ──▶ settle(result)
//!                                                                         │ Ok: append sent log
//!  items() = sent log ⨝ Build::records ──▶ Pending | Done(summary) | Declined(reason)
//! ```
//!
//! Status is never stored. A sent item is pending until the running build holds a record with
//! its id (decision 8). The only things written are the sent log (this module) and the record
//! files (`ship.sh`, never the app).

mod link;
mod post;
mod record;
#[cfg(test)]
mod tests;

// The link type stays inside `feedback`: the UI hands over the pasted string and gets a message.
use link::FeedbackLink;
pub use post::{Post, SendError};
pub use record::{FeedbackId, MAX_LINE, Outcome, Record, write_record};

use crate::store::{KeyStore, Paths, Secret};
use std::collections::BTreeMap;
use std::io;
use std::time::SystemTime;

// ───────────────────────────── the running build ─────────────────────────────

/// A build number: `git rev-list --count HEAD` when the binary was built. Ordered, so "the bundle
/// on disk is newer" is `installed > running`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildNumber(pub u32);

/// What this binary is: its version, its build number and the records compiled into it.
/// The host makes one at launch and hands it to the UI; nothing changes it while the app runs.
#[derive(Clone, Debug)]
pub struct Build {
    /// `CARGO_PKG_VERSION` of inkling-core. `mac/install.sh` reads the same one for Info.plist.
    pub version: &'static str,
    /// `None` for a development build (`cargo run`, the snaps, tests): it never offers a restart.
    pub number: Option<BuildNumber>,
    records: BTreeMap<FeedbackId, Record>,
}

impl Build {
    /// This binary. `core/build.rs` validated every `feedback/*.json` and wrote them as one
    /// JSON array to `$OUT_DIR/records.json`; it also turned `INKLING_BUILD` (set by
    /// `mac/install.sh` and `ios/cargo-ios.sh`) into `INKLING_BUILD_NUMBER`.
    pub fn running() -> Self {
        // TODO:
        // const RECORDS: &str = include_str!(concat!(env!("OUT_DIR"), "/records.json"));
        // records: serde_json over an array of the wire shape, each through record::parse;
        //   `expect("core/build.rs validated the records")` — it can't fail, the build did it.
        // number: option_env!("INKLING_BUILD_NUMBER").map(|n| BuildNumber(n.parse().unwrap()))
        todo!()
    }

    /// A build with exactly these records. For the Mac snap, which relaunches the app "as the
    /// next build" to show done and declined, and for tests.
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

// ───────────────────────────── what the list shows ─────────────────────────────

/// One thing the writer sent, as the app keeps it. The log holds nothing else (decision 6).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sent {
    pub id: FeedbackId,
    pub text: String,
}

/// Derived, never stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status<'a> {
    /// No record in the running build yet. Shown as "Waiting for a build".
    Pending,
    Done { summary: &'a str, seconds_to_ship: u64 },
    Declined { reason: &'a str },
}

#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub sent: &'a Sent,
    pub status: Status<'a>,
}

// ───────────────────────────── sending ─────────────────────────────

/// Where the writer was when they sent it. Every field is a number or a `&'static str`, so it
/// can't carry draft text (see `post::Body`). The UI fills it from its own state.
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

/// Why Send did nothing before reaching the network. Shown under the field; the text stays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CantSend {
    Empty,
    TooLong,
    /// The stored link is gone or unreadable. The UI hides feedback without a link, so this is
    /// a link removed from the Keychain behind the app's back.
    NoLink,
}

// `impl Display for CantSend`: "Write what should change first.", "That's more than 2,000
// characters. Say it shorter.", "Paste the feedback link on the key screen first."

/// The one send the desk allows at a time, and what became of the last one. The UI renders
/// this as the line under the field and the Send button's label; it keeps no copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Send. Also the state after a success: the item is in the list, the field is empty.
    Idle,
    /// "Sending…", Send disabled. A second `send` returns the same state and no `Post`.
    Sending,
    /// Stopped before the network. Button: Send.
    Stopped(CantSend),
    /// The network said no. Button: Try again (same text → same id).
    Failed(SendError),
}

/// The attempt the desk is on: the text and the id bound to it. A retry of the same text keeps
/// the id, so if the first attempt reached T3 and only the answer was lost, T3's dedupe drops the
/// retry. Changed text gets a new id, so T3 never drops a different message as a duplicate.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Attempt {
    id: FeedbackId,
    text: String,
}

impl Attempt {
    /// Pure, and the rule above in one place: `feedback::tests::a_retry_keeps_its_id_only_for_the_same_text`.
    fn next(last: Option<&Attempt>, text: &str, now: SystemTime) -> Attempt {
        match last {
            Some(last) if last.text == text => last.clone(),
            _ => Attempt { id: FeedbackId::new(now), text: text.to_string() },
        }
    }
}

impl FeedbackId {
    /// Lives here, not in record.rs, because it needs `ring` and record.rs is shared with the
    /// build script.
    pub fn new(now: SystemTime) -> Self {
        // TODO: secs = now.duration_since(UNIX_EPOCH); 5 bytes from ring::rand::SystemRandom,
        // base32 (RFC 4648 lowercase, no padding) → 8 chars; FeedbackId::from_parts(secs, chars).
        todo!()
    }
}

// ───────────────────────────── the desk ─────────────────────────────

/// The UI's whole interface to feedback.
pub struct Desk {
    paths: Paths,
    build: Build,
    /// Oldest first, as on disk. Loaded once in `open`; `settle` appends to both.
    sent: Vec<Sent>,
    /// The attempt in flight or the last one that failed. `None` after a success.
    attempt: Option<Attempt>,
    delivery: Delivery,
    /// `INKLING_FAKE_AI` at open: posts never touch the network (see `Post::run`).
    fake: bool,
}

impl Desk {
    /// Reads the sent log. A missing or unreadable log is an empty one, like `Settings::load`.
    pub fn open(paths: Paths, build: Build) -> Self {
        let sent = load_sent(&paths);
        let fake = std::env::var_os("INKLING_FAKE_AI").is_some();
        Desk { paths, build, sent, attempt: None, delivery: Delivery::Idle, fake }
    }

    pub fn build(&self) -> &Build {
        &self.build
    }

    /// Whether feedback is set up on this device. Asks the key store without reading the link,
    /// like the key screen's "Saved" (KeyStore::has). The UI asks at launch and after a save,
    /// never on a frame.
    pub fn has_link(keys: &dyn KeyStore) -> bool {
        keys.has(Secret::FeedbackLink)
    }

    /// Parses the pasted link and stores it. Nothing is stored when it doesn't parse.
    pub fn save_link(keys: &dyn KeyStore, pasted: &str) -> Result<(), String> {
        let link = FeedbackLink::parse(pasted).map_err(|bad| bad.to_string())?;
        keys.set(Secret::FeedbackLink, &link.to_stored())
    }

    /// Starts a send of `text` and returns the request to run off the UI thread, or `None` when
    /// nothing should go out (the reason is in `delivery()`). The caller must pass the result of
    /// `Post::run` to `settle`.
    pub fn send(&mut self, text: &str, keys: &dyn KeyStore, from: SentFrom) -> Option<Post> {
        // TODO:
        // if Sending { return None }  (delivery unchanged)
        // let text = text.trim(); Empty / TooLong (chars().count() > MAX_TEXT) → delivery = Stopped(_), None
        // let link = keys.get(Secret::FeedbackLink).and_then(|s| FeedbackLink::parse(&s).ok()).ok_or(NoLink)?
        //   (the Keychain is a boundary too, so the stored string is parsed again); else Stopped(NoLink)
        // let attempt = Attempt::next(self.attempt.as_ref(), text, SystemTime::now());
        // let body = post::Body { id, text, sent_at: rfc3339(id.sent_at()), form_factor, window,
        //                         screen, version: self.build.version, build: self.build.number };
        // self.attempt = Some(attempt); self.delivery = Sending;
        // Some(Post::new(&link, &body, self.fake))
        todo!()
    }

    /// The result of the `Post` that `send` returned. On success the item joins the sent log
    /// (and the list, as pending) and the attempt is cleared; the UI then empties the field.
    /// On failure the attempt is kept, so a retry of the same text reuses its id.
    ///
    /// Returns `Err` only when the log couldn't be written; the item is still in `items()` for
    /// this session and the UI shows the error like a failed draft save.
    pub fn settle(&mut self, result: Result<(), SendError>) -> io::Result<()> {
        // TODO:
        // match result {
        //   Err(e) => self.delivery = Failed(e),
        //   Ok(()) => { let a = self.attempt.take().expect("settle follows send");
        //               self.delivery = Idle;
        //               if !self.sent.iter().any(|s| s.id == a.id) { self.sent.push(Sent { id: a.id, text: a.text }) }
        //               save_sent(&self.paths, &self.sent)? }
        // }
        todo!()
    }

    pub fn delivery(&self) -> &Delivery {
        &self.delivery
    }

    /// Everything this device sent, newest first, each with its status in the running build.
    pub fn items(&self) -> impl Iterator<Item = Item<'_>> {
        self.sent.iter().rev().map(|sent| Item { sent, status: self.build.status(sent.id) })
    }
}

// ───────────────────────────── the sent log ─────────────────────────────
// `Paths::sent_feedback()` = `<data dir>/sent-feedback.json`: a JSON array of `Sent`, oldest
// first, rewritten whole with `write_atomic` like the draft and settings. One writer (this app on
// this device), so no lock. Loading drops duplicate ids (keeps the first) and unparseable
// entries, so a hand-edited or half-old file never stops the app.

fn load_sent(paths: &Paths) -> Vec<Sent> {
    todo!()
}

fn save_sent(paths: &Paths, sent: &[Sent]) -> io::Result<()> {
    todo!("crate::store::write_atomic(&paths.sent_feedback(), &serde_json::to_vec_pretty(sent)?)")
}

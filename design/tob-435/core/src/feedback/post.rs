//! SKETCH: core/src/feedback/post.rs
//!
//! The one HTTPS request: the body, its signature and the call. Private to `feedback`; the UI
//! sees only `Post::run`'s result.

use super::{FeedbackId, FeedbackLink, SentFrom};
use std::fmt;

/// The header T3 verifies. OPEN QUESTION: must match T3's webhook verifier exactly.
pub(crate) const SIGNATURE_HEADER: &str = "x-inkling-signature-256";

/// The JSON the webhook gets. PRODUCT.md: "writing goes only to the provider the writer picked",
/// so this body never holds draft text or an API key. The type makes that hard to break:
///
/// - The only runtime-text field is `text`, the feedback field's own contents.
/// - Everything else is a number, a `&'static str` (which can't carry text typed at run time) or
///   comes from `Build`, which is fixed at compile time.
/// - It is built only by `Desk::send` from a `SentFrom`, which has the same restriction.
/// - `the_body_holds_only_the_agreed_fields` (tests.rs) pins the exact key set, so adding a field
///   fails a test that names this rule, and has to be a deliberate edit to both.
#[derive(serde::Serialize)]
pub(crate) struct Body<'a> {
    pub id: FeedbackId,
    pub text: &'a str,
    /// RFC 3339 UTC, from `id.sent_at()`: "2026-11-11T22:15:30Z".
    pub sent_at: String,
    /// "mac" or "phone".
    pub form_factor: &'static str,
    pub window: Window,
    /// The screen open when the writer opened feedback: "page", "review" or "keys".
    pub screen: &'static str,
    /// `Build::version`, e.g. "0.1.0".
    pub version: &'static str,
    /// `Build::number`, `null` on a development build.
    pub build: Option<u32>,
}

#[derive(serde::Serialize)]
pub(crate) struct Window {
    pub width: u32,
    pub height: u32,
}

/// A send that's ready to go: everything decided, nothing left but the network.
/// Built by `Desk::send` on the UI thread; `run` blocks, so the UI calls it on a background task.
/// `send` and `settle` are two calls only because the network wait sits between them on GPUI's
/// executor; `#[must_use]` on both ends keeps a caller from dropping either half.
#[must_use = "run it, then pass the result to Desk::settle"]
pub struct Post {
    url: String,
    body: Vec<u8>,
    signature: String,
    fake: bool,
}

/// What the writer reads under the feedback field when a send fails. The typed text stays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// No connection, DNS, timeout.
    Offline,
    /// 401, 403 or 404: the link is wrong or was replaced on T3's side.
    Refused,
    /// Any other non-2xx.
    Server(u16),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Offline => f.write_str("Couldn't reach your Mac. Check the connection and try again."),
            SendError::Refused => f.write_str("The feedback link was refused. Paste a new one on the key screen."),
            SendError::Server(code) => write!(f, "The feedback didn't go through (HTTP {code}). Try again."),
        }
    }
}

impl Post {
    pub(crate) fn new(link: &FeedbackLink, body: &Body<'_>, fake: bool) -> Self {
        let body = serde_json::to_vec(body).expect("the body serializes");
        let signature = link.sign(&body);
        Post { url: link.url().to_string(), body, signature, fake }
    }

    /// Sends once. 2xx is success, including T3's answer to a duplicate id (it dedupes), so a
    /// retry after a lost response is safe.
    ///
    /// Fake (`INKLING_FAKE_AI`, decided in `Desk::open`): no network. Succeeds, except for a URL
    /// whose host ends in `.invalid` (reserved by RFC 2606, so no real link has one), which fails
    /// as `Offline`. That is how both snap scripts show a failed send.
    #[must_use = "pass it to Desk::settle"]
    pub fn run(self) -> Result<(), SendError> {
        // TODO:
        // if self.fake { return if host(&self.url).ends_with(".invalid") { Err(Offline) } else { Ok(()) } }
        // ureq agent: http_status_as_error(false), timeout_connect 10 s, timeout_global 20 s.
        // POST self.url, content-type application/json, SIGNATURE_HEADER: self.signature,
        //   body: self.body exactly (the bytes that were signed).
        // transport error → Offline (same mapping as providers::transport);
        // 200..300 → Ok; 401 | 403 | 404 → Refused; other → Server(code).
        todo!()
    }
}

/// `unix seconds` → "YYYY-MM-DDTHH:MM:SSZ". Days-from-civil inverse (Howard Hinnant); no date crate.
pub(crate) fn rfc3339(unix: u64) -> String {
    todo!()
}

//! SKETCH: core/src/feedback/link.rs
//!
//! The feedback link: the one string Toby pastes on each device. It carries the T3 webhook URL
//! and the HMAC signing secret together, as a URL whose fragment holds the secret:
//!
//! ```text
//! https://<t3 webhook host>/<path>#k=<secret>
//! ```
//!
//! A URL fragment is never part of an HTTP request, and `Post` (post.rs) is built from `url`
//! alone, so the secret can only leave the device as the HMAC it keys. Composing a link from what
//! T3 shows: append `#k=` and the webhook's secret to its URL.
//!
//! Parsed once, here. Inside the crate a `FeedbackLink` is trusted (per boundary-discipline).

use std::fmt;

/// The shortest secret accepted, in chars. 32 chars of base64url or hex is 128+ bits.
const MIN_SECRET: usize = 32;

/// A parsed feedback link. No field is public: the secret is reachable only through `sign`,
/// and the URL only through `url()`, which never includes the fragment.
#[derive(Clone, PartialEq, Eq)]
pub struct FeedbackLink {
    /// `https://…` without the fragment. Plain `http` only for a loopback host, so core's tests
    /// can post to a local mock server.
    url: String,
    secret: String,
}

/// Why a pasted string isn't a link. The message is shown on the key screen as is, and never
/// quotes what was pasted (it may hold the secret).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BadLink {
    NotAUrl,
    NotHttps,
    NoSecret,
    ShortSecret,
}

impl fmt::Display for BadLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BadLink::NotAUrl => "That isn't a feedback link. Paste the whole link, starting with https://.",
            BadLink::NotHttps => "Feedback links start with https://.",
            BadLink::NoSecret => "That link has no secret. It should end with #k= and the secret.",
            BadLink::ShortSecret => "That link's secret is too short.",
        })
    }
}

impl FeedbackLink {
    /// The boundary. Trims whitespace (a paste often carries a newline).
    pub fn parse(pasted: &str) -> Result<Self, BadLink> {
        // TODO:
        // - split at the first '#'; no '#' → NoSecret
        // - left must start "https://" with a non-empty host, or "http://127.0.0.1[:port]" /
        //   "http://localhost[:port]"; "http://" anything else → NotHttps; otherwise NotAUrl.
        //   No whitespace, no control chars.
        // - right must be exactly "k=<secret>", secret in [A-Za-z0-9_-]; empty → NoSecret,
        //   shorter than MIN_SECRET → ShortSecret
        todo!()
    }

    /// The string `parse` reads back. What `Desk::save_link` stores in the key store.
    pub(crate) fn to_stored(&self) -> String {
        format!("{}#k={}", self.url, self.secret)
    }

    /// Where to POST. Never contains the secret.
    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    /// `sha256=<lowercase hex of HMAC-SHA256(secret bytes, body)>`, the signature header value.
    /// T3 verifies it the GitHub way: the key is the secret string's UTF-8 bytes.
    pub(crate) fn sign(&self, body: &[u8]) -> String {
        // TODO: ring::hmac::sign(&Key::new(HMAC_SHA256, self.secret.as_bytes()), body), hex.
        todo!()
    }
}

/// Redacted, so a `{:?}` in a log, a panic or a snap's FAIL line can't leak the secret.
impl fmt::Debug for FeedbackLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedbackLink").field("url", &self.url).field("secret", &"<redacted>").finish()
    }
}

// Deliberately no `Display`, no `Serialize` and no public accessor for the secret.

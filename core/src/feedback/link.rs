//! The feedback link: the one string the writer pastes on each device. It carries the T3 webhook
//! URL and the signing secret together, as a URL whose fragment holds the secret:
//!
//! ```text
//! https://<webhook host>/<path>#k=<secret>
//! ```
//!
//! No HTTP client sends a URL's fragment, and `Post` is built from `url()` alone, so the secret
//! leaves the device only as the HMAC it keys.

use std::fmt;

/// The shortest secret accepted, in chars. 32 chars of base64url or hex is 128 bits or more.
const MIN_SECRET: usize = 32;

/// A parsed feedback link. The secret is reachable only through `sign`, and the URL only through
/// `url()`, which never includes the fragment.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FeedbackLink {
    url: String,
    secret: String,
}

/// Why a pasted string isn't a link. Shown on the key screen as is. It never quotes what was
/// pasted, which may hold the secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BadLink {
    NotAUrl,
    NotHttps,
    NoSecret,
    BadSecret,
    ShortSecret,
}

impl fmt::Display for BadLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BadLink::NotAUrl => "That isn't a feedback link. Paste the whole link, starting with https://.",
            BadLink::NotHttps => "Feedback links start with https://.",
            BadLink::NoSecret => "That link has no secret. It should end with #k= and the secret.",
            BadLink::BadSecret => "That link's secret has characters a secret can't have.",
            BadLink::ShortSecret => "That link's secret is too short.",
        })
    }
}

impl FeedbackLink {
    /// Plain `http` is refused, except for a loopback host in core's own tests, which post to a
    /// local mock server.
    pub fn parse(pasted: &str) -> Result<Self, BadLink> {
        let pasted = pasted.trim();
        let rest = if let Some(rest) = pasted.strip_prefix("https://") {
            rest
        } else if let Some(rest) = pasted.strip_prefix("http://") {
            if !(cfg!(test) && rest.starts_with("127.0.0.1")) {
                return Err(BadLink::NotHttps);
            }
            rest
        } else {
            return Err(BadLink::NotAUrl);
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host_ok = !host.is_empty() && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b));
        if !host_ok || !pasted.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(BadLink::NotAUrl);
        }
        let (url, fragment) = pasted.split_once('#').ok_or(BadLink::NoSecret)?;
        let secret = fragment.strip_prefix("k=").ok_or(BadLink::NoSecret)?;
        if secret.is_empty() {
            return Err(BadLink::NoSecret);
        }
        if !secret.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)) {
            return Err(BadLink::BadSecret);
        }
        if secret.len() < MIN_SECRET {
            return Err(BadLink::ShortSecret);
        }
        Ok(FeedbackLink { url: url.to_string(), secret: secret.to_string() })
    }

    /// The string `parse` reads back, which the key store keeps.
    pub fn to_stored(&self) -> String {
        format!("{}#k={}", self.url, self.secret)
    }

    /// Where to POST. Never contains the secret.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `sha256=<lowercase hex of HMAC-SHA256(secret, body)>`, keyed by the secret's UTF-8 bytes.
    pub fn sign(&self, body: &[u8]) -> String {
        sign(self.secret.as_bytes(), body)
    }
}

pub(crate) fn sign(key: &[u8], body: &[u8]) -> String {
    let tag = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key), body);
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

/// Redacted, so a `{:?}` in a log, a panic or a snap's FAIL line can't leak the secret.
impl fmt::Debug for FeedbackLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedbackLink").field("url", &self.url).field("secret", &"<redacted>").finish()
    }
}

//! The one HTTPS request: the body, its signature and the call.

use super::{FeedbackId, FeedbackLink};
use std::fmt;
use std::time::Duration;

/// The header the webhook verifies: `sha256=<lowercase hex>`.
pub(crate) const SIGNATURE_HEADER: &str = "x-inkling-signature";

/// The JSON the webhook gets. PRODUCT.md: writing goes only to the provider the writer picked,
/// so this body never holds draft text or an API key. The only text typed at run time is `text`,
/// the feedback field's own contents. Every other field is a number, a `&'static str` or comes
/// from `Build`. `the_body_holds_only_the_agreed_fields` pins the key set.
#[derive(serde::Serialize)]
pub(crate) struct Body<'a> {
    pub id: FeedbackId,
    pub text: &'a str,
    /// RFC 3339 UTC, from the id.
    pub sent_at: String,
    pub form_factor: &'static str,
    pub window: Window,
    pub screen: &'static str,
    pub version: &'static str,
    /// `null` on a development build.
    pub build: Option<u32>,
}

#[derive(serde::Serialize)]
pub(crate) struct Window {
    pub width: u32,
    pub height: u32,
}

/// A send with everything decided and nothing left but the network. `run` blocks, so the UI
/// calls it on a background task and hands its result to `Desk::settle`.
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
    /// 401, 403 or 404: the link is wrong or was replaced.
    Refused,
    /// Any other answer that isn't 2xx, redirects included.
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

    /// Sends once. Any 2xx is success, including the webhook's answer to an id it has seen, so a
    /// retry after a lost answer is safe.
    ///
    /// A fake post never touches the network. It succeeds, except for a host under `.invalid`
    /// (reserved by RFC 2606, so no real link has one), which fails as `Offline`. That is how the
    /// snap scripts show a failed send.
    #[must_use = "pass it to Desk::settle"]
    pub fn run(self) -> Result<(), SendError> {
        if self.fake {
            let host = self.url.split('/').nth(2).unwrap_or_default();
            let host = host.split(':').next().unwrap_or_default();
            return if host.ends_with(".invalid") { Err(SendError::Offline) } else { Ok(()) };
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        let response = agent
            .post(&self.url)
            .header("content-type", "application/json")
            .header(SIGNATURE_HEADER, &self.signature)
            .send(&self.body[..])
            .map_err(|_| SendError::Offline)?;
        match response.status().as_u16() {
            200..300 => Ok(()),
            401 | 403 | 404 => Err(SendError::Refused),
            code => Err(SendError::Server(code)),
        }
    }

    #[cfg(test)]
    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    #[cfg(test)]
    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, by Howard Hinnant's civil-from-days.
pub(crate) fn rfc3339(unix: u64) -> String {
    let (days, secs) = (unix / 86_400, unix % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", secs / 3_600, secs % 3_600 / 60, secs % 60)
}

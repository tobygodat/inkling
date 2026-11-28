//! Feedback from the native app's dev builds (TOB-571): the clip of the last 10 seconds, when one
//! is attached, then the record, to `server/src/feedback.ts`, which keeps both and posts the
//! inkling Captain a notice. The record keeps the GPUI feedback body's fields (`core::feedback`)
//! and adds where in the app it was sent from, the bundle, and what the clip holds.

use crate::api::{Api, Body};
use crate::{Error, Method, SignIn, Transport};
use inkling_core::feedback::FeedbackId;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Feedback {
    pub id: FeedbackId,
    pub text: String,
    /// "phone" or "mac".
    pub form_factor: String,
    pub window: Window,
    /// The tab, or the reader or writer open in it.
    pub screen: String,
    /// Where that is, in words: the item and how far through, or the document's id.
    pub place: String,
    pub version: String,
    pub build: String,
    pub bundle: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Window {
    pub width: u32,
    pub height: u32,
}

/// What the attached clip holds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Clip {
    pub seconds: f64,
    pub touches: u32,
}

#[derive(Serialize)]
struct Record<'a> {
    #[serde(flatten)]
    feedback: &'a Feedback,
    sent_at: String,
    clip: Option<Clip>,
}

/// Sends one feedback. A retry with the same id is safe: the server keeps the first and tells the
/// Captain once, and a clip it answers 409 to belongs to a record it already has.
pub fn send(transport: &dyn Transport, sign_in: &mut SignIn, feedback: &Feedback, clip: Option<(Clip, &[u8])>) -> Result<(), Error> {
    let token = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, token);
    if let Some((_, bytes)) = clip {
        let answer = api.answer(Method::Put, &format!("/v1/feedback/{}/clip", feedback.id), Body::Bytes(bytes))?;
        if !(200..300).contains(&answer.status) && answer.status != 409 {
            return Err(crate::api::refused(&answer));
        }
    }
    let record = Record { feedback, sent_at: feedback.id.sent_at_rfc3339(), clip: clip.map(|(c, _)| c) };
    let json = serde_json::to_value(&record).map_err(|e| Error::Local(e.to_string()))?;
    api.send(Method::Post, "/v1/feedback", Body::Json(json)).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FAKE_SERVER, Fake, account, fake_way_back};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn the_clip_and_then_the_record_reach_the_server_once() {
        let fake = Fake::memory();
        let begun = account::begin(&fake, FAKE_SERVER, "inkling://callback", "test").unwrap();
        let mut sign_in = begun.finish(&fake, &fake_way_back(&begun.url).unwrap()).unwrap();
        let id = FeedbackId::new(UNIX_EPOCH + Duration::from_secs(1_794_460_000));
        let feedback = Feedback {
            id,
            text: "The handle covers the scroll bar.".into(),
            form_factor: "phone".into(),
            window: Window { width: 402, height: 874 },
            screen: "reader".into(),
            place: "Inbox › Walking, 34%".into(),
            version: "0.1.0".into(),
            build: "20261128.0630".into(),
            bundle: "com.tobygodat.inkling.dev".into(),
        };
        let clip = Clip { seconds: 9.8, touches: 12 };

        send(&fake, &mut sign_in, &feedback, Some((clip, b"ftyp clip"))).unwrap();
        send(&fake, &mut sign_in, &feedback, Some((clip, b"ftyp clip"))).unwrap();

        let (records, clip_bytes) = fake.with(|s| (s.feedback.clone(), s.file(&format!("feedback/{id}/clip.mp4"))));
        assert_eq!(records.len(), 1, "a retry of the same id is kept once");
        let mut fields: Vec<&str> = records[0].as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["build", "bundle", "clip", "form_factor", "id", "place", "screen", "sent_at", "text", "version", "window"]);
        assert_eq!(records[0]["sent_at"], "2026-11-12T05:06:40Z");
        assert_eq!(records[0]["clip"], serde_json::json!({ "seconds": 9.8, "touches": 12 }));
        assert_eq!(clip_bytes.as_deref(), Some(&b"ftyp clip"[..]));
    }
}

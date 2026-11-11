//! The three live providers. Each one is a [`Wire`]: how to build its request, how to read one
//! streamed event, and how to read an HTTP error. This module owns the HTTP call, the event
//! loop, cancelling, and turning failures into messages a writer can act on.

mod claude;
mod gemini;
mod openai;
mod sse;
#[cfg(test)]
mod tests;

use crate::ai::{Cancel, Prompt, Provider};
use std::io::{BufRead, BufReader};
use std::time::Duration;

/// The default model per provider: each one's current model for good prose with a fast first token.
/// Claude Sonnet 5.5 (Anthropic's model table), GPT-6.1 Terra (OpenAI: "clear, natural writing"),
/// Gemini 3.8 Flash (Google: recommended for new projects, low thinking for drafts).
pub fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude-sonnet-5-5",
        Provider::Gemini => "gemini-3.8-flash",
        Provider::ChatGpt => "gpt-6.1-terra",
    }
}

/// Blocks until the answer is complete, calling `on_text` with each piece.
/// Returns early with `Ok` when `cancel` is set.
pub fn stream(
    provider: Provider,
    model: &str,
    key: &str,
    prompt: &Prompt,
    cancel: &Cancel,
    on_text: &mut dyn FnMut(String),
) -> Result<(), String> {
    stream_at(wire(provider).base_url, provider, model, key, prompt, cancel, on_text)
}

/// [`stream`] against another server, such as a local mock. `base_url` has no trailing slash.
pub fn stream_at(
    base_url: &str,
    provider: Provider,
    model: &str,
    key: &str,
    prompt: &Prompt,
    cancel: &Cancel,
    on_text: &mut dyn FnMut(String),
) -> Result<(), String> {
    let key = key.trim();
    run(base_url, wire(provider), model.trim(), key, prompt, cancel, on_text)
        .map_err(|failure| redact(failure.message(provider), key))
}

/// What went wrong, in the terms a writer can act on.
#[derive(Clone, Debug, PartialEq)]
enum Failure {
    BadKey,
    RateLimited,
    Busy,
    Offline,
    UnknownModel(String),
    Declined,
    Cut,
    Provider(String),
}

impl Failure {
    fn message(&self, provider: Provider) -> String {
        let p = provider.label();
        match self {
            Failure::BadKey => format!("{p} didn't accept your API key. Check it in Settings."),
            Failure::RateLimited => format!("{p} is rate-limiting your key. Wait a moment, then try again."),
            Failure::Busy => format!("{p} is busy right now. Try again in a moment."),
            Failure::Offline => format!("Can't reach {p}. Check your internet connection."),
            Failure::UnknownModel(model) => {
                format!("{p} has no model called \"{model}\". Check the model in Settings.")
            }
            Failure::Declined => format!("{p} declined to write this. Try rewording the instruction."),
            Failure::Cut => format!("The connection to {p} dropped before the answer finished. Try again."),
            Failure::Provider(message) => format!("{p}: {message}"),
        }
    }

    /// Anything without a more specific meaning: the provider's own words, else the status.
    fn http(status: u16, message: Option<String>) -> Failure {
        let message = message.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        Failure::Provider(message.unwrap_or_else(|| match status {
            0 => "sent an error without saying why.".into(),
            _ => format!("HTTP {status}"),
        }))
    }
}

/// Providers sometimes echo part of a key in an error. None of it reaches the writer or a log.
fn redact(message: String, key: &str) -> String {
    if key.len() < 8 { message } else { message.replace(key, "[key]") }
}

/// The request a provider wants, ready to send.
struct Request {
    url: String,
    headers: Vec<(&'static str, String)>,
    body: String,
}

/// One streamed event, read: text to show, and whether the answer is complete.
#[derive(Debug, Default, PartialEq)]
struct Chunk {
    text: String,
    done: bool,
}

impl Chunk {
    const NOTHING: Chunk = Chunk { text: String::new(), done: false };
    const DONE: Chunk = Chunk { text: String::new(), done: true };

    fn text(text: String) -> Chunk {
        Chunk { text, done: false }
    }

    fn last(text: String) -> Chunk {
        Chunk { text, done: true }
    }
}

/// One provider's wire format. `model` is passed so errors can name it.
struct Wire {
    base_url: &'static str,
    request: fn(base_url: &str, model: &str, key: &str, prompt: &Prompt) -> Result<Request, Failure>,
    event: fn(event: &sse::Event, model: &str) -> Result<Chunk, Failure>,
    http_error: fn(status: u16, body: &str, model: &str) -> Failure,
}

fn wire(provider: Provider) -> &'static Wire {
    match provider {
        Provider::Claude => &claude::WIRE,
        Provider::Gemini => &gemini::WIRE,
        Provider::ChatGpt => &openai::WIRE,
    }
}

fn run(
    base_url: &str,
    wire: &Wire,
    model: &str,
    key: &str,
    prompt: &Prompt,
    cancel: &Cancel,
    on_text: &mut dyn FnMut(String),
) -> Result<(), Failure> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Failure::BadKey);
    }
    let request = (wire.request)(base_url, model, key, prompt)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        // A cancel can't interrupt a blocked read, so a stalled stream ends here instead of never.
        .timeout_recv_body(Some(Duration::from_secs(300)))
        .build()
        .into();
    let mut call = agent.post(&request.url).header("content-type", "application/json");
    for (name, value) in &request.headers {
        call = call.header(*name, value.as_str());
    }
    let response = call.send(request.body.as_bytes());
    if cancel.is_cancelled() {
        return Ok(());
    }
    let mut response = response.map_err(transport)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let body = response.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
        return Err((wire.http_error)(status, &body, model));
    }
    let mut reader = BufReader::new(response.into_body().into_reader());
    drain(wire, &mut reader, model, cancel, on_text)
}

/// Reads events until the answer is complete, the stream fails, or `cancel` is set.
fn drain(
    wire: &Wire,
    reader: &mut impl BufRead,
    model: &str,
    cancel: &Cancel,
    on_text: &mut dyn FnMut(String),
) -> Result<(), Failure> {
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let event = match sse::next(reader) {
            _ if cancel.is_cancelled() => return Ok(()),
            Ok(Some(event)) => event,
            Ok(None) | Err(_) => return Err(Failure::Cut),
        };
        let chunk = (wire.event)(&event, model)?;
        if !chunk.text.is_empty() {
            on_text(chunk.text);
        }
        if chunk.done {
            return Ok(());
        }
    }
}

fn transport(error: ureq::Error) -> Failure {
    match error {
        ureq::Error::Io(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Timeout(_) => Failure::Offline,
        other => Failure::Provider(other.to_string()),
    }
}

/// Parses one streamed event's JSON. A body that isn't the documented shape is the provider's
/// problem, not the writer's, so it reads as a provider error.
fn parse<'a, T: serde::Deserialize<'a>>(data: &'a str) -> Result<T, Failure> {
    serde_json::from_str(data).map_err(|e| Failure::Provider(format!("sent a reply Inkling can't read ({e}).")))
}

/// Model IDs go into Gemini's URL path, so anything outside this set is refused before sending.
fn model_is_plain(model: &str) -> bool {
    !model.is_empty() && model.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
}

/// Feeds `body` through a provider's event loop, as the tests do with doc-derived streams.
#[cfg(test)]
fn replay(provider: Provider, body: &str) -> (String, Result<(), Failure>) {
    let mut text = String::new();
    let mut reader = std::io::Cursor::new(body.as_bytes());
    let result = drain(wire(provider), &mut reader, "m", &Cancel::default(), &mut |t| text.push_str(&t));
    (text, result)
}

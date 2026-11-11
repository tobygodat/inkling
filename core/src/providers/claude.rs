//! Anthropic's Messages API, streamed (`anthropic-version: 2026-10-21`). Wire shapes from the
//! claude-api skill's `curl/examples.md` (Streaming, Refusal Fallbacks) and `shared/error-codes.md`.

use super::{Chunk, Failure, Request, Wire, parse, sse};
use crate::ai::Prompt;
use serde::{Deserialize, Serialize};

pub(super) const WIRE: Wire =
    Wire { base_url: "https://api.anthropic.com", request, event, http_error };

/// Models that take server-side `fallbacks: "default"`, so a declined request is retried on
/// another Claude model inside the same call instead of failing.
const FALLBACK_MODELS: [&str; 5] =
    ["claude-sonnet-5-5", "claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-fable-5-2"];
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    system: &'a str,
    messages: [Message<'a>; 1],
    output_config: OutputConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallbacks: Option<&'static str>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct OutputConfig {
    effort: &'static str,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    ContentBlockDelta { delta: Delta },
    MessageDelta { delta: MessageDelta },
    MessageStop,
    Error { error: ApiError },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Delta {
    TextDelta { text: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct MessageDelta {
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
}

fn request(base_url: &str, model: &str, key: &str, prompt: &Prompt) -> Result<Request, Failure> {
    let fallback = FALLBACK_MODELS.contains(&model);
    let body = Body {
        model,
        max_tokens: 8192,
        stream: true,
        system: &prompt.system,
        messages: [Message { role: "user", content: &prompt.user }],
        // Low effort: an inline partner writes short passages and should start fast.
        output_config: OutputConfig { effort: "low" },
        fallbacks: fallback.then_some("default"),
    };
    let mut headers = vec![("x-api-key", key.to_string()), ("anthropic-version", "2026-10-21".to_string())];
    if fallback {
        headers.push(("anthropic-beta", FALLBACK_BETA.to_string()));
    }
    Ok(Request {
        url: format!("{base_url}/v1/messages"),
        headers,
        body: serde_json::to_string(&body).expect("the request body serializes"),
    })
}

fn event(event: &sse::Event, model: &str) -> Result<Chunk, Failure> {
    Ok(match parse(&event.data)? {
        Event::ContentBlockDelta { delta: Delta::TextDelta { text } } => Chunk::text(text),
        Event::MessageDelta { delta: MessageDelta { stop_reason: Some(reason) } } if reason == "refusal" => {
            return Err(Failure::Declined);
        }
        Event::MessageStop => Chunk::DONE,
        Event::Error { error } => return Err(api_error(error, model, None)),
        _ => Chunk::NOTHING,
    })
}

fn http_error(status: u16, body: &str, model: &str) -> Failure {
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(ErrorBody { error }) => api_error(error, model, Some(status)),
        Err(_) => status_failure(status, model, None),
    }
}

fn api_error(error: ApiError, model: &str, status: Option<u16>) -> Failure {
    match error.kind.as_str() {
        "authentication_error" => Failure::BadKey,
        "rate_limit_error" => Failure::RateLimited,
        "not_found_error" => Failure::UnknownModel(model.into()),
        "overloaded_error" => Failure::Busy,
        _ => status_failure(status.unwrap_or(0), model, Some(error.message)),
    }
}

fn status_failure(status: u16, model: &str, message: Option<String>) -> Failure {
    match status {
        401 => Failure::BadKey,
        404 => Failure::UnknownModel(model.into()),
        429 => Failure::RateLimited,
        503 | 529 => Failure::Busy,
        _ => Failure::http(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Failure, replay};
    use crate::ai::Provider;

    /// The stream from Anthropic's streaming docs, with a second text delta and a ping.
    const GOOD: &str = "event: message_start\n\
        data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-sonnet-5-5\",\"stop_reason\":null,\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
        event: content_block_start\n\
        data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n\
        event: content_block_start\n\
        data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
        event: ping\n\
        data: {\"type\": \"ping\"}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\", world\"}}\n\n\
        event: content_block_stop\n\
        data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
        event: message_delta\n\
        data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":12}}\n\n\
        event: message_stop\n\
        data: {\"type\":\"message_stop\"}\n\n";

    #[test]
    fn text_deltas_stream_and_message_stop_finishes() {
        assert_eq!(replay(Provider::Claude, GOOD), ("Hello, world".into(), Ok(())));
    }

    #[test]
    fn an_error_event_mid_stream_fails_with_its_meaning() {
        let body = "event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n\
            event: error\n\
            data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let (text, result) = replay(Provider::Claude, body);
        assert_eq!(text, "Hi");
        assert_eq!(result, Err(Failure::Busy));
    }

    #[test]
    fn a_refusal_stop_reason_is_a_decline() {
        let body = "event: message_delta\n\
            data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"},\"usage\":{\"output_tokens\":0}}\n\n";
        assert_eq!(replay(Provider::Claude, body).1, Err(Failure::Declined));
    }

    #[test]
    fn a_stream_that_ends_without_message_stop_was_cut() {
        assert_eq!(replay(Provider::Claude, &GOOD[..GOOD.find("event: message_stop").unwrap()]).1, Err(Failure::Cut));
    }

    #[test]
    fn http_errors_map_to_what_the_writer_can_do() {
        let body = |kind: &str| format!("{{\"type\":\"error\",\"error\":{{\"type\":\"{kind}\",\"message\":\"m\"}},\"request_id\":\"r\"}}");
        assert_eq!(super::http_error(401, &body("authentication_error"), "m"), Failure::BadKey);
        assert_eq!(super::http_error(429, &body("rate_limit_error"), "m"), Failure::RateLimited);
        assert_eq!(super::http_error(404, &body("not_found_error"), "x"), Failure::UnknownModel("x".into()));
        assert_eq!(super::http_error(400, &body("invalid_request_error"), "m"), Failure::Provider("m".into()));
        assert_eq!(super::http_error(502, "<html>", "m"), Failure::Provider("HTTP 502".into()));
    }
}

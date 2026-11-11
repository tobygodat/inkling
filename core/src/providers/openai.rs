//! OpenAI's Responses API, streamed. Shapes from developers.openai.com: `docs/guides/text`
//! (`POST /v1/responses`, bearer auth, `instructions`, `reasoning.effort`), the Responses
//! streaming-events reference, and `docs/guides/error-codes`.

use super::{Chunk, Failure, Request, Wire, parse, sse};
use crate::ai::Prompt;
use serde::{Deserialize, Serialize};

pub(super) const WIRE: Wire = Wire { base_url: "https://api.openai.com", request, event, http_error };

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    instructions: &'a str,
    input: &'a str,
    stream: bool,
    store: bool,
    max_output_tokens: u32,
    reasoning: Reasoning,
}

#[derive(Serialize)]
struct Reasoning {
    effort: &'static str,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "response.output_text.delta")]
    TextDelta { delta: String },
    #[serde(rename = "response.refusal.delta")]
    RefusalDelta,
    #[serde(rename = "response.completed")]
    Completed,
    #[serde(rename = "response.incomplete")]
    Incomplete { response: Response },
    #[serde(rename = "response.failed")]
    Failed { response: Response },
    #[serde(rename = "error")]
    Error {
        code: Option<String>,
        #[serde(default)]
        message: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Response {
    error: Option<ResponseError>,
    incomplete_details: Option<IncompleteDetails>,
}

#[derive(Deserialize)]
struct ResponseError {
    code: Option<String>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct IncompleteDetails {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    code: Option<String>,
}

fn request(base_url: &str, model: &str, key: &str, prompt: &Prompt) -> Result<Request, Failure> {
    let body = Body {
        model,
        instructions: &prompt.system,
        input: &prompt.user,
        stream: true,
        store: false,
        max_output_tokens: 8192,
        // `low` is the lowest effort every current GPT-6 model accepts, and keeps the first token fast.
        reasoning: Reasoning { effort: "low" },
    };
    Ok(Request {
        url: format!("{base_url}/v1/responses"),
        headers: vec![("authorization", format!("Bearer {key}"))],
        body: serde_json::to_string(&body).expect("the request body serializes"),
    })
}

fn event(event: &sse::Event, model: &str) -> Result<Chunk, Failure> {
    Ok(match parse(&event.data)? {
        Event::TextDelta { delta } => Chunk::text(delta),
        Event::RefusalDelta => return Err(Failure::Declined),
        Event::Completed => Chunk::DONE,
        Event::Incomplete { response } => {
            match response.incomplete_details.and_then(|d| d.reason).as_deref() {
                Some("max_output_tokens") => Chunk::DONE,
                Some("content_filter") => return Err(Failure::Declined),
                Some(reason) => return Err(Failure::Provider(format!("stopped early ({reason})."))),
                None => return Err(Failure::Provider("stopped early.".into())),
            }
        }
        Event::Failed { response } => {
            let error = response.error.unwrap_or(ResponseError { code: None, message: String::new() });
            return Err(coded(error.code.as_deref(), error.message, model));
        }
        Event::Error { code, message } => return Err(coded(code.as_deref(), message, model)),
        Event::Other => Chunk::NOTHING,
    })
}

/// An error that arrived inside the stream, read by its code.
fn coded(code: Option<&str>, message: String, model: &str) -> Failure {
    match code {
        Some("rate_limit_exceeded") => Failure::RateLimited,
        Some("model_not_found") => Failure::UnknownModel(model.into()),
        Some("invalid_api_key") => Failure::BadKey,
        _ => Failure::http(0, Some(message).filter(|m| !m.is_empty()).or(code.map(String::from))),
    }
}

fn http_error(status: u16, body: &str, model: &str) -> Failure {
    let error = serde_json::from_str::<ErrorBody>(body).ok().map(|b| b.error);
    let kind = error.as_ref().and_then(|e| e.kind.as_deref());
    let code = error.as_ref().and_then(|e| e.code.as_deref());
    match (status, kind, code) {
        (401, ..) | (_, _, Some("invalid_api_key")) => Failure::BadKey,
        (_, _, Some("model_not_found")) | (404, ..) => Failure::UnknownModel(model.into()),
        // Billing and quota errors are 429s too, but waiting won't fix them: show OpenAI's words.
        (429, Some("insufficient_quota"), _) => Failure::http(status, error.map(|e| e.message)),
        (429, ..) => Failure::RateLimited,
        (503, ..) => Failure::Busy,
        _ => Failure::http(status, error.map(|e| e.message)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Failure, replay};
    use crate::ai::Provider;

    /// The documented event sequence for a short text answer, with the reference's example
    /// `response.output_text.delta` payload.
    const GOOD: &str = "event: response.created\n\
        data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"object\":\"response\",\"status\":\"in_progress\",\"error\":null,\"incomplete_details\":null,\"output\":[]},\"sequence_number\":0}\n\n\
        event: response.output_text.delta\n\
        data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_123\",\"output_index\":0,\"content_index\":0,\"delta\":\"In\",\"sequence_number\":1,\"logprobs\":[]}\n\n\
        event: response.output_text.delta\n\
        data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_123\",\"output_index\":0,\"content_index\":0,\"delta\":\" the end\",\"sequence_number\":2,\"logprobs\":[]}\n\n\
        event: response.output_text.done\n\
        data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_123\",\"output_index\":0,\"content_index\":0,\"text\":\"In the end\",\"sequence_number\":3,\"logprobs\":[]}\n\n\
        event: response.completed\n\
        data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"object\":\"response\",\"status\":\"completed\",\"error\":null,\"incomplete_details\":null},\"sequence_number\":4}\n\n";

    #[test]
    fn text_deltas_stream_and_completed_finishes() {
        assert_eq!(replay(Provider::ChatGpt, GOOD), ("In the end".into(), Ok(())));
    }

    #[test]
    fn a_stream_that_ends_without_completed_was_cut() {
        let cut = &GOOD[..GOOD.find("event: response.completed").unwrap()];
        assert_eq!(replay(Provider::ChatGpt, cut), ("In the end".into(), Err(Failure::Cut)));
    }

    #[test]
    fn the_documented_error_event_fails_with_its_message() {
        let body = "event: error\n\
            data: {\"type\":\"error\",\"code\":\"ERR_SOMETHING\",\"message\":\"Something went wrong\",\"param\":null,\"sequence_number\":1}\n\n";
        assert_eq!(replay(Provider::ChatGpt, body).1, Err(Failure::Provider("Something went wrong".into())));
    }

    #[test]
    fn failed_and_incomplete_responses_read_by_their_reason() {
        let failed = |code: &str| {
            format!("data: {{\"type\":\"response.failed\",\"response\":{{\"status\":\"failed\",\"error\":{{\"code\":\"{code}\",\"message\":\"The model failed to generate a response.\"}},\"incomplete_details\":null}},\"sequence_number\":1}}\n\n")
        };
        assert_eq!(replay(Provider::ChatGpt, &failed("rate_limit_exceeded")).1, Err(Failure::RateLimited));
        assert_eq!(
            replay(Provider::ChatGpt, &failed("server_error")).1,
            Err(Failure::Provider("The model failed to generate a response.".into()))
        );
        let incomplete = |reason: &str| {
            format!("data: {{\"type\":\"response.incomplete\",\"response\":{{\"status\":\"incomplete\",\"error\":null,\"incomplete_details\":{{\"reason\":\"{reason}\"}}}},\"sequence_number\":1}}\n\n")
        };
        assert_eq!(replay(Provider::ChatGpt, &incomplete("max_output_tokens")).1, Ok(()));
        assert_eq!(replay(Provider::ChatGpt, &incomplete("content_filter")).1, Err(Failure::Declined));
    }

    #[test]
    fn a_refusal_is_a_decline() {
        let body = "data: {\"type\":\"response.refusal.delta\",\"item_id\":\"msg_123\",\"output_index\":0,\"content_index\":0,\"delta\":\"I can't\",\"sequence_number\":1}\n\n";
        assert_eq!(replay(Provider::ChatGpt, body).1, Err(Failure::Declined));
    }

    #[test]
    fn http_errors_map_to_what_the_writer_can_do() {
        let body = |kind: &str, code: &str| {
            format!("{{\"error\":{{\"message\":\"m\",\"type\":\"{kind}\",\"param\":null,\"code\":\"{code}\"}}}}")
        };
        assert_eq!(super::http_error(401, &body("invalid_request_error", "invalid_api_key"), "x"), Failure::BadKey);
        assert_eq!(super::http_error(429, &body("requests", "rate_limit_exceeded"), "x"), Failure::RateLimited);
        assert_eq!(
            super::http_error(429, &body("insufficient_quota", "insufficient_quota"), "x"),
            Failure::Provider("m".into())
        );
        assert_eq!(
            super::http_error(404, &body("invalid_request_error", "model_not_found"), "gpt-x"),
            Failure::UnknownModel("gpt-x".into())
        );
        assert_eq!(super::http_error(503, "", "x"), Failure::Busy);
        assert_eq!(super::http_error(400, &body("invalid_request_error", "bad"), "x"), Failure::Provider("m".into()));
    }
}

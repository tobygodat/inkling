//! Google's Gemini API, `models/{model}:streamGenerateContent?alt=sse`. Shapes from
//! aistudio.google.com: `docs/text-generation?generate-content=true` (REST streaming, the
//! `x-goog-api-key` header, `system_instruction`, `thinkingConfig.thinkingLevel`), the
//! `api-reference/generate-content` response fields, and `docs/api-errors?generate-content=true`.
//! Google now calls `generateContent` legacy in favour of its Interactions API, but still supports it.

use super::{Chunk, Failure, Request, Wire, model_is_plain, parse, sse};
use crate::ai::Prompt;
use serde::{Deserialize, Serialize};

pub(super) const WIRE: Wire =
    Wire { base_url: "https://generativelanguage.googleapis.com", request, event, http_error };

#[derive(Serialize)]
struct Body<'a> {
    system_instruction: Content<'a>,
    contents: [Content<'a>; 1],
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    parts: [Text<'a>; 1],
}

#[derive(Serialize)]
struct Text<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    max_output_tokens: u32,
    thinking_config: ThinkingConfig,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    thinking_level: &'static str,
}

/// One streamed `GenerateContentResponse`. An `error` can also arrive mid-stream.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    #[serde(default)]
    candidates: Vec<Candidate>,
    prompt_feedback: Option<PromptFeedback>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<CandidateContent>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct CandidateContent {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
struct Part {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    block_reason: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    code: u16,
    #[serde(default)]
    message: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    details: Vec<ErrorDetail>,
}

#[derive(Deserialize)]
struct ErrorDetail {
    reason: Option<String>,
}

fn request(base_url: &str, model: &str, key: &str, prompt: &Prompt) -> Result<Request, Failure> {
    if !model_is_plain(model) {
        return Err(Failure::UnknownModel(model.into()));
    }
    let body = Body {
        system_instruction: Content { role: None, parts: [Text { text: &prompt.system }] },
        contents: [Content { role: Some("user"), parts: [Text { text: &prompt.user }] }],
        generation_config: GenerationConfig {
            max_output_tokens: 8192,
            // Google's guidance: low thinking for latency-critical work such as writing drafts.
            thinking_config: ThinkingConfig { thinking_level: "low" },
        },
    };
    Ok(Request {
        url: format!("{base_url}/v1beta/models/{model}:streamGenerateContent?alt=sse"),
        headers: vec![("x-goog-api-key", key.to_string())],
        body: serde_json::to_string(&body).expect("the request body serializes"),
    })
}

fn event(event: &sse::Event, model: &str) -> Result<Chunk, Failure> {
    let response: Response = parse(&event.data)?;
    if let Some(error) = response.error {
        return Err(api_error(error, model));
    }
    if response.prompt_feedback.and_then(|f| f.block_reason).is_some() {
        return Err(Failure::Declined);
    }
    let Some(candidate) = response.candidates.into_iter().next() else { return Ok(Chunk::NOTHING) };
    let text: String = candidate
        .content
        .into_iter()
        .flat_map(|c| c.parts)
        .filter(|p| !p.thought)
        .filter_map(|p| p.text)
        .collect();
    match candidate.finish_reason.as_deref() {
        None | Some("FINISH_REASON_UNSPECIFIED") => Ok(Chunk::text(text)),
        Some("STOP" | "MAX_TOKENS") => Ok(Chunk::last(text)),
        Some("SAFETY" | "RECITATION" | "LANGUAGE" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII") => {
            Err(Failure::Declined)
        }
        Some(reason) => Err(Failure::Provider(format!("stopped early ({reason})."))),
    }
}

fn http_error(status: u16, body: &str, model: &str) -> Failure {
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(ErrorBody { error }) => api_error(ApiError { code: status, ..error }, model),
        Err(_) => api_error(ApiError { code: status, message: String::new(), status: String::new(), details: vec![] }, model),
    }
}

fn api_error(error: ApiError, model: &str) -> Failure {
    let key_invalid = error.details.iter().any(|d| d.reason.as_deref() == Some("API_KEY_INVALID"));
    match (error.code, error.status.as_str()) {
        _ if key_invalid => Failure::BadKey,
        (401, _) | (_, "UNAUTHENTICATED") => Failure::BadKey,
        (404, _) | (_, "NOT_FOUND") => Failure::UnknownModel(model.into()),
        (429, _) => Failure::RateLimited,
        (503, _) | (_, "UNAVAILABLE") => Failure::Busy,
        // 402 (credits), 403 (blocked or leaked key, restricted project) and the rest explain themselves.
        (status, _) => Failure::http(status, Some(error.message)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Failure, replay};
    use crate::ai::Provider;

    fn chunk(parts: &str, finish: Option<&str>) -> String {
        let finish = finish.map(|f| format!(",\"finishReason\":\"{f}\"")).unwrap_or_default();
        format!(
            "data: {{\"candidates\":[{{\"content\":{{\"parts\":[{parts}],\"role\":\"model\"}}{finish},\"index\":0}}],\
             \"usageMetadata\":{{\"promptTokenCount\":9,\"candidatesTokenCount\":3,\"totalTokenCount\":12}},\
             \"modelVersion\":\"gemini-3.8-flash\"}}\r\n\r\n"
        )
    }

    #[test]
    fn text_parts_stream_thoughts_are_skipped_and_stop_finishes() {
        let body = [
            chunk("{\"text\":\"planning\",\"thought\":true}", None),
            chunk("{\"text\":\"Once upon\"}", None),
            chunk("{\"text\":\" a time\"}", Some("STOP")),
        ]
        .concat();
        assert_eq!(replay(Provider::Gemini, &body), ("Once upon a time".into(), Ok(())));
    }

    #[test]
    fn a_stream_that_ends_without_a_finish_reason_was_cut() {
        assert_eq!(replay(Provider::Gemini, &chunk("{\"text\":\"Once\"}", None)), ("Once".into(), Err(Failure::Cut)));
    }

    #[test]
    fn safety_stops_and_blocked_prompts_are_declines() {
        assert_eq!(replay(Provider::Gemini, &chunk("{\"text\":\"x\"}", Some("SAFETY"))).1, Err(Failure::Declined));
        let blocked = "data: {\"promptFeedback\":{\"blockReason\":\"PROHIBITED_CONTENT\"}}\n\n";
        assert_eq!(replay(Provider::Gemini, blocked).1, Err(Failure::Declined));
    }

    #[test]
    fn an_error_mid_stream_fails_with_its_meaning() {
        let body = [
            chunk("{\"text\":\"Hi\"}", None),
            "data: {\"error\":{\"code\":503,\"message\":\"The model is overloaded.\",\"status\":\"UNAVAILABLE\"}}\n\n".into(),
        ]
        .concat();
        assert_eq!(replay(Provider::Gemini, &body), ("Hi".into(), Err(Failure::Busy)));
    }

    /// The documented `generateContent` error body for a bad key.
    const BAD_KEY: &str = r#"{
      "error": {
        "code": 400,
        "message": "API key not valid. Please pass a valid API key.",
        "status": "INVALID_ARGUMENT",
        "details": [
          {"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "API_KEY_INVALID", "domain": "googleapis.com",
           "metadata": {"service": "generativelanguage.googleapis.com"}},
          {"@type": "type.googleapis.com/google.rpc.LocalizedMessage", "locale": "en-US",
           "message": "API key not valid. Please pass a valid API key."}
        ]
      }
    }"#;

    #[test]
    fn http_errors_map_to_what_the_writer_can_do() {
        let body = |code: u16, status: &str| format!("{{\"error\":{{\"code\":{code},\"message\":\"m\",\"status\":\"{status}\"}}}}");
        assert_eq!(super::http_error(400, BAD_KEY, "x"), Failure::BadKey);
        assert_eq!(super::http_error(429, &body(429, "RESOURCE_EXHAUSTED"), "x"), Failure::RateLimited);
        assert_eq!(super::http_error(404, &body(404, "NOT_FOUND"), "g-x"), Failure::UnknownModel("g-x".into()));
        assert_eq!(super::http_error(403, &body(403, "PERMISSION_DENIED"), "x"), Failure::Provider("m".into()));
        assert_eq!(super::http_error(502, "<html>", "x"), Failure::Provider("HTTP 502".into()));
    }

    #[test]
    fn a_model_that_would_change_the_url_is_refused_before_sending() {
        let prompt = crate::ai::Prompt { system: "s".into(), user: "u".into() };
        for model in ["gemini/../x", "a?b", "a b", ""] {
            assert!(matches!(super::request("http://h", model, "k", &prompt), Err(Failure::UnknownModel(_))));
        }
    }
}

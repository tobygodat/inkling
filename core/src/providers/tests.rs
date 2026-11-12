//! End to end through the real HTTP client, against a local mock server per test.

use super::{Failure, stream_at};
use crate::ai::{Cancel, Prompt, Provider};
use crate::mock::{Reply, serve};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const KEY: &str = "sk-test-0123456789abcdef";

fn prompt() -> Prompt {
    Prompt { system: "Be brief.".into(), user: "Once".into() }
}

fn run(base: &str, provider: Provider, model: &str, cancel_after_first: bool) -> (Vec<String>, Result<(), String>) {
    let cancel = Cancel::default();
    let mut pieces = Vec::new();
    let result = stream_at(base, provider, model, KEY, &prompt(), &cancel.clone(), &mut |text| {
        pieces.push(text);
        if cancel_after_first {
            cancel.cancel();
        }
    });
    (pieces, result)
}

fn sse(data: &str) -> String {
    format!("data: {data}\n\n")
}

/// Per provider: a good stream's events, the text they spell, and a documented 401 body.
fn good(provider: Provider) -> (Vec<String>, &'static str) {
    let events = match provider {
        Provider::Claude => vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"upon\"}}\n\n".into(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" a time\"}}\n\n".into(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into(),
        ],
        Provider::ChatGpt => vec![
            sse("{\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"upon\",\"sequence_number\":1}"),
            sse("{\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\" a time\",\"sequence_number\":2}"),
            sse("{\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":null,\"incomplete_details\":null},\"sequence_number\":3}"),
        ],
        Provider::Gemini => vec![
            sse("{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"upon\"}],\"role\":\"model\"},\"index\":0}]}"),
            sse("{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" a time\"}],\"role\":\"model\"},\"finishReason\":\"STOP\",\"index\":0}]}"),
        ],
    };
    (events, "upon a time")
}

fn unauthorized(provider: Provider) -> (u16, String) {
    match provider {
        Provider::Claude => (
            401,
            format!("{{\"type\":\"error\",\"error\":{{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key {KEY}\"}}}}"),
        ),
        Provider::ChatGpt => (
            401,
            format!("{{\"error\":{{\"message\":\"Incorrect API key provided: {KEY}.\",\"type\":\"invalid_request_error\",\"param\":null,\"code\":\"invalid_api_key\"}}}}"),
        ),
        Provider::Gemini => (
            400,
            "{\"error\":{\"code\":400,\"message\":\"API key not valid. Please pass a valid API key.\",\"status\":\"INVALID_ARGUMENT\",\
             \"details\":[{\"@type\":\"type.googleapis.com/google.rpc.ErrorInfo\",\"reason\":\"API_KEY_INVALID\"}]}}"
                .into(),
        ),
    }
}

#[test]
fn a_good_stream_arrives_piece_by_piece_with_the_documented_request() {
    for provider in Provider::ALL {
        let (events, text) = good(provider);
        let (base, seen) = serve(Reply::Stream { events, hold: false });
        let model = provider.default_model();
        let (pieces, result) = run(&base, provider, model, false);
        assert_eq!(result, Ok(()), "{provider:?}");
        assert_eq!(pieces, ["upon", " a time"], "{provider:?}");
        assert_eq!(pieces.concat(), text);

        let seen = seen.recv().unwrap();
        let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
        match provider {
            Provider::Claude => {
                assert_eq!(seen.line, "POST /v1/messages HTTP/1.1");
                assert_eq!(seen.header("x-api-key"), Some(KEY));
                assert_eq!(seen.header("anthropic-version"), Some("2026-10-21"));
                assert_eq!(seen.header("anthropic-beta"), Some("server-side-fallback-2026-07-01"));
                assert_eq!((body["model"].as_str(), body["stream"].as_bool()), (Some(model), Some(true)));
                assert_eq!(body["fallbacks"], "default");
                assert_eq!(body["messages"][0]["content"], "Once");
                assert_eq!(body["system"], "Be brief.");
            }
            Provider::ChatGpt => {
                assert_eq!(seen.line, "POST /v1/responses HTTP/1.1");
                assert_eq!(seen.header("authorization"), Some(format!("Bearer {KEY}").as_str()));
                assert_eq!((body["model"].as_str(), body["stream"].as_bool()), (Some(model), Some(true)));
                assert_eq!((body["instructions"].as_str(), body["input"].as_str()), (Some("Be brief."), Some("Once")));
                assert_eq!(body["store"], false);
            }
            Provider::Gemini => {
                assert_eq!(seen.line, format!("POST /v1beta/models/{model}:streamGenerateContent?alt=sse HTTP/1.1"));
                assert_eq!(seen.header("x-goog-api-key"), Some(KEY));
                assert_eq!(body["system_instruction"]["parts"][0]["text"], "Be brief.");
                assert_eq!(body["contents"][0]["parts"][0]["text"], "Once");
                assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
            }
        }
        assert_eq!(seen.header("content-type"), Some("application/json"));
    }
}

#[test]
fn a_rejected_key_says_so_and_never_echoes_the_key() {
    for provider in Provider::ALL {
        let (code, body) = unauthorized(provider);
        let (base, _seen) = serve(Reply::Status { code, body });
        let (pieces, result) = run(&base, provider, provider.default_model(), false);
        assert!(pieces.is_empty());
        let message = result.unwrap_err();
        assert_eq!(message, Failure::BadKey.message(provider), "{provider:?}");
        assert!(!message.contains(KEY));
    }
}

#[test]
fn a_provider_message_that_quotes_the_key_is_redacted() {
    let body = format!("{{\"error\":{{\"message\":\"Project for {KEY} is suspended.\",\"type\":\"x\",\"code\":null}}}}");
    let (base, _seen) = serve(Reply::Status { code: 400, body });
    let message = run(&base, Provider::ChatGpt, "gpt-6.1-terra", false).1.unwrap_err();
    assert_eq!(message, "ChatGPT: Project for [key] is suspended.");
}

#[test]
fn cancel_mid_stream_returns_at_once_while_the_server_still_holds_the_connection() {
    for provider in Provider::ALL {
        let (events, _) = good(provider);
        let (base, _seen) = serve(Reply::Stream { events: events[..events.len() - 1].to_vec(), hold: true });
        let started = Instant::now();
        let (pieces, result) = run(&base, provider, provider.default_model(), true);
        assert_eq!(result, Ok(()), "{provider:?}");
        assert_eq!(pieces, ["upon"], "{provider:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "{provider:?} took {:?}", started.elapsed());
    }
}

#[test]
fn a_rate_limit_and_an_unknown_model_say_what_to_do() {
    let limited = "{\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}}";
    let (base, _seen) = serve(Reply::Status { code: 429, body: limited.into() });
    assert_eq!(run(&base, Provider::Claude, "claude-sonnet-5-5", false).1, Err(Failure::RateLimited.message(Provider::Claude)));

    let missing = "{\"error\":{\"code\":404,\"message\":\"models/gemini-9 is not found\",\"status\":\"NOT_FOUND\"}}";
    let (base, _seen) = serve(Reply::Status { code: 404, body: missing.into() });
    assert_eq!(
        run(&base, Provider::Gemini, "gemini-9", false).1,
        Err("Gemini has no model called \"gemini-9\". Check the model in Settings.".into())
    );
}

#[test]
fn no_server_reads_as_offline() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let result = run(&format!("http://127.0.0.1:{port}"), Provider::Claude, "claude-sonnet-5-5", false).1;
    assert_eq!(result, Err(Failure::Offline.message(Provider::Claude)));
}

#[test]
fn a_key_that_could_not_be_a_header_fails_before_any_request() {
    let cancel = Cancel::default();
    let result = stream_at("http://127.0.0.1:9", Provider::Claude, "m", "sk bad\nkey", &prompt(), &cancel, &mut |_| {});
    assert_eq!(result, Err(Failure::BadKey.message(Provider::Claude)));
}

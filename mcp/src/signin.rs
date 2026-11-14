//! Signing this Mac in to Inkling's server, the way any OAuth client of it does (ADR 0004): it
//! registers itself, the writer allows it on the consent page and signs in with Google in the
//! browser, and the browser comes back to a one-time listener on 127.0.0.1 with a code. The
//! refresh token it gets is kept in the Keychain, never in a file.

use crate::sync::{Body, Error, Method, Remote};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use inkling_core::store::{KeyStore, Secret};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

/// Where `server/wrangler.jsonc` deploys the Worker.
pub const DEFAULT_SERVER: &str = "https://inkling-server.tobygodat.workers.dev";
/// The devices' scope, and staying signed in.
const SCOPE: &str = "sync offline_access";

/// What the Keychain keeps for this Mac's sign-in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignIn {
    pub server: String,
    pub client_id: String,
    pub refresh_token: String,
}

impl SignIn {
    pub fn load(keys: &dyn KeyStore) -> Option<SignIn> {
        serde_json::from_str(&keys.get(Secret::ServerSignIn)?).ok()
    }

    pub fn save(&self, keys: &dyn KeyStore) -> Result<(), String> {
        keys.set(Secret::ServerSignIn, &serde_json::to_string(self).expect("a sign-in serialises"))
    }

    /// A fresh access token for `/v1`. The server may hand back a new refresh token as well, and
    /// then this one is the one to keep.
    pub fn refresh(&mut self) -> Result<String, Error> {
        let metadata = discover(&self.server)?;
        let tokens = token(
            &metadata.token_endpoint,
            &[("grant_type", "refresh_token"), ("refresh_token", &self.refresh_token), ("client_id", &self.client_id)],
        )?;
        if let Some(rotated) = tokens.refresh_token {
            self.refresh_token = rotated;
        }
        Ok(tokens.access_token)
    }
}

#[derive(Deserialize)]
struct Metadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
}

/// Runs the whole sign-in. `open` is given the address to show the writer in their browser.
pub fn sign_in(server: &str, open: impl FnOnce(&str)) -> Result<SignIn, Error> {
    let server = server.trim_end_matches('/').to_string();
    let metadata = discover(&server)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| Error::Local(format!("No port for the browser to come back to ({e}).")))?;
    let port = listener.local_addr().map_err(|e| Error::Local(e.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let registered = agent()
        .post(&metadata.registration_endpoint)
        .header("content-type", "application/json")
        .send(
            json!({
                "client_name": "Inkling for Mac",
                "redirect_uris": [redirect],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            })
            .to_string(),
        )
        .map_err(transport)?;
    let client_id = answer(registered)?.get("client_id").and_then(Value::as_str).map(str::to_string).ok_or(Error::Refused {
        status: 200,
        message: "The server didn't give this Mac a client id.".into(),
    })?;

    let verifier = random();
    let state = random();
    let address = format!(
        "{}?{}",
        metadata.authorization_endpoint,
        query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", &redirect),
            ("scope", SCOPE),
            ("state", &state),
            ("code_challenge", &challenge(&verifier)),
            ("code_challenge_method", "S256"),
            ("resource", &format!("{server}/v1")),
        ])
    );
    open(&address);
    let code = wait_for_code(&listener, &state)?;

    let tokens = token(
        &metadata.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("client_id", &client_id),
            ("code_verifier", &verifier),
        ],
    )?;
    let refresh_token = tokens.refresh_token.ok_or(Error::Refused {
        status: 200,
        message: "The server signed this Mac in without a refresh token, so it would be signed out within the hour.".into(),
    })?;
    Ok(SignIn { server, client_id, refresh_token })
}

/// Takes browser requests until one comes to `/callback`, and answers it with a page saying how it
/// went. The `state` it carries must be the one sent, so a page elsewhere can't sign this Mac in.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, Error> {
    loop {
        let (stream, _) = listener.accept().map_err(|e| Error::Local(format!("The browser couldn't come back ({e}).")))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut line = String::new();
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let Some(params) = callback(&line) else {
            let _ = (&stream).write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            continue;
        };
        let get = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let (result, page) = match (get("code"), get("state"), get("error")) {
            (Some(code), Some(s), None) if s == state => (Ok(code.to_string()), "Signed in. You can close this tab and go back to the terminal."),
            (_, _, Some("access_denied")) => (Err(Error::Refused { status: 403, message: "Signing in was turned down.".into() }), "Not signed in. You can close this tab."),
            _ => (Err(Error::Refused { status: 400, message: "The sign-in came back without a code this Mac asked for.".into() }), "That sign-in didn't work. Try again from the terminal."),
        };
        let html = format!("<!doctype html><meta charset=utf-8><title>Inkling</title><p style=\"font:17px -apple-system,system-ui;margin:3rem\">{page}</p>");
        let _ = write!(&stream, "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}", html.len());
        return result;
    }
}

/// The query of a request line for `/callback`, decoded, or `None` for any other request.
fn callback(request_line: &str) -> Option<Vec<(String, String)>> {
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next()?, parts.next()?);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != "/callback" {
        return None;
    }
    Some(
        query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| {
                let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                (decode(k), decode(v))
            })
            .collect(),
    )
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(pairs: &[(&str, &str)]) -> String {
    let encode = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

/// 32 random bytes, base64url: a PKCE verifier or a state.
fn random() -> String {
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("the system has randomness");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE's S256 challenge for `verifier` (RFC 7636).
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()))
}

fn discover(server: &str) -> Result<Metadata, Error> {
    let response = agent().get(&format!("{server}/.well-known/oauth-authorization-server")).call().map_err(transport)?;
    serde_json::from_value(answer(response)?).map_err(|e| Error::Refused { status: 200, message: format!("{server} doesn't look like Inkling's server ({e}).") })
}

fn token(endpoint: &str, form: &[(&str, &str)]) -> Result<Tokens, Error> {
    let response = agent().post(endpoint).header("content-type", "application/x-www-form-urlencoded").send(query(form)).map_err(transport)?;
    match answer(response) {
        // An ended grant: the refresh token was revoked or sat unused for 60 days.
        Err(Error::Refused { status: 400 | 401, message }) if message.contains("invalid_grant") => Err(Error::SignedOut),
        other => serde_json::from_value(other?).map_err(|e| Error::Refused { status: 200, message: format!("The server's tokens couldn't be read ({e}).") }),
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

fn transport(_: ureq::Error) -> Error {
    Error::Offline
}

fn answer(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, Error> {
    let status = response.status().as_u16();
    let body = response.body_mut().with_config().limit(1024 * 1024).read_to_vec().map_err(transport)?;
    let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(value);
    }
    if status == 401 && value.get("error").and_then(Value::as_str) != Some("invalid_grant") {
        return Err(Error::SignedOut);
    }
    let message = match (value.get("error").and_then(Value::as_str), value.get("error_description").and_then(Value::as_str)) {
        (Some(code), Some(why)) => format!("{why} ({code})"),
        (Some(error), None) => error.to_string(),
        _ => format!("The server answered {status}."),
    };
    Err(Error::Refused { status, message })
}

/// The sync API over HTTPS with an access token.
pub struct Http {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl Http {
    pub fn new(server: &str, token: String) -> Http {
        Http { base: server.trim_end_matches('/').to_string(), token, agent: agent() }
    }
}

impl Remote for Http {
    fn send(&self, method: Method, path: &str, body: Body) -> Result<Vec<u8>, Error> {
        let url = format!("{}{path}", self.base);
        let bearer = format!("Bearer {}", self.token);
        let sent = match method {
            Method::Get => self.agent.get(&url).header("authorization", &bearer).call(),
            Method::Delete => self.agent.delete(&url).header("authorization", &bearer).call(),
            Method::Put | Method::Post => {
                let request = if method == Method::Put { self.agent.put(&url) } else { self.agent.post(&url) };
                let request = request.header("authorization", &bearer);
                match body {
                    Body::None => request.send_empty(),
                    Body::Json(value) => request.header("content-type", "application/json").send(value.to_string()),
                    Body::Bytes(bytes) => request.header("content-type", "application/octet-stream").send(bytes),
                }
            }
        };
        let mut response = sent.map_err(transport)?;
        let status = response.status().as_u16();
        let bytes = response.body_mut().with_config().limit(crate::sync::MAX_FILE_BYTES as u64 + 1024).read_to_vec().map_err(transport)?;
        match status {
            200..=299 => Ok(bytes),
            401 => Err(Error::SignedOut),
            _ => {
                let said = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
                Err(Error::Refused { status, message: said.unwrap_or_else(|| format!("The server answered {status}.")) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pkce_challenge_matches_rfc_7636() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn only_the_callback_is_read_and_its_query_is_decoded() {
        let params = callback("GET /callback?code=a%2Fb%3D&state=x+y&iss=https%3A%2F%2Fs HTTP/1.1\r\n").unwrap();
        assert_eq!(params, [("code".into(), "a/b=".into()), ("state".into(), "x y".into()), ("iss".into(), "https://s".into())]);
        assert_eq!(callback("GET /favicon.ico HTTP/1.1\r\n"), None);
        assert_eq!(callback("GET /callback HTTP/1.1"), Some(vec![]));
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
        assert_eq!(decode("%é"), "%é");
    }

    #[test]
    fn a_query_round_trips_through_the_callback_parser() {
        let pairs = [("state", "a b/c?d=é"), ("code", "~._-")];
        let line = format!("GET /callback?{} HTTP/1.1", query(&pairs));
        let parsed = callback(&line).unwrap();
        assert_eq!(parsed, pairs.map(|(k, v)| (k.to_string(), v.to_string())));
    }
}

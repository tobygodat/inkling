//! Signing a device in to Inkling's server, the way any OAuth client of it does (ADR 0004): it
//! registers itself, the writer allows it and signs in with Google in the browser, and the
//! browser comes back with a code. On the Mac it comes back to a one-time listener on 127.0.0.1;
//! on iPhone to `inkling://`, through the system sign-in sheet. The refresh token is kept in the
//! key store, which is the Keychain, never a file.

use crate::api::{read, refused};
use crate::{Error, Method, Response, Transport};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use inkling_core::store::{KeyStore, Secret};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The devices' scope, and staying signed in.
const SCOPE: &str = "sync offline_access";

/// What the key store keeps for this device's sign-in.
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
    /// then that one is the one to keep.
    pub fn token(&mut self, transport: &dyn Transport) -> Result<String, Error> {
        let metadata = discover(transport, &self.server)?;
        let tokens = token(
            transport,
            &metadata.token_endpoint,
            &[("grant_type", "refresh_token"), ("refresh_token", &self.refresh_token), ("client_id", &self.client_id)],
        )?;
        if let Some(rotated) = tokens.refresh_token {
            self.refresh_token = rotated;
        }
        Ok(tokens.access_token)
    }

    /// Where an agent connects to this account.
    pub fn agents_address(&self) -> String {
        format!("{}/mcp", self.server)
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

/// A sign-in under way: the page to show the writer, and what the answer is checked against.
#[derive(Clone, Debug)]
pub struct Begun {
    /// The server's sign-in page, for the browser or the sign-in sheet.
    pub url: String,
    server: String,
    client_id: String,
    redirect: String,
    state: String,
    verifier: String,
}

/// Registers this device with the server, coming back to `redirect`, and builds the sign-in page's
/// address with PKCE.
pub fn begin(transport: &dyn Transport, server: &str, redirect: &str, client_name: &str) -> Result<Begun, Error> {
    let server = server.trim_end_matches('/').to_string();
    let metadata = discover(transport, &server)?;
    let registration = json!({
        "client_name": client_name,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
    .to_string();
    let registered = transport.send(Method::Post, &metadata.registration_endpoint, &[], Some(("application/json", registration.as_bytes())))?;
    let client_id = answer(registered)?.get("client_id").and_then(Value::as_str).map(str::to_string).ok_or(Error::Refused {
        status: 200,
        message: "The server didn't give this device a client id.".into(),
    })?;
    let verifier = random();
    let state = random();
    let url = format!(
        "{}?{}",
        metadata.authorization_endpoint,
        query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", redirect),
            ("scope", SCOPE),
            ("state", &state),
            ("code_challenge", &challenge(&verifier)),
            ("code_challenge_method", "S256"),
            ("resource", &format!("{server}/v1")),
        ])
    );
    Ok(Begun { url, server, client_id, redirect: redirect.to_string(), state, verifier })
}

impl Begun {
    /// Takes the address the sign-in page came back to and trades its code for a refresh token.
    /// The `state` it carries must be the one sent, so a page elsewhere can't sign this device in.
    pub fn finish(&self, transport: &dyn Transport, came_back: &str) -> Result<SignIn, Error> {
        let params = params(came_back);
        let get = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let code = match (get("code"), get("state"), get("error")) {
            (Some(code), Some(state), None) if state == self.state => code,
            (_, _, Some("access_denied")) => return Err(Error::Refused { status: 403, message: "Signing in was turned down.".into() }),
            _ => return Err(Error::Refused { status: 400, message: "The sign-in came back without a code this device asked for.".into() }),
        };
        let metadata = discover(transport, &self.server)?;
        let tokens = token(
            transport,
            &metadata.token_endpoint,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &self.redirect),
                ("client_id", &self.client_id),
                ("code_verifier", &self.verifier),
            ],
        )?;
        let refresh_token = tokens.refresh_token.ok_or(Error::Refused {
            status: 200,
            message: "The server signed this device in without a refresh token, so it would be signed out within the hour.".into(),
        })?;
        Ok(SignIn { server: self.server.clone(), client_id: self.client_id.clone(), refresh_token })
    }
}

/// The Mac's way back from the browser: a listener on 127.0.0.1 for one request to `/callback`.
pub struct Loopback {
    listener: TcpListener,
    port: u16,
}

impl Loopback {
    pub fn bind() -> Result<Loopback, Error> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| Error::Local(format!("No port for the browser to come back to ({e}).")))?;
        let port = listener.local_addr().map_err(|e| Error::Local(e.to_string()))?.port();
        listener.set_nonblocking(true).map_err(|e| Error::Local(e.to_string()))?;
        Ok(Loopback { listener, port })
    }

    pub fn redirect(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits for the browser, answering it with a page that says to go back to Inkling, and
    /// gives the address it came back to. Gives up after `limit`, or once `cancelled` is set.
    pub fn wait(&self, cancelled: &AtomicBool, limit: Duration) -> Result<String, Error> {
        let deadline = Instant::now() + limit;
        loop {
            if cancelled.load(Ordering::Relaxed) || Instant::now() > deadline {
                return Err(Error::Unfinished);
            }
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => return Err(Error::Local(format!("The browser couldn't come back ({e})."))),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let target = match (parts.next(), parts.next()) {
                (Some("GET"), Some(target)) if target.split('?').next() == Some("/callback") => target.to_string(),
                _ => {
                    let _ = (&stream).write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                    continue;
                }
            };
            let html = "<!doctype html><meta charset=utf-8><title>Inkling</title><p style=\"font:17px -apple-system,system-ui;margin:3rem\">You can close this tab and go back to Inkling.</p>";
            let _ = write!(&stream, "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}", html.len());
            return Ok(format!("http://127.0.0.1:{}{target}", self.port));
        }
    }
}

/// The decoded query of an address.
pub(crate) fn params(address: &str) -> Vec<(String, String)> {
    let query = address.split_once('?').map_or("", |(_, q)| q);
    let query = query.split('#').next().unwrap_or("");
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect()
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

pub(crate) fn query(pairs: &[(&str, &str)]) -> String {
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
pub(crate) fn random() -> String {
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).expect("the system has randomness");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE's S256 challenge for `verifier` (RFC 7636).
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()))
}

fn discover(transport: &dyn Transport, server: &str) -> Result<Metadata, Error> {
    let response = transport.send(Method::Get, &format!("{server}/.well-known/oauth-authorization-server"), &[], None)?;
    serde_json::from_value(answer(response)?).map_err(|e| Error::Refused { status: 200, message: format!("{server} doesn't look like Inkling's server ({e}).") })
}

fn token(transport: &dyn Transport, endpoint: &str, form: &[(&str, &str)]) -> Result<Tokens, Error> {
    let body = query(form);
    let response = transport.send(Method::Post, endpoint, &[], Some(("application/x-www-form-urlencoded", body.as_bytes())))?;
    match answer(response) {
        // An ended grant: the refresh token was revoked or sat unused for 60 days.
        Err(Error::Refused { status: 400 | 401, message }) if message.contains("invalid_grant") => Err(Error::SignedOut),
        other => serde_json::from_value(other?).map_err(|e| Error::Refused { status: 200, message: format!("The server's tokens couldn't be read ({e}).") }),
    }
}

fn answer(response: Response) -> Result<Value, Error> {
    if (200..300).contains(&response.status) {
        return read(&response.body);
    }
    let value: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let message = match (value.get("error").and_then(Value::as_str), value.get("error_description").and_then(Value::as_str)) {
        (Some(code), Some(why)) => format!("{why} ({code})"),
        (Some(code), None) => code.to_string(),
        _ => return Err(refused(&response)),
    };
    Err(Error::Refused { status: response.status, message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fake;

    #[test]
    fn the_pkce_challenge_matches_rfc_7636() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn a_query_round_trips_and_odd_escapes_stay_as_they_are() {
        let pairs = [("state", "a b/c?d=é"), ("code", "~._-")];
        let parsed = params(&format!("inkling://callback?{}", query(&pairs)));
        assert_eq!(parsed, pairs.map(|(k, v)| (k.to_string(), v.to_string())));
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn signing_in_asks_for_the_devices_scope_and_checks_the_state() {
        let fake = Fake::memory();
        let begun = begin(&fake, crate::FAKE_SERVER, "inkling://callback", "Inkling for iPhone").unwrap();
        let asked = params(&begun.url);
        let get = |k: &str| asked.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
        assert!(begun.url.starts_with(&format!("{}/authorize?", crate::FAKE_SERVER)));
        assert_eq!(get("scope"), "sync offline_access");
        assert_eq!(get("redirect_uri"), "inkling://callback");
        assert_eq!(get("resource"), format!("{}/v1", crate::FAKE_SERVER));
        assert_eq!(get("code_challenge"), challenge(&begun.verifier));

        let forged = format!("inkling://callback?code={}&state=someone-else", crate::fake::CODE);
        assert!(matches!(begun.finish(&fake, &forged), Err(Error::Refused { status: 400, .. })));
        let mut signed = begun.finish(&fake, &crate::fake_way_back(&begun.url).unwrap()).unwrap();
        assert!(signed.token(&fake).is_ok());
        assert_eq!(signed.agents_address(), format!("{}/mcp", crate::FAKE_SERVER));
    }
}

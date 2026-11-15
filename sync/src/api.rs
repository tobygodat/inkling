//! The server's sync API under `/v1`, with an access token in hand.

use crate::{Error, Method, Response, Transport};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub enum Body<'a> {
    None,
    Json(Value),
    Bytes(&'a [u8]),
}

pub struct Api<'a> {
    transport: &'a dyn Transport,
    base: String,
    token: String,
}

impl<'a> Api<'a> {
    pub fn new(transport: &'a dyn Transport, server: &str, token: String) -> Self {
        Api { transport, base: server.trim_end_matches('/').to_string(), token }
    }

    /// Whatever the server answered, 401 aside, which means the sign-in has ended.
    pub fn answer(&self, method: Method, path: &str, body: Body) -> Result<Response, Error> {
        let bearer = format!("Bearer {}", self.token);
        let headers = [("authorization", bearer.as_str())];
        let json;
        let body = match body {
            Body::None => None,
            Body::Json(value) => {
                json = value.to_string();
                Some(("application/json", json.as_bytes()))
            }
            Body::Bytes(bytes) => Some(("application/octet-stream", bytes)),
        };
        let response = self.transport.send(method, &format!("{}{path}", self.base), &headers, body)?;
        if response.status == 401 {
            return Err(Error::SignedOut);
        }
        Ok(response)
    }

    /// The body of a 2xx answer.
    pub fn send(&self, method: Method, path: &str, body: Body) -> Result<Vec<u8>, Error> {
        let response = self.answer(method, path, body)?;
        if (200..300).contains(&response.status) {
            return Ok(response.body);
        }
        Err(refused(&response))
    }

    pub fn json<T: DeserializeOwned>(&self, method: Method, path: &str, body: Body) -> Result<T, Error> {
        read(&self.send(method, path, body)?)
    }
}

pub(crate) fn read<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|e| Error::Refused { status: 200, message: format!("The server sent an answer this can't read ({e}).") })
}

pub(crate) fn refused(response: &Response) -> Error {
    let said = serde_json::from_slice::<Value>(&response.body).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string));
    Error::Refused { status: response.status, message: said.unwrap_or_else(|| format!("The server answered {}.", response.status)) }
}

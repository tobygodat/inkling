//! One HTTP request at a time, so the same client runs over the network or against [`crate::Fake`].

use crate::Error;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Put,
    Post,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Sends a request and gives back whatever the server answered. Only failing to reach the server
/// is an error here: every status is a [`Response`].
pub trait Transport: Send + Sync {
    /// `body` is its content type and bytes.
    fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Response, Error>;
}

/// The largest file the server keeps, as `MAX_FILE_BYTES` in `server/src/http.ts`.
pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;

/// HTTPS, without following redirects.
pub struct Net(ureq::Agent);

impl Net {
    pub fn new() -> Self {
        Net(ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into())
    }
}

impl Default for Net {
    fn default() -> Self {
        Net::new()
    }
}

impl Transport for Net {
    fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Response, Error> {
        let sent = match method {
            Method::Get | Method::Delete => {
                let mut request = if method == Method::Get { self.0.get(url) } else { self.0.delete(url) };
                for (k, v) in headers {
                    request = request.header(*k, *v);
                }
                request.call()
            }
            Method::Put | Method::Post => {
                let mut request = if method == Method::Put { self.0.put(url) } else { self.0.post(url) };
                for (k, v) in headers {
                    request = request.header(*k, *v);
                }
                match body {
                    Some((kind, bytes)) => request.header("content-type", kind).send(bytes),
                    None => request.send_empty(),
                }
            }
        };
        let mut response = sent.map_err(|_| Error::Offline)?;
        let status = response.status().as_u16();
        let body = response.body_mut().with_config().limit(MAX_FILE_BYTES as u64 + 1024).read_to_vec().map_err(|_| Error::Offline)?;
        Ok(Response { status, body })
    }
}

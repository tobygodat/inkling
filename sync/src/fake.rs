//! Inkling's server in memory, for every headless run and the tests: the OAuth endpoints a device
//! uses, without Google, and the sync API with the Worker's rules (`server/src/sync.ts`). With a
//! path it is kept in a JSON file between requests, so a script can look at what a run sent,
//! seed what another device would have sent, or set `"down": true` to take it off the network.

use crate::account::{params, query};
use crate::transport::{Method, Response, Transport};
use crate::Error;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The fake's address. `.invalid` never resolves, so nothing meant for it can reach a real host.
pub const FAKE_SERVER: &str = "https://inkling.invalid";
/// The only code the fake's sign-in page hands out.
pub(crate) const CODE: &str = "fake-code";
const ACCESS: &str = "fake-access";
const REFRESH: &str = "fake-refresh";

/// What the browser would come back to from the fake's sign-in page at `authorize`: the
/// device's own way back, with the code and the state it sent.
pub fn fake_way_back(authorize: &str) -> Option<String> {
    let asked = params(authorize);
    let get = |k: &str| asked.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    Some(format!("{}?{}", get("redirect_uri")?, query(&[("code", CODE), ("state", get("state")?)])))
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Every request fails as if offline.
    pub down: bool,
    clients: u64,
    pub documents: BTreeMap<String, Doc>,
    pub books: BTreeMap<String, BookRow>,
    /// Which chapters of each book are uploaded.
    chapters: BTreeMap<String, Vec<usize>>,
    /// Parsed books, covers and inbox files, base64.
    files: BTreeMap<String, String>,
    pub inbox: Vec<InboxItem>,
    pub readwise: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Doc {
    pub text: String,
    pub saved_at: u64,
    pub device: String,
    pub version: u64,
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BookRow {
    pub title: String,
    pub author: Option<String>,
    pub format: String,
    pub chapters: usize,
    pub cover: Option<String>,
    pub added: u64,
    pub chapter: usize,
    pub fraction: f64,
    pub position_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InboxItem {
    pub readwise_id: String,
    pub title: String,
    pub category: String,
    pub state: String,
}

pub struct Fake {
    path: Option<PathBuf>,
    state: Mutex<State>,
}

impl Fake {
    pub fn memory() -> Fake {
        Fake { path: None, state: Mutex::default() }
    }

    /// Kept in `path`, read before and written after every request.
    pub fn at(path: PathBuf) -> Fake {
        Fake { path: Some(path), state: Mutex::default() }
    }

    /// Looks at or changes the server's state, as another device or a script would.
    pub fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(path) = &self.path {
            *state = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        }
        let out = f(&mut state);
        if let Some(path) = &self.path {
            let _ = inkling_books::library::write_atomic(path, &serde_json::to_vec_pretty(&*state).expect("the state serialises"));
        }
        out
    }
}

impl Transport for Fake {
    fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Response, Error> {
        let Some(rest) = url.strip_prefix(FAKE_SERVER) else { return Err(Error::Offline) };
        let path = rest.split('?').next().unwrap_or("");
        let body = body.map_or(&[][..], |(_, b)| b);
        self.with(|state| {
            if state.down {
                return Err(Error::Offline);
            }
            Ok(match path {
                "/.well-known/oauth-authorization-server" => ok(json!({
                    "authorization_endpoint": format!("{FAKE_SERVER}/authorize"),
                    "token_endpoint": format!("{FAKE_SERVER}/oauth/token"),
                    "registration_endpoint": format!("{FAKE_SERVER}/oauth/register"),
                })),
                "/oauth/register" => {
                    state.clients += 1;
                    ok(json!({ "client_id": format!("fake-client-{}", state.clients) }))
                }
                "/oauth/token" => {
                    let form = params(&format!("?{}", String::from_utf8_lossy(body)));
                    let get = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                    match (get("grant_type"), get("code"), get("refresh_token")) {
                        (Some("authorization_code"), Some(CODE), _) | (Some("refresh_token"), _, Some(REFRESH)) => {
                            ok(json!({ "access_token": ACCESS, "refresh_token": REFRESH, "token_type": "bearer" }))
                        }
                        _ => status(400, json!({ "error": "invalid_grant" })),
                    }
                }
                _ if path.starts_with("/v1/") => {
                    if !headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("authorization") && *v == format!("Bearer {ACCESS}")) {
                        return Ok(status(401, json!({ "error": "invalid_token" })));
                    }
                    let parts: Vec<&str> = path.trim_start_matches("/v1/").split('/').collect();
                    sync(state, method, &parts, body)
                }
                _ => status(404, json!({ "error": "Not found." })),
            })
        })
    }
}

fn sync(s: &mut State, method: Method, parts: &[&str], body: &[u8]) -> Response {
    let input: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match (method, parts) {
        (Method::Get, ["library"]) => {
            let books: Vec<Value> = s
                .books
                .iter()
                .map(|(id, b)| {
                    json!({ "id": id, "title": b.title, "author": b.author, "format": b.format, "chapters": b.chapters,
                        "cover": b.cover, "added": b.added, "chapter": b.chapter, "fraction": b.fraction, "position_at": b.position_at })
                })
                .collect();
            ok(json!({ "books": books, "inbox": s.inbox }))
        }
        (Method::Put, ["books", id, "file"]) => file_in(s, format!("book/{id}"), body),
        (Method::Get, ["books", id, "file"]) => file_out(s, &format!("book/{id}")),
        (Method::Put, ["books", id, "images", name]) => file_in(s, format!("image/{id}/{name}"), body),
        (Method::Get, ["books", id, "images", name]) => file_out(s, &format!("image/{id}/{name}")),
        (Method::Put, ["covers", name]) => file_in(s, format!("cover/{name}"), body),
        (Method::Get, ["covers", name]) => file_out(s, &format!("cover/{name}")),
        (Method::Get, ["inbox", id, "file"]) => file_out(s, &format!("inbox/{id}")),
        (Method::Put, ["books", id, "chapters", n]) => {
            let n: usize = n.parse().unwrap_or(usize::MAX);
            let list = s.chapters.entry(id.to_string()).or_default();
            if !list.contains(&n) {
                list.push(n);
            }
            ok(json!({ "ok": true }))
        }
        (Method::Put, ["books", id]) => {
            let row: BookRow = serde_json::from_value(input.clone()).unwrap_or_default();
            let uploaded = s.chapters.get(*id).map_or(0, |c| c.iter().filter(|n| **n < row.chapters).count());
            if !s.files.contains_key(&format!("book/{id}")) || uploaded != row.chapters {
                return status(409, json!({ "error": "Upload the book’s file and every chapter first." }));
            }
            let there = s.books.entry(id.to_string()).or_insert(row.clone());
            if there.cover.is_none() {
                there.cover = row.cover;
            }
            ok(json!({ "ok": true }))
        }
        (Method::Put, ["books", id, "position"]) => {
            let at = input["at"].as_u64().unwrap_or(0);
            let kept = match s.books.get_mut(*id) {
                Some(b) if b.position_at < at => {
                    b.chapter = input["chapter"].as_u64().unwrap_or(0) as usize;
                    b.fraction = input["fraction"].as_f64().unwrap_or(0.);
                    b.position_at = at;
                    true
                }
                _ => false,
            };
            ok(json!({ "ok": true, "kept": kept }))
        }
        (Method::Post, ["inbox", id]) => {
            if let Some(item) = s.inbox.iter_mut().find(|i| i.readwise_id == *id) {
                item.state = input["state"].as_str().unwrap_or("failed").to_string();
            }
            s.files.remove(&format!("inbox/{id}"));
            ok(json!({ "ok": true }))
        }
        (Method::Get, ["documents"]) => {
            let list: Vec<Value> = s
                .documents
                .iter()
                .map(|(id, d)| json!({ "id": id, "version": d.version, "saved_at": d.saved_at, "device": d.device, "deleted": d.deleted }))
                .collect();
            ok(json!(list))
        }
        (Method::Get, ["documents", id]) => match s.documents.get(*id) {
            Some(d) => ok(document(id, d)),
            None => status(404, json!({ "error": "Not found." })),
        },
        (Method::Put, ["documents", id]) => {
            let current = s.documents.get(*id).map_or(0, |d| d.version);
            if input["base"].as_u64() != Some(current) {
                let there = s.documents.get(*id).cloned().unwrap_or_default();
                return status(409, document(id, &there));
            }
            let doc = Doc {
                text: input["text"].as_str().unwrap_or_default().to_string(),
                saved_at: input["saved_at"].as_u64().unwrap_or(0),
                device: input["device"].as_str().unwrap_or_default().to_string(),
                version: current + 1,
                deleted: input["deleted"].as_bool().unwrap_or(false),
            };
            // A tombstone keeps the last text, as the Worker's does.
            let doc = match (doc.deleted, s.documents.get(*id)) {
                (true, Some(old)) => Doc { text: old.text.clone(), ..doc },
                _ => doc,
            };
            s.documents.insert(id.to_string(), doc);
            ok(json!({ "version": current + 1 }))
        }
        (Method::Get, ["readwise"]) => ok(json!({ "connected": s.readwise })),
        (Method::Put, ["readwise"]) => {
            if input["token"].as_str().is_none_or(|t| t.trim().is_empty()) {
                return status(400, json!({ "error": "Paste the token from readwise.io/access_token." }));
            }
            s.readwise = true;
            ok(json!({ "connected": true }))
        }
        (Method::Delete, ["readwise"]) => {
            s.readwise = false;
            ok(json!({ "connected": false }))
        }
        _ => status(404, json!({ "error": "Not found." })),
    }
}

fn document(id: &str, d: &Doc) -> Value {
    json!({ "id": id, "text": d.text, "version": d.version, "saved_at": d.saved_at, "device": d.device, "deleted": d.deleted })
}

fn file_in(s: &mut State, key: String, body: &[u8]) -> Response {
    s.files.insert(key, STANDARD.encode(body));
    ok(json!({ "ok": true }))
}

fn file_out(s: &State, key: &str) -> Response {
    match s.files.get(key).and_then(|b| STANDARD.decode(b).ok()) {
        Some(bytes) => Response { status: 200, body: bytes },
        None => status(404, json!({ "error": "Not found." })),
    }
}

fn ok(value: Value) -> Response {
    status(200, value)
}

fn status(status: u16, value: Value) -> Response {
    Response { status, body: value.to_string().into_bytes() }
}

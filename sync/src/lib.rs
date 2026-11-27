//! Keeping a device in step with Inkling's server (ADR 0004): signing in, then the documents,
//! the books and the reading positions in both directions, and Readwise uploads into the library.
//! No UI framework here. Everything that talks to the server goes through a [`Transport`], which
//! is the network in the app and [`Fake`], a server in memory, in every headless run.

pub mod account;
mod api;
pub mod documents;
pub mod fake;
pub mod library;
mod transport;

use std::fmt;

pub use account::{Begun, Loopback, SignIn};
pub use api::Api;
pub use fake::{FAKE_SERVER, Fake, fake_way_back};
pub use transport::{Method, Net, Response, Transport};
pub use documents::{Disk, State};

/// Where `server/wrangler.jsonc` deploys the Worker.
pub const SERVER: &str = "https://inkling-server.tobygodat.workers.dev";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The sign-in has ended or was never made.
    SignedOut,
    /// The server turned the request down, and said why.
    Refused { status: u16, message: String },
    /// The server couldn't be reached.
    Offline,
    /// Something on this device.
    Local(String),
    /// The writer closed the sign-in, or it was never finished.
    Unfinished,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SignedOut => write!(f, "The sign-in has ended. Sign in again."),
            Error::Refused { message, .. } => write!(f, "{message}"),
            Error::Offline => write!(f, "Inkling's server couldn't be reached."),
            Error::Local(why) => write!(f, "{why}"),
            Error::Unfinished => write!(f, "The sign-in wasn't finished."),
        }
    }
}

impl std::error::Error for Error {}

/// The transport a run should use: [`Fake`], kept in `fake-server.json` in the data folder, for a
/// headless run, so it never reaches the real server; else the network.
pub fn transport_for(paths: &inkling_core::Paths) -> (Box<dyn Transport>, &'static str) {
    if inkling_core::headless() {
        (Box::new(Fake::at(paths.dir.join("fake-server.json"))), FAKE_SERVER)
    } else {
        (Box::new(Net::new()), SERVER)
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// What one sync brought, for the app to record on the UI thread.
#[derive(Debug)]
pub struct Synced {
    pub documents: documents::Outcome,
    pub library: library::Outcome,
    /// Whether Readwise is connected on the server.
    pub readwise: bool,
}

/// One sync of everything: the documents in `paths`, its library, and Readwise's state. It runs
/// off the UI thread and changes nothing the app owns: [`documents::apply`] and the library's own
/// methods record the outcome. `sign_in` may come back with a new refresh token, to keep.
pub fn run(transport: &dyn Transport, sign_in: &mut SignIn, state: &documents::State, paths: &inkling_core::Paths) -> Result<Synced, Error> {
    let token = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, token);
    let folder = inkling_core::Documents::new(paths);
    let locals: Vec<documents::Local> = folder
        .list()
        .into_iter()
        .filter_map(|d| {
            let text = folder.read(&d.id).ok()?;
            let saved_at = d.changed.duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_millis() as u64);
            Some(documents::Local { id: d.id, text, saved_at })
        })
        .collect();
    let documents = documents::sync(&api, state, &locals, |taken| {
        let mut n = now_millis();
        while taken.contains(&n.to_string()) || folder.path(&n.to_string()).exists() {
            n += 1;
        }
        n.to_string()
    })?;
    // Readwise status goes before `library::sync` so no network call follows settling inbox or
    // incoming items (which deletes their staged files on the server).
    let readwise = api.json::<serde_json::Value>(Method::Get, "/v1/readwise", api::Body::None)?["connected"].as_bool().unwrap_or(false);
    let library = library::sync(&api, &paths.dir.join("library"))?;
    Ok(Synced { documents, library, readwise })
}

/// Connects Readwise with the writer's access token, or disconnects it with `None`. The token
/// goes to the server once and is kept only there. Returns whether Readwise is connected.
pub fn readwise(transport: &dyn Transport, sign_in: &mut SignIn, token: Option<&str>) -> Result<bool, Error> {
    let access = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, access);
    let answer: serde_json::Value = match token {
        Some(token) => api.json(Method::Put, "/v1/readwise", api::Body::Json(serde_json::json!({ "token": token.trim() })))?,
        None => api.json(Method::Delete, "/v1/readwise", api::Body::None)?,
    };
    Ok(answer["connected"].as_bool().unwrap_or(false))
}

/// A followed RSS or Atom feed on the server.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Feed {
    pub id: String,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub added_at: u64,
    pub checked_at: u64,
    pub last_error: Option<String>,
}

/// The server's answer after staging a saved web address for sync.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct SavedUrl {
    pub id: String,
    pub url: String,
    pub status: String,
}

/// Saves a web address (and optionally the page's HTML) on the server so it reaches every device.
pub fn save_url(transport: &dyn Transport, sign_in: &mut SignIn, url: &str, html: Option<&str>) -> Result<SavedUrl, Error> {
    let access = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, access);
    let mut body = serde_json::json!({ "url": url.trim() });
    if let Some(html) = html {
        body["html"] = serde_json::Value::String(html.to_string());
    }
    api.json(Method::Post, "/v1/save", api::Body::Json(body))
}

/// Lists the RSS and Atom feeds followed on the server.
pub fn feeds(transport: &dyn Transport, sign_in: &mut SignIn) -> Result<Vec<Feed>, Error> {
    #[derive(serde::Deserialize)]
    struct FeedList {
        feeds: Vec<Feed>,
    }
    let access = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, access);
    let list: FeedList = api.json(Method::Get, "/v1/feeds", api::Body::None)?;
    Ok(list.feeds)
}

/// Follows an RSS or Atom feed by its feed URL or site URL.
pub fn follow_feed(transport: &dyn Transport, sign_in: &mut SignIn, url: &str) -> Result<Feed, Error> {
    let access = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, access);
    api.json(Method::Post, "/v1/feeds", api::Body::Json(serde_json::json!({ "url": url.trim() })))
}

/// Unfollows a feed on the server. Articles already in the library stay.
pub fn unfollow_feed(transport: &dyn Transport, sign_in: &mut SignIn, id: &str) -> Result<(), Error> {
    let access = sign_in.token(transport)?;
    let api = Api::new(transport, &sign_in.server, access);
    api.send(Method::Delete, &format!("/v1/feeds/{id}"), api::Body::None).map(drop)
}

/// A name for this install, as the server records which device saved a document: the kind of
/// device and a few random letters.
pub fn new_device(kind: &str) -> String {
    format!("{kind}-{}", &account::random()[..8])
}

/// `sync.json`: what this device last agreed with the server on. Missing or unreadable, it starts
/// again, which costs nothing but a comparison of every document with the server's.
pub fn load_state(paths: &inkling_core::Paths, kind: &str) -> documents::State {
    let mut state: documents::State = std::fs::read(paths.dir.join("sync.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if state.device.is_empty() {
        state.device = new_device(kind);
    }
    state
}

pub fn save_state(paths: &inkling_core::Paths, state: &documents::State) -> std::io::Result<()> {
    inkling_books::library::write_atomic(&paths.dir.join("sync.json"), &serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn readwise_status_offline_in_run_never_loses_an_incoming_article() {
        struct DropReadwiseOnce<'a> {
            inner: &'a Fake,
            dropped: AtomicBool,
        }

        impl Transport for DropReadwiseOnce<'_> {
            fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Response, Error> {
                if method == Method::Get && url.ends_with("/v1/readwise") && !self.dropped.swap(true, Ordering::Relaxed) {
                    return Err(Error::Offline);
                }
                self.inner.send(method, url, headers, body)
            }
        }

        let fake = Fake::memory();
        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(8);
        let html = format!("<!doctype html><html><head><title>Safe Save</title></head><body><article><h1>Safe Save</h1><p>{words}</p></article></body></html>");
        fake.stage_incoming("save", "queue", "https://shore.example/safe", Some("Safe Save"), html.as_bytes());

        let begun = account::begin(&fake, FAKE_SERVER, "inkling://callback", "test").unwrap();
        let mut sign_in = begun.finish(&fake, &fake_way_back(&begun.url).unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = inkling_core::Paths { dir: dir.path().to_path_buf() };
        let state = load_state(&paths, "mac");

        let flaky = DropReadwiseOnce { inner: &fake, dropped: AtomicBool::new(false) };
        assert!(matches!(run(&flaky, &mut sign_in, &state, &paths), Err(Error::Offline)));

        // Because `/v1/readwise` ran before `library::sync`, the staged item wasn't settled yet and imports on retry.
        let recovered = run(&flaky, &mut sign_in, &state, &paths).unwrap();
        assert_eq!(recovered.library.imported.len(), 1);
        assert_eq!(recovered.library.imported[0].title, "Safe Save");
    }
}

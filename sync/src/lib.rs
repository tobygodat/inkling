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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SignedOut => write!(f, "The sign-in has ended. Sign in again."),
            Error::Refused { message, .. } => write!(f, "{message}"),
            Error::Offline => write!(f, "Inkling's server couldn't be reached."),
            Error::Local(why) => write!(f, "{why}"),
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
    let library = library::sync(&api, &paths.dir.join("library"))?;
    let readwise = api.json::<serde_json::Value>(Method::Get, "/v1/readwise", api::Body::None)?["connected"].as_bool().unwrap_or(false);
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

//! Account sign-in, background sync, feeds and Readwise as Swift sees them, directly over
//! `inkling-sync` (ADR 0004, ADR 0007, TOB-556).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use inkling_core::{Documents, KeyStore, MemoryKeys, Paths, Secret};
use inkling_sync::{self as sync, Begun, Disk, FAKE_SERVER, Fake, Feed, Loopback, SignIn, Synced, Transport};

use crate::{DocumentStore, InklingError, Library};

/// How long the Mac waits for the browser to come back from signing in.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);

/// A followed RSS or Atom feed on the server.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FeedInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub site_url: Option<String>,
    pub added_at: u64,
    pub checked_at: u64,
    pub last_error: Option<String>,
}

impl From<Feed> for FeedInfo {
    fn from(f: Feed) -> Self {
        FeedInfo {
            id: f.id,
            url: f.url,
            title: f.title,
            site_url: f.site_url,
            added_at: f.added_at,
            checked_at: f.checked_at,
            last_error: f.last_error,
        }
    }
}

/// Summary of what one sync applied locally, so the UI refreshes only what changed.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SyncReport {
    /// Document ids written or removed on disk.
    pub touched_docs: Vec<String>,
    /// Number of staged Worker items (Readwise, saved links, feeds, newsletters) parsed and added.
    pub imported_count: u32,
    /// Number of books or articles downloaded from another device.
    pub books_added: u32,
    /// Number of reading positions updated from another device.
    pub positions_updated: u32,
    /// Number of books whose highlights received updates from another device.
    pub highlights_updated: u32,
    /// Whether Readwise is connected on the server.
    pub readwise_connected: bool,
    /// Non-fatal per-item problems reported by library sync.
    pub problems: Vec<String>,
}

/// An in-flight OAuth sign-in: the authorization URL to open and an optional Mac loopback listener.
#[derive(uniffi::Object)]
pub struct SignInSession {
    begun: Begun,
    loopback: Mutex<Option<Loopback>>,
    cancelled: Arc<AtomicBool>,
}

impl Drop for SignInSession {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[uniffi::export]
impl SignInSession {
    /// The server's OAuth authorize URL to open in the browser or sign-in sheet.
    pub fn url(&self) -> String {
        self.begun.url.clone()
    }

    /// Cancels a waiting loopback listener.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// The fetched outcome of one background sync, waiting to be applied under the app's live locks.
#[derive(uniffi::Object)]
pub struct PendingSync {
    inner: Mutex<Option<(SignIn, Synced)>>,
}

/// Stages a raw HTML article (from `/save`, a feed, or a newsletter) into a fake server JSON file.
/// Used by headless snapshot and integration tests.
#[uniffi::export]
pub fn stage_fake_incoming(
    server_file: String,
    source: String,
    destination: String,
    url: String,
    title: Option<String>,
    html: String,
) -> String {
    let fake = Fake::at(PathBuf::from(server_file));
    fake.stage_incoming(&source, &destination, &url, title.as_deref(), html.as_bytes())
}

/// Account sign-in, sync, feeds and Readwise over `inkling-sync`.
#[derive(uniffi::Object)]
pub struct SyncClient {
    paths: Paths,
    transport: Arc<dyn Transport>,
    server: String,
    keys: Box<dyn KeyStore>,
    state: Mutex<sync::State>,
    headless: bool,
}

#[uniffi::export]
impl SyncClient {
    /// Opens the sync client for `data`. In headless runs (`INKLING_FAKE_AI` or
    /// `INKLING_MEMORY_KEYS`), talks to `Fake` and keeps sign-in credentials in memory only.
    #[uniffi::constructor]
    pub fn open(data: String, device_kind: String) -> Arc<SyncClient> {
        let paths = Paths { dir: data.into() };
        let (transport, server) = sync::transport_for(&paths);
        let state = sync::load_state(&paths, &device_kind);
        Arc::new(SyncClient {
            paths,
            transport: Arc::from(transport),
            server: server.to_string(),
            keys: inkling_core::default_keys(),
            state: Mutex::new(state),
            headless: inkling_core::headless(),
        })
    }

    /// Opens a sync client bound to an explicit fake server JSON file with in-memory keys.
    /// Used by headless multi-device tests so two folders sync against one fake server.
    #[uniffi::constructor]
    pub fn open_fake(data: String, device_kind: String, server_file: String) -> Arc<SyncClient> {
        let paths = Paths { dir: data.into() };
        let state = sync::load_state(&paths, &device_kind);
        Arc::new(SyncClient {
            paths,
            transport: Arc::new(Fake::at(PathBuf::from(server_file))),
            server: FAKE_SERVER.to_string(),
            keys: Box::new(MemoryKeys::default()),
            state: Mutex::new(state),
            headless: true,
        })
    }

    pub fn is_headless(&self) -> bool {
        self.headless
    }

    pub fn is_signed_in(&self) -> bool {
        self.keys.has(Secret::ServerSignIn)
    }

    /// The MCP endpoint URL for external agents (`<server>/mcp`), while signed in.
    pub fn agents_address(&self) -> Option<String> {
        SignIn::load(self.keys.as_ref()).map(|s| s.agents_address())
    }

    /// The address newsletters are sent to, while signed in.
    pub fn newsletter_address(&self) -> Option<String> {
        SignIn::load(self.keys.as_ref()).map(|_| inkling_sync::NEWSLETTER_ADDRESS.to_string())
    }

    /// The fake server's immediate OAuth callback URL for headless sign-in, or `None` outside headless mode.
    pub fn fake_callback_url(&self, authorize_url: String) -> Option<String> {
        self.headless.then(|| sync::fake_way_back(&authorize_url)).flatten()
    }

    /// Starts OAuth sign-in with an explicit redirect URI (iPhone: `inkling://callback`).
    pub fn begin_sign_in(&self, redirect_uri: String, client_name: String) -> Result<Arc<SignInSession>, InklingError> {
        let begun = sync::account::begin(self.transport.as_ref(), &self.server, &redirect_uri, &client_name).map_err(map_error)?;
        Ok(Arc::new(SignInSession {
            begun,
            loopback: Mutex::new(None),
            cancelled: Arc::new(AtomicBool::new(false)),
        }))
    }

    /// Starts OAuth sign-in with a one-time localhost listener (Mac: system browser).
    pub fn begin_loopback_sign_in(&self, client_name: String) -> Result<Arc<SignInSession>, InklingError> {
        let listener = Loopback::bind().map_err(map_error)?;
        let redirect = listener.redirect();
        let begun = sync::account::begin(self.transport.as_ref(), &self.server, &redirect, &client_name).map_err(map_error)?;
        Ok(Arc::new(SignInSession {
            begun,
            loopback: Mutex::new(Some(listener)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }))
    }

    /// Completes OAuth sign-in from the callback URL returned by `ASWebAuthenticationSession` or fake server.
    pub fn finish_sign_in(&self, session: Arc<SignInSession>, callback_url: String) -> Result<(), InklingError> {
        let sign_in = session.begun.finish(self.transport.as_ref(), &callback_url).map_err(map_error)?;
        sign_in.save(self.keys.as_ref()).map_err(|message| InklingError::Keychain { message })
    }

    /// Waits for the Mac's loopback listener to receive the OAuth callback and completes sign-in.
    pub fn wait_and_finish_loopback(&self, session: Arc<SignInSession>) -> Result<(), InklingError> {
        let listener = {
            let mut slot = session.loopback.lock().unwrap_or_else(|p| p.into_inner());
            slot.take().ok_or(InklingError::Cancelled)?
        };
        let came_back = listener.wait(&session.cancelled, SIGN_IN_LIMIT).map_err(map_error)?;
        let sign_in = session.begun.finish(self.transport.as_ref(), &came_back).map_err(map_error)?;
        sign_in.save(self.keys.as_ref()).map_err(|message| InklingError::Keychain { message })
    }

    /// Removes the server sign-in from the Keychain (or memory in headless runs).
    pub fn sign_out(&self) -> Result<(), InklingError> {
        self.keys.remove(Secret::ServerSignIn).map_err(|message| InklingError::Keychain { message })
    }

    /// Runs one sync pass off the main thread without touching the app's live index or open document.
    pub fn fetch(&self) -> Result<Arc<PendingSync>, InklingError> {
        let mut sign_in = SignIn::load(self.keys.as_ref()).ok_or(InklingError::SignedOut)?;
        let state = self.state_lock().clone();
        match sync::run(self.transport.as_ref(), &mut sign_in, &state, &self.paths) {
            Ok(synced) => Ok(Arc::new(PendingSync { inner: Mutex::new(Some((sign_in, synced))) })),
            Err(sync::Error::SignedOut) => {
                let _ = self.keys.remove(Secret::ServerSignIn);
                Err(InklingError::SignedOut)
            }
            Err(e) => {
                self.save_rotated(&sign_in);
                Err(map_error(e))
            }
        }
    }

    /// Applies a fetched sync onto `library` and `documents`, skipping any `held_docs` and never
    /// deleting `open_doc` under the writer.
    pub fn apply(
        &self,
        pending: Arc<PendingSync>,
        library: Arc<Library>,
        documents: Arc<DocumentStore>,
        open_doc: Option<String>,
        held_docs: Vec<String>,
    ) -> Result<SyncReport, InklingError> {
        let Some((sign_in, synced)) = pending.inner.lock().unwrap_or_else(|p| p.into_inner()).take() else {
            return Err(InklingError::Sync { message: "That sync was already applied.".into() });
        };
        self.save_rotated(&sign_in);

        let touched_docs = {
            let _doc_guard = documents.guard();
            let mut state = self.state_lock();
            let held: HashSet<String> = held_docs.into_iter().collect();
            let mut disk = PageDisk { store: documents.store(), held, open: open_doc };
            let touched = sync::documents::apply(synced.documents, &mut state, &mut disk);
            sync::save_state(&self.paths, &state)?;
            touched
        };

        let mut books_added: u32 = 0;
        let mut positions_updated: u32 = 0;
        let mut highlights_updated: u32 = 0;
        let mut imported_count: u32 = 0;
        {
            let mut lib = library.lock();
            for entry in synced.library.books {
                let existed = lib.entry(&entry.id).is_some();
                lib.record(entry)?;
                if !existed {
                    books_added += 1;
                }
            }
            for (id, position, at) in synced.library.positions {
                if lib.take_position(&id, position, at)? {
                    positions_updated += 1;
                }
            }
            for (id, incoming) in synced.library.highlights {
                let mut merged = lib.highlights(&id);
                let before = merged.len();
                for h in incoming {
                    if !merged.iter().any(|existing| (existing.chapter, existing.start, existing.end, &existing.words) == (h.chapter, h.start, h.end, &h.words)) {
                        merged.push(h);
                    }
                }
                if merged.len() > before {
                    merged.sort_by_key(|h| (h.chapter, h.start, h.made));
                    lib.set_highlights(&id, &merged)?;
                    highlights_updated += 1;
                }
            }
            for prepared in synced.library.imported {
                lib.add(&prepared)?;
                imported_count += 1;
            }
        }

        Ok(SyncReport {
            touched_docs,
            imported_count,
            books_added,
            positions_updated,
            highlights_updated,
            readwise_connected: synced.readwise,
            problems: synced.library.problems,
        })
    }

    /// Sends the Readwise access token directly to the server and never stores it on this device.
    pub fn connect_readwise(&self, token: String) -> Result<bool, InklingError> {
        self.with_sign_in(|transport, sign_in| sync::readwise(transport, sign_in, Some(&token)))
    }

    /// Disconnects Readwise on the server.
    pub fn disconnect_readwise(&self) -> Result<bool, InklingError> {
        self.with_sign_in(|transport, sign_in| sync::readwise(transport, sign_in, None))
    }

    /// Lists RSS and Atom feeds followed on the server.
    pub fn feeds(&self) -> Result<Vec<FeedInfo>, InklingError> {
        let list = self.with_sign_in(sync::feeds)?;
        Ok(list.into_iter().map(FeedInfo::from).collect())
    }

    /// Follows an RSS or Atom feed by its feed URL or site URL.
    pub fn follow_feed(&self, url: String) -> Result<FeedInfo, InklingError> {
        let feed = self.with_sign_in(|transport, sign_in| sync::follow_feed(transport, sign_in, &url))?;
        Ok(FeedInfo::from(feed))
    }

    /// Unfollows a feed on the server. Articles already in the library stay.
    pub fn unfollow_feed(&self, id: String) -> Result<(), InklingError> {
        self.with_sign_in(|transport, sign_in| sync::unfollow_feed(transport, sign_in, &id))
    }
}

impl SyncClient {
    fn state_lock(&self) -> MutexGuard<'_, sync::State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn save_rotated(&self, sign_in: &SignIn) {
        if SignIn::load(self.keys.as_ref()).is_some_and(|kept| kept != *sign_in && kept.client_id == sign_in.client_id) {
            let _ = sign_in.save(self.keys.as_ref());
        }
    }

    fn with_sign_in<T>(&self, f: impl FnOnce(&dyn Transport, &mut SignIn) -> Result<T, sync::Error>) -> Result<T, InklingError> {
        let mut sign_in = SignIn::load(self.keys.as_ref()).ok_or(InklingError::SignedOut)?;
        let result = f(self.transport.as_ref(), &mut sign_in);
        match result {
            Ok(value) => {
                self.save_rotated(&sign_in);
                Ok(value)
            }
            Err(sync::Error::SignedOut) => {
                let _ = self.keys.remove(Secret::ServerSignIn);
                Err(InklingError::SignedOut)
            }
            Err(e) => {
                self.save_rotated(&sign_in);
                Err(map_error(e))
            }
        }
    }
}

fn map_error(error: sync::Error) -> InklingError {
    match error {
        sync::Error::SignedOut => InklingError::SignedOut,
        sync::Error::Unfinished => InklingError::Cancelled,
        other => InklingError::Sync { message: other.to_string() },
    }
}

struct PageDisk<'a> {
    store: &'a Documents,
    held: HashSet<String>,
    open: Option<String>,
}

impl Disk for PageDisk<'_> {
    fn read(&self, id: &str) -> Option<String> {
        self.store.read(id).ok()
    }

    fn write(&mut self, id: &str, text: &str, saved_at: u64) -> Result<(), String> {
        self.store.restore(id, text, UNIX_EPOCH + Duration::from_millis(saved_at)).map_err(|e| e.to_string())
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        if self.open.as_deref() == Some(id) {
            return Err("open".into());
        }
        self.store.remove(id).map_err(|e| e.to_string())
    }

    fn open_to_sync(&self, id: &str) -> bool {
        !self.held.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Passage;
    use std::fs;

    fn sign_in_device(client: &SyncClient) {
        let session = client.begin_sign_in("inkling://callback".into(), "Inkling Test".into()).unwrap();
        let callback = client.fake_callback_url(session.url()).unwrap();
        client.finish_sign_in(session, callback).unwrap();
        assert!(client.is_signed_in());
    }

    #[test]
    fn two_devices_sync_documents_articles_positions_highlights_feeds_and_readwise() {
        let root = tempfile::tempdir().unwrap();
        let server_file = root.path().join("fake-server.json").to_string_lossy().into_owned();
        let mac_dir = root.path().join("mac").to_string_lossy().into_owned();
        let phone_dir = root.path().join("phone").to_string_lossy().into_owned();

        let mac_lib = Library::open(mac_dir.clone()).unwrap();
        let mac_docs = DocumentStore::open(mac_dir.clone()).unwrap();
        let mac_sync = SyncClient::open_fake(mac_dir.clone(), "mac".into(), server_file.clone());

        let phone_lib = Library::open(phone_dir.clone()).unwrap();
        let phone_docs = DocumentStore::open(phone_dir.clone()).unwrap();
        let phone_sync = SyncClient::open_fake(phone_dir.clone(), "phone".into(), server_file.clone());

        assert!(!mac_sync.is_signed_in());
        sign_in_device(&mac_sync);
        sign_in_device(&phone_sync);
        assert_eq!(mac_sync.agents_address().as_deref(), Some("https://inkling.invalid/mcp"));

        // Stage a feed item (Inbox) and a saved link (Queue) in the fake server.
        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(8);
        let feed_html = format!(
            "<!doctype html><html><head><title>Dispatch from the Coast</title><meta property=\"og:site_name\" content=\"Shoreline Notes\">\
             <meta name=\"author\" content=\"Ana Lima\"></head><body><article><h1>Dispatch from the Coast</h1><p>{words}</p></article></body></html>"
        );
        let saved_html = format!(
            "<!doctype html><html><head><title>Saved Essay</title><meta property=\"og:site_name\" content=\"Essays\"></head>\
             <body><article><h1>Saved Essay</h1><p>{words}</p></article></body></html>"
        );
        stage_fake_incoming(server_file.clone(), "feed".into(), "inbox".into(), "https://shore.example/dispatch".into(), Some("Dispatch from the Coast".into()), feed_html);
        stage_fake_incoming(server_file.clone(), "save".into(), "queue".into(), "https://essays.example/saved".into(), Some("Saved Essay".into()), saved_html);

        // Mac writes a document and syncs.
        let doc_id = mac_docs.new_id();
        let doc_text = "# Morning Notes\n\nWritten on the Mac and synced across.";
        mac_docs.write(doc_id.clone(), doc_text.into()).unwrap();

        let pending = mac_sync.fetch().unwrap();
        let report = mac_sync.apply(pending, mac_lib.clone(), mac_docs.clone(), None, vec![]).unwrap();
        assert_eq!(report.imported_count, 2);
        assert!(!report.readwise_connected);

        let items = mac_lib.items();
        let dispatch = items.iter().find(|i| i.title == "Dispatch from the Coast").unwrap();
        let saved = items.iter().find(|i| i.title == "Saved Essay").unwrap();
        assert!(dispatch.inbox, "feed item lands in Inbox");
        assert!(!saved.inbox, "saved link lands in Queue");

        // Mac highlights a passage and advances reading position, then syncs up.
        mac_lib.set_position(saved.id.clone(), 0, 0.5).unwrap();
        let passage = Passage { chapter: 0, start: 2, end: 13, words: "Saved Essay".into(), made: 1_700_000_000_000 };
        mac_lib.set_highlights(saved.id.clone(), vec![passage.clone()]).unwrap();
        let pending = mac_sync.fetch().unwrap();
        mac_sync.apply(pending, mac_lib.clone(), mac_docs.clone(), None, vec![]).unwrap();

        // Phone syncs and receives the document, both articles (with Inbox preserved), position and highlight.
        let pending = phone_sync.fetch().unwrap();
        let phone_report = phone_sync.apply(pending, phone_lib.clone(), phone_docs.clone(), None, vec![]).unwrap();
        assert_eq!(phone_report.touched_docs, vec![doc_id.clone()]);
        assert_eq!(phone_report.books_added, 2);
        assert_eq!(phone_report.highlights_updated, 1);
        assert_eq!(phone_docs.read(doc_id).unwrap(), doc_text);
        let phone_items = phone_lib.items();
        let phone_dispatch = phone_items.iter().find(|i| i.title == "Dispatch from the Coast").unwrap();
        let phone_saved = phone_items.iter().find(|i| i.title == "Saved Essay").unwrap();
        assert!(phone_dispatch.inbox);
        assert!(!phone_saved.inbox);
        assert!((phone_saved.fraction - 0.5).abs() < 0.01);
        assert_eq!(phone_lib.highlights(phone_saved.id.clone()), vec![passage]);

        // Follow and unfollow feeds.
        let feed = mac_sync.follow_feed("https://daringfireball.net/feeds/main".into()).unwrap();
        assert_eq!(feed.title, "daringfireball.net");
        assert_eq!(mac_sync.feeds().unwrap().len(), 1);
        mac_sync.unfollow_feed(feed.id).unwrap();
        assert!(mac_sync.feeds().unwrap().is_empty());

        // Connect Readwise and prove the token is never written to the device's folder.
        let secret_token = "rw-super-secret-token-never-on-disk";
        assert!(mac_sync.connect_readwise(secret_token.into()).unwrap());
        let pending = phone_sync.fetch().unwrap();
        let status = phone_sync.apply(pending, phone_lib.clone(), phone_docs.clone(), None, vec![]).unwrap();
        assert!(status.readwise_connected);

        for entry in walk_files(root.path()) {
            let bytes = fs::read(&entry).unwrap_or_default();
            assert!(!String::from_utf8_lossy(&bytes).contains(secret_token), "Readwise token must never appear in {:?}", entry);
        }

        assert!(!mac_sync.disconnect_readwise().unwrap());
        mac_sync.sign_out().unwrap();
        assert!(!mac_sync.is_signed_in());
    }

    fn walk_files(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    out.extend(walk_files(&path));
                } else {
                    out.push(path);
                }
            }
        }
        out
    }
}

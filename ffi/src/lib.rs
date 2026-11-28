//! Inkling's Rust crates as Swift sees them (ADR 0007). Everything here is a thin layer over
//! `inkling-books`: plain records in, plain records out, and one error type, so the crates under
//! it keep no UniFFI dependency. `apple/core.sh` builds it into `InklingCore`, the Swift package
//! the native app imports.
//!
//! Every call reads or writes the data folder and may take a while (a parse, a save), so Swift
//! makes them off the main thread. A `Library` is safe to share between threads.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use inkling_books::{self as books, BookId, Entry, Format, Highlight, Position};

mod ask;
mod documents;
mod look;
mod review;
mod sync;

pub use ask::{AnswerListener, AnswerSession, AnswerState, AskAbout, Citation, Conversation, Turn};
pub use documents::{DocInfo, DocumentStore, document_title};
pub use look::{Colors, Look, Looks, Tone, faces};
pub use review::{
    AiEngine, AiProvider, AskKind, ChangeDecision, ReviewChange, ReviewSegment, ReviewSession,
    ReviewStats, RevisionListener, RevisionSnapshot, RevisionState, TextEdit,
    provider_default_model, provider_label,
};
pub use sync::{FeedInfo, PendingSync, SignInSession, SyncClient, SyncReport, stage_fake_incoming};

uniffi::setup_scaffolding!();

/// What went wrong, in words the reader or writer can be shown.
#[derive(Debug, uniffi::Error)]
pub enum InklingError {
    /// The data folder couldn't be read or written.
    Storage { message: String },
    /// A file or page couldn't become a book or an article.
    Import { message: String },
    /// No book or document has this id.
    NotFound { id: String },
    /// The AI request wasn't valid for the draft (empty instruction, empty selection, etc.).
    InvalidRequest { message: String },
    /// The chosen provider has no API key saved yet.
    MissingKey { provider: String },
    /// The Keychain refused to save or remove a secret.
    Keychain { message: String },
    /// Sync or an account request failed.
    Sync { message: String },
    /// The server sign-in has ended or was revoked.
    SignedOut,
    /// Sign-in was cancelled before finishing.
    Cancelled,
}

impl std::fmt::Display for InklingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InklingError::Storage { message }
            | InklingError::Import { message }
            | InklingError::InvalidRequest { message }
            | InklingError::Keychain { message }
            | InklingError::Sync { message } => f.write_str(message),
            InklingError::NotFound { id } => write!(f, "Nothing in Inkling has the id {id}."),
            InklingError::MissingKey { provider } => {
                write!(f, "Add your {provider} API key in Settings before asking.")
            }
            InklingError::SignedOut => write!(f, "The sign-in has ended. Sign in again."),
            InklingError::Cancelled => write!(f, "The sign-in wasn’t finished."),
        }
    }
}

impl std::error::Error for InklingError {}

impl From<std::io::Error> for InklingError {
    fn from(e: std::io::Error) -> Self {
        InklingError::Storage { message: format!("The library couldn’t be read or written: {}.", e.to_string().to_lowercase()) }
    }
}

impl From<books::ImportError> for InklingError {
    fn from(e: books::ImportError) -> Self {
        InklingError::Import { message: e.to_string() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Kind {
    Epub,
    Pdf,
    Article,
}

impl From<Format> for Kind {
    fn from(f: Format) -> Self {
        match f {
            Format::Epub => Kind::Epub,
            Format::Pdf => Kind::Pdf,
            Format::Article => Kind::Article,
        }
    }
}

/// One row of the library: a book or an article.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub kind: Kind,
    pub chapters: u32,
    /// Where the reader left off: the chapter, from 0, and how far through it, from 0 to 1.
    pub chapter: u32,
    pub fraction: f32,
    /// How far through the whole thing, from 0 to 1.
    pub progress: f32,
    /// Seconds since the Unix epoch.
    pub added: u64,
    /// When it was last opened, in seconds since the Unix epoch, or 0 if never.
    pub opened: u64,
    /// The cover image's full path, for a book that has one.
    pub cover: Option<String>,
    /// When it was archived, in seconds since the Unix epoch, or 0 while it is still to read.
    pub archived: u64,
    /// An article's site: its own name, else its address's host.
    pub site: Option<String>,
    /// How many words it has, or 0 when that isn't known.
    pub words: u64,
    /// An article's summary of itself.
    pub excerpt: Option<String>,
    /// An article's first picture's full path, for one that has a picture.
    pub picture: Option<String>,
    /// An article's site icon's full path, when it has no lead picture.
    pub icon: Option<String>,
    /// Whether this item sits in the Inbox (from a feed or newsletter) rather than the Queue.
    pub inbox: bool,
    /// Whether it came by email rather than from a feed.
    pub newsletter: bool,
}

/// Where an article came from.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Source {
    pub url: String,
    pub site: Option<String>,
    pub published: Option<String>,
    pub excerpt: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Chapter {
    pub title: Option<String>,
    /// What the reader calls it in its contents, its place and "Next: …" (`Chapter::label`).
    pub label: String,
    /// The chapter as Markdown (ADR 0006). A picture's path is relative to `Library::folder`.
    pub text: String,
}

/// A book or an article, opened to read.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub kind: Kind,
    pub source: Option<Source>,
    pub chapters: Vec<Chapter>,
}

/// A highlighted passage: where it is in its chapter's Markdown, in bytes, and its words.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Passage {
    pub chapter: u32,
    pub start: u64,
    pub end: u64,
    pub words: String,
    /// When it was made, in milliseconds since the Unix epoch.
    pub made: u64,
}

impl From<Highlight> for Passage {
    fn from(h: Highlight) -> Self {
        Passage { chapter: h.chapter as u32, start: h.start as u64, end: h.end as u64, words: h.words, made: h.made }
    }
}

impl From<Passage> for Highlight {
    fn from(p: Passage) -> Self {
        Highlight { chapter: p.chapter as usize, start: p.start as usize, end: p.end as usize, words: p.words, made: p.made }
    }
}

/// Fetches a picture an article shows, by its absolute address, while the article is saved.
/// Swift implements it with URLSession. Returning nothing leaves the picture out.
#[uniffi::export(with_foreign)]
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: String) -> Option<Vec<u8>>;
}

/// The data folder the GPUI app keeps, so the native app opens the same library and look:
/// `INKLING_DATA_DIR` when set, else `~/Library/Application Support/Inkling`. On iPhone `HOME` is
/// the app's own container, so the folder is too.
#[uniffi::export]
pub fn data_folder() -> String {
    inkling_core::Paths::mac().dir.to_string_lossy().into_owned()
}

/// The library in a data folder: `<data>/library/`, the same folder the GPUI app keeps.
#[derive(uniffi::Object)]
pub struct Library {
    inner: Mutex<books::Library>,
}

#[uniffi::export]
impl Library {
    /// Opens the library in the data folder `data`, creating it if needed.
    #[uniffi::constructor]
    pub fn open(data: String) -> Result<Arc<Library>, InklingError> {
        let library = books::Library::open(PathBuf::from(data).join("library"))?;
        Ok(Arc::new(Library { inner: Mutex::new(library) }))
    }

    /// The library's own folder, which a chapter's picture paths are relative to.
    pub fn folder(&self) -> String {
        self.lock().dir().to_string_lossy().into_owned()
    }

    /// Everything in the library, newest first.
    pub fn items(&self) -> Vec<Item> {
        let library = self.lock();
        library.entries().iter().map(|e| item(&library, e)).collect()
    }

    /// Opens a book to read it. One stored by an older parser is parsed again first, and its
    /// place follows its chapter.
    pub fn book(&self, id: String) -> Result<Book, InklingError> {
        let mut library = self.lock();
        let id = known(&library, id)?;
        let (book, upgrade) = books::library::open(library.dir(), &id)?;
        if let Some(upgrade) = upgrade {
            library.add(&upgrade)?;
        }
        Ok(Book {
            id: book.id.0,
            title: book.title,
            author: book.author,
            kind: book.format.into(),
            source: book.source.map(|s| Source { url: s.url, site: s.site, published: s.published, excerpt: s.excerpt }),
            chapters: book.chapters.into_iter().enumerate().map(|(i, c)| Chapter { label: c.label(i), title: c.title, text: c.text }).collect(),
        })
    }

    /// Adds an EPUB or a PDF with text from a file on disk. The same file again finds the book
    /// already there, at its place.
    pub fn add_file(&self, path: String) -> Result<Item, InklingError> {
        let dir = self.lock().dir().to_path_buf();
        // The parse runs without the lock, so reading carries on meanwhile.
        let prepared = books::prepare(&dir, &PathBuf::from(path))?;
        let mut library = self.lock();
        let entry = library.add(&prepared)?.clone();
        Ok(item(&library, &entry))
    }

    /// Saves a web page as an article: `html` is the page as served from `url`. Its pictures are
    /// fetched once through `fetcher`. The same address again finds the article already there.
    pub fn save_article(&self, url: String, html: String, fetcher: Arc<dyn Fetcher>) -> Result<Item, InklingError> {
        let dir = self.lock().dir().to_path_buf();
        let prepared = books::prepare_article(&dir, &url, &html, &mut |at| fetcher.fetch(at.to_string()))?;
        let mut library = self.lock();
        let entry = library.add(&prepared)?.clone();
        Ok(item(&library, &entry))
    }

    /// Keeps the reader's place: the chapter, from 0, and how far through it, from 0 to 1.
    pub fn set_position(&self, id: String, chapter: u32, fraction: f32) -> Result<(), InklingError> {
        let mut library = self.lock();
        let id = known(&library, id)?;
        library.set_position(&id, Position { chapter: chapter as usize, fraction: fraction.clamp(0.0, 1.0) })?;
        Ok(())
    }

    /// Archives a book or an article, or brings it back to read.
    pub fn set_archived(&self, id: String, archived: bool) -> Result<(), InklingError> {
        let mut library = self.lock();
        let id = known(&library, id)?;
        library.set_archived(&id, archived)?;
        Ok(())
    }

    /// Marks an item as in the Inbox, or moves it out to the Queue when `inbox` is false.
    pub fn set_inbox(&self, id: String, inbox: bool) -> Result<(), InklingError> {
        let mut library = self.lock();
        let id = known(&library, id)?;
        library.set_inbox(&id, inbox)?;
        Ok(())
    }

    /// Notes that a book was just opened, for Keep reading.
    pub fn opened(&self, id: String) -> Result<(), InklingError> {
        let mut library = self.lock();
        let id = known(&library, id)?;
        library.opened(&id)?;
        Ok(())
    }

    /// A book's highlights, in reading order.
    pub fn highlights(&self, id: String) -> Vec<Passage> {
        self.lock().highlights(&BookId(id)).into_iter().map(Passage::from).collect()
    }

    /// Replaces a book's highlights with `all`.
    pub fn set_highlights(&self, id: String, all: Vec<Passage>) -> Result<(), InklingError> {
        let library = self.lock();
        let id = known(&library, id)?;
        let all: Vec<Highlight> = all.into_iter().map(Highlight::from).collect();
        library.set_highlights(&id, &all)?;
        Ok(())
    }
}

impl Library {
    pub(crate) fn lock(&self) -> MutexGuard<'_, books::Library> {
        // A panic while the lock was held leaves the index as last saved, which is still whole.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn known(library: &books::Library, id: String) -> Result<BookId, InklingError> {
    let id = BookId(id);
    match library.entry(&id) {
        Some(_) => Ok(id),
        None => Err(InklingError::NotFound { id: id.0 }),
    }
}

fn item(library: &books::Library, e: &Entry) -> Item {
    Item {
        id: e.id.0.clone(),
        title: e.title.clone(),
        author: e.author.clone(),
        kind: e.format.into(),
        chapters: e.chapters as u32,
        chapter: e.position.chapter as u32,
        fraction: e.position.fraction,
        progress: e.progress(),
        added: e.added,
        opened: e.opened,
        cover: library.cover_path(e).map(|p| p.to_string_lossy().into_owned()),
        archived: e.archived,
        site: e.site.clone(),
        words: e.words as u64,
        excerpt: e.excerpt.clone(),
        picture: library.picture_path(e).map(|p| p.to_string_lossy().into_owned()),
        icon: library.icon_path(e).map(|p| p.to_string_lossy().into_owned()),
        inbox: e.inbox,
        newsletter: e.newsletter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pictures;

    impl Fetcher for Pictures {
        fn fetch(&self, url: String) -> Option<Vec<u8>> {
            url.ends_with(".png").then(|| b"\x89PNG\r\n\x1a\n".to_vec())
        }
    }

    #[test]
    fn swift_saves_reads_and_marks_an_article_through_the_library() {
        let data = tempfile::tempdir().unwrap();
        let data = data.path().to_string_lossy().into_owned();
        let library = Library::open(data.clone()).unwrap();
        assert!(library.items().is_empty());

        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(8);
        let html = format!(
            "<html><head><title>Notes on the tide</title></head><body><nav>Home</nav><article><h1>Notes on the tide</h1>\
             <p>{words}</p><p><img src=\"/flats.png\" alt=\"The flats\"></p><p>{words}</p></article></body></html>"
        );
        let saved = library.save_article("https://shore.example/tide".into(), html.clone(), Arc::new(Pictures)).unwrap();
        assert_eq!((saved.kind, saved.title.as_str(), saved.chapters), (Kind::Article, "Notes on the tide", 1));
        assert_eq!((saved.site.as_deref(), saved.archived), (Some("shore.example"), 0));
        assert!(saved.words > 200, "{} words", saved.words);
        assert!(saved.picture.as_deref().is_some_and(|p| PathBuf::from(p).is_file()), "the first picture is on disk for the row: {:?}", saved.picture);
        assert!(saved.excerpt.as_deref().is_some_and(|e| e.starts_with("The tide came in")), "{:?}", saved.excerpt);
        let again = library.save_article("https://shore.example/tide#top".into(), html, Arc::new(Pictures)).unwrap();
        assert_eq!(again.id, saved.id, "the same page is found again");

        let book = library.book(saved.id.clone()).unwrap();
        assert_eq!(book.source.as_ref().map(|s| s.url.as_str()), Some("https://shore.example/tide"));
        let picture = book.chapters[0].text.split("](").nth(1).and_then(|rest| rest.split([' ', ')']).next()).unwrap().to_string();
        assert!(PathBuf::from(library.folder()).join(&picture).is_file(), "{picture} is on disk, relative to the library");

        library.set_position(saved.id.clone(), 0, 1.5).unwrap();
        let passage = Passage { chapter: 0, start: 2, end: 19, words: "Notes on the tide".into(), made: 7 };
        library.set_highlights(saved.id.clone(), vec![passage.clone()]).unwrap();
        library.set_archived(saved.id.clone(), true).unwrap();

        let reopened = Library::open(data).unwrap();
        let item = &reopened.items()[0];
        assert_eq!((item.chapter, item.fraction, item.progress), (0, 1.0, 1.0), "a place past the end is kept at the end");
        assert!(item.archived > 0, "archiving is kept");
        assert_eq!(reopened.highlights(saved.id.clone()), [passage]);
        assert!(matches!(reopened.book("nope".into()), Err(InklingError::NotFound { .. })));
        let refused = reopened.save_article("/tide".into(), String::new(), Arc::new(Pictures)).unwrap_err();
        assert_eq!(refused.to_string(), "This page couldn’t be saved as an article: “/tide” isn’t a full web address.");
    }
}

//! The books on disk, in the data dir:
//! - `library.json`: one entry per book (id, title, author, format, position, added time).
//! - `books/<id>.json`: the parsed book, its chapters in Markdown. Named by the hash of the file
//!   it came from, so importing the same file again reuses it, and two imports writing it at once
//!   write the same.
//! - `originals/<id>.epub` or `.pdf`: the file as it was added, so a newer parser can read it
//!   again without the reader adding the book again.
//! - `images/<id>/<n>.<ext>`: the images the chapters show.
//! - `covers/<id>.<ext>`: the cover image an EPUB names.
//! - `highlights/<id>.json`: the passages the reader highlighted (`crate::highlights`).
//!
//! Every write goes to a temporary file first and is renamed into place, the parsed book last.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::book::{Book, BookId, Format, PARSER, Position};
use crate::highlights::{self, Highlight};
use crate::import::{self, ImportError};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: BookId,
    pub title: String,
    pub author: Option<String>,
    pub format: Format,
    #[serde(default)]
    pub position: Position,
    /// Seconds since the Unix epoch.
    pub added: u64,
    /// How many chapters the book has, for the progress line on its cover.
    #[serde(default)]
    pub chapters: usize,
    /// The cover's file name in `covers/`, for a book that has one.
    #[serde(default)]
    pub cover: Option<String>,
    /// When the book was last opened, in seconds since the Unix epoch, or 0 if never.
    #[serde(default)]
    pub opened: u64,
    /// When `position` last changed, in milliseconds since the Unix epoch, or 0 if never. Sync
    /// keeps the later of two devices' positions by this.
    #[serde(default)]
    pub position_at: u64,
    /// When it was archived, in seconds since the Unix epoch, or 0 while it is still to read.
    #[serde(default)]
    pub archived: u64,
    /// An article's site for its row: the site's own name, else its address's host.
    #[serde(default)]
    pub site: Option<String>,
    /// How many words it has, for how long it takes to read. 0 for one added before this was kept.
    #[serde(default)]
    pub words: usize,
    /// An article's summary of itself, for its row.
    #[serde(default)]
    pub excerpt: Option<String>,
    /// An article's first picture, its path relative to the library's folder, for its row.
    #[serde(default)]
    pub picture: Option<String>,
}

impl Entry {
    /// How far through the whole book, in `0.0..=1.0`, counting chapters as equal.
    pub fn progress(&self) -> f32 {
        if self.chapters == 0 {
            return 0.0;
        }
        ((self.position.chapter as f32 + self.position.fraction) / self.chapters as f32).clamp(0.0, 1.0)
    }
}

#[derive(Debug)]
pub struct Library {
    dir: PathBuf,
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
struct Index {
    books: Vec<Entry>,
}

impl Library {
    /// Opens the library in `dir`, creating it if needed. A missing or damaged index reads as
    /// an empty library rather than failing the launch.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Library> {
        let dir = dir.into();
        fs::create_dir_all(dir.join("books"))?;
        let entries = fs::read(dir.join("library.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Index>(&bytes).ok())
            .map(|i| i.books)
            .unwrap_or_default();
        Ok(Library { dir, entries })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Newest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Where a book's cover image is on disk, if it has one.
    pub fn cover_path(&self, entry: &Entry) -> Option<PathBuf> {
        entry.cover.as_ref().map(|name| self.dir.join("covers").join(name))
    }

    /// Where an article's first picture is on disk, if it has one.
    pub fn picture_path(&self, entry: &Entry) -> Option<PathBuf> {
        entry.picture.as_ref().map(|path| self.dir.join(path))
    }

    pub fn entry(&self, id: &BookId) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.id == id)
    }

    /// Records a book [`prepare`] wrote. The same book again keeps its entry and its place, which
    /// follows its chapter when a newer parse changed the chapters.
    pub fn add(&mut self, book: &Prepared) -> io::Result<&Entry> {
        let at = match self.entries.iter().position(|e| e.id == book.id) {
            Some(at) => {
                let entry = &mut self.entries[at];
                let before = entry.clone();
                if entry.cover.is_none() && book.cover.is_some() {
                    entry.cover = book.cover.clone();
                }
                if entry.site.is_none() {
                    entry.site = book.site.clone();
                }
                if entry.words == 0 {
                    entry.words = book.words;
                }
                if entry.excerpt.is_none() {
                    entry.excerpt = book.excerpt.clone();
                }
                if entry.picture.is_none() {
                    entry.picture = book.picture.clone();
                }
                if let Some(old) = &book.before {
                    (entry.title, entry.author, entry.chapters) = (book.title.clone(), book.author.clone(), book.chapters);
                    let carried = entry.position.carried(old, &book.titles);
                    if carried != entry.position {
                        entry.position = carried;
                        entry.position_at = now_millis().max(entry.position_at + 1);
                    }
                }
                if *entry != before {
                    self.save()?;
                }
                at
            }
            None => {
                let entry = Entry {
                    id: book.id.clone(),
                    title: book.title.clone(),
                    author: book.author.clone(),
                    format: book.format,
                    position: Position::default(),
                    added: now(),
                    chapters: book.chapters,
                    cover: book.cover.clone(),
                    opened: 0,
                    position_at: 0,
                    archived: 0,
                    site: book.site.clone(),
                    words: book.words,
                    excerpt: book.excerpt.clone(),
                    picture: book.picture.clone(),
                };
                self.entries.insert(0, entry);
                self.save()?;
                0
            }
        };
        Ok(&self.entries[at])
    }

    /// Stores an already parsed book and records it, as an import of its file would.
    pub fn insert(&mut self, book: &Book) -> io::Result<&Entry> {
        store(&self.dir, book)?;
        self.add(&Prepared::of(book))
    }

    pub fn load(&self, id: &BookId) -> io::Result<Book> {
        load(&self.dir, id)
    }

    /// The book opened most recently, for going back to it.
    pub fn last_opened(&self) -> Option<&Entry> {
        self.entries.iter().filter(|e| e.opened > 0).max_by_key(|e| e.opened)
    }

    /// Records that the book was opened now.
    pub fn opened(&mut self, id: &BookId) -> io::Result<()> {
        let latest = self.entries.iter().map(|e| e.opened).max().unwrap_or(0);
        let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) else { return Ok(()) };
        // Later than any book opened before, even within the same second.
        entry.opened = now().max(latest + 1);
        self.save()
    }

    /// Takes the book out of the library: its entry, and every file kept under its id (the parsed
    /// book, the original, the cover, the images and anything else stored by id) move to
    /// `.removed/<id>/` until [`Library::restore`] puts them back or [`Library::forget`] deletes them.
    pub fn remove(&mut self, id: &BookId) -> io::Result<Removed> {
        let Some(at) = self.entries.iter().position(|e| &e.id == id) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "the book isn't in the library"));
        };
        let aside = self.aside(id);
        let _ = fs::remove_dir_all(&aside);
        for (from, to) in self.stored_under(id, &self.dir, &aside)? {
            fs::create_dir_all(to.parent().unwrap_or(&aside))?;
            fs::rename(from, to)?;
        }
        let entry = self.entries.remove(at);
        self.save()?;
        Ok(Removed { entry, at })
    }

    /// Puts a removed book back where it was, with everything it had.
    pub fn restore(&mut self, removed: Removed) -> io::Result<()> {
        let aside = self.aside(&removed.entry.id);
        if aside.is_dir() {
            for (from, to) in self.stored_under(&removed.entry.id, &aside, &self.dir)? {
                fs::create_dir_all(to.parent().unwrap_or(&self.dir))?;
                fs::rename(from, to)?;
            }
            let _ = fs::remove_dir_all(&aside);
        }
        let at = removed.at.min(self.entries.len());
        self.entries.insert(at, removed.entry);
        self.save()
    }

    /// Deletes what a removed book left aside, once it can no longer come back.
    pub fn forget(&self, removed: Removed) {
        let _ = fs::remove_dir_all(self.aside(&removed.entry.id));
    }

    /// Deletes what books removed before the last quit left aside: they can no longer come back.
    /// Only the app calls it, at launch, so a reader opening the library meanwhile never does.
    pub fn forget_removed(&self) {
        let _ = fs::remove_dir_all(self.dir.join(REMOVED));
    }

    fn aside(&self, id: &BookId) -> PathBuf {
        self.dir.join(REMOVED).join(&id.0)
    }

    /// Each file or folder in a folder of `from` named for the book, paired with the same place
    /// under `to`.
    fn stored_under(&self, id: &BookId, from: &Path, to: &Path) -> io::Result<Vec<(PathBuf, PathBuf)>> {
        let mut found = vec![];
        for folder in fs::read_dir(from)?.flatten() {
            let name = folder.file_name();
            if name == REMOVED || !folder.path().is_dir() {
                continue;
            }
            for item in fs::read_dir(folder.path())?.flatten() {
                let path = item.path();
                if path.file_stem().is_some_and(|stem| stem == id.0.as_str()) {
                    found.push((path, to.join(&name).join(item.file_name())));
                }
            }
        }
        Ok(found)
    }

    pub fn set_position(&mut self, id: &BookId, position: Position) -> io::Result<()> {
        let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) else { return Ok(()) };
        if entry.position == position {
            return Ok(());
        }
        entry.position = position;
        entry.position_at = now_millis().max(entry.position_at + 1);
        self.save()
    }

    /// Archives the book, or brings it back to read when `archived` is false.
    pub fn set_archived(&mut self, id: &BookId, archived: bool) -> io::Result<()> {
        let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) else { return Ok(()) };
        if (entry.archived > 0) == archived {
            return Ok(());
        }
        entry.archived = if archived { now() } else { 0 };
        self.save()
    }

    /// Records a book another device added, whose parsed file is already stored here. A book
    /// already in the library is left as it is.
    pub fn record(&mut self, entry: Entry) -> io::Result<()> {
        if self.entry(&entry.id).is_some() {
            return Ok(());
        }
        let at = self.entries.iter().position(|e| e.added <= entry.added).unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
        self.save()
    }

    /// Takes a place read on another device, if it was read there after here. Returns whether it did.
    pub fn take_position(&mut self, id: &BookId, position: Position, at: u64) -> io::Result<bool> {
        let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) else { return Ok(false) };
        if at <= entry.position_at {
            return Ok(false);
        }
        entry.position = position;
        entry.position_at = at;
        self.save()?;
        Ok(true)
    }

    /// A book's highlights, in reading order.
    pub fn highlights(&self, id: &BookId) -> Vec<Highlight> {
        highlights::load(&self.dir, id)
    }

    pub fn set_highlights(&self, id: &BookId, all: &[Highlight]) -> io::Result<()> {
        highlights::save(&self.dir, id, all)
    }

    fn save(&self) -> io::Result<()> {
        let index = Index { books: self.entries.clone() };
        write_atomic(&self.dir.join("library.json"), &serde_json::to_vec_pretty(&index).map_err(io::Error::other)?)
    }
}

/// Where a removed book's files wait for an undo.
const REMOVED: &str = ".removed";

/// A book taken out of the library, which can still come back.
#[derive(Debug)]
pub struct Removed {
    pub entry: Entry,
    /// Its place in the list.
    at: usize,
}

/// What an import leaves for the library to record: the book is already on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub id: BookId,
    pub title: String,
    pub author: Option<String>,
    pub format: Format,
    pub chapters: usize,
    pub cover: Option<String>,
    /// Each chapter's title, so a place can follow its chapter.
    pub titles: Vec<Option<String>>,
    /// The chapter titles of the parse this one replaced, when it replaced one.
    pub before: Option<Vec<Option<String>>>,
    pub site: Option<String>,
    pub words: usize,
    pub excerpt: Option<String>,
    pub picture: Option<String>,
}

/// Reads, parses and stores the book at `path` under `dir`, without touching the index, so it
/// runs on any thread. A book already stored by this parser is read back instead of parsed again;
/// one stored by an older parser is parsed again, which upgrades it in place.
pub fn prepare(dir: &Path, path: &Path) -> Result<Prepared, ImportError> {
    let unreadable = |e: io::Error| ImportError::Unreadable(e.to_string().to_lowercase());
    let bytes = fs::read(path).map_err(unreadable)?;
    let id = import::book_id(&bytes);
    let stored = load(dir, &id).ok();
    let (book, before) = match stored {
        Some(book) if book.parser >= PARSER => {
            keep_original(dir, &book, &bytes).map_err(unreadable)?;
            (book, None)
        }
        old => (parse_and_store(dir, path, &bytes)?, old.map(|b| titles(&b))),
    };
    let mut prepared = Prepared::of(&book);
    prepared.before = before;
    if book.format == Format::Epub {
        prepared.cover = store_cover(dir, &id, &bytes);
    }
    Ok(prepared)
}

/// Parses and stores the web page `html`, saved from `url`, under `dir`, as [`prepare`] does for
/// a file: without touching the index, so it runs on any thread. The page's HTML is kept in
/// `originals/` and its pictures, fetched through `fetch`, in `images/<id>/`. An article already
/// stored by this parser is read back, and nothing is fetched.
pub fn prepare_article(dir: &Path, url: &str, html: &str, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Prepared, ImportError> {
    let id = crate::article::article_id(url);
    if let Ok(book) = load(dir, &id)
        && book.parser >= PARSER
    {
        return Ok(Prepared::of(&book));
    }
    let book = store_parsed(dir, crate::article::parse(url, html, fetch)?, html.as_bytes())?;
    Ok(Prepared::of(&book))
}

/// Reads a stored book from the library in `dir`, without the index and without writing, so it
/// runs on any thread and in a reader that must not change the library. A book stored before
/// chapters were Markdown reads as Markdown.
pub fn load(dir: &Path, id: &BookId) -> io::Result<Book> {
    let bytes = fs::read(book_path(dir, id))?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Loads a book to read it, first parsing it again from its original when an older parser stored
/// it. The upgrade comes back for [`Library::add`] to record, which moves the place to follow its
/// chapter. A book with no original kept (one added before originals were) loads as it is.
pub fn open(dir: &Path, id: &BookId) -> io::Result<(Book, Option<Prepared>)> {
    let book = load(dir, id)?;
    // An article's pictures were fetched when it was saved, and parsing it again would fetch them
    // again, so it reads as it was stored.
    if book.parser >= PARSER || book.format == Format::Article {
        return Ok((book, None));
    }
    let original = original_path(dir, id, book.format);
    if !original.is_file() {
        return Ok((book, None));
    }
    let bytes = fs::read(&original)?;
    match parse_and_store(dir, &original, &bytes) {
        Ok(new) => {
            let mut prepared = Prepared::of(&new);
            prepared.before = Some(titles(&book));
            Ok((new, Some(prepared)))
        }
        Err(_) => Ok((book, None)),
    }
}

/// The file a book was added from, as the library keeps it.
pub fn original_path(dir: &Path, id: &BookId, format: Format) -> PathBuf {
    let extension = match format {
        Format::Epub => "epub",
        Format::Pdf => "pdf",
        Format::Article => "html",
    };
    dir.join("originals").join(format!("{}.{extension}", id.0))
}

fn titles(book: &Book) -> Vec<Option<String>> {
    book.chapters.iter().map(|c| c.title.clone()).collect()
}

/// Parses the file and stores everything it needs: the original, its images, then the book.
fn parse_and_store(dir: &Path, path: &Path, bytes: &[u8]) -> Result<Book, ImportError> {
    store_parsed(dir, import::parse(path, bytes)?, bytes)
}

/// Keeps a parse on disk: the original it came from, its pictures, then the book itself.
fn store_parsed(dir: &Path, parsed: import::Parsed, bytes: &[u8]) -> Result<Book, ImportError> {
    let unreadable = |e: io::Error| ImportError::Unreadable(e.to_string().to_lowercase());
    keep_original(dir, &parsed.book, bytes).map_err(unreadable)?;
    if !parsed.images.is_empty() {
        let images = dir.join(import::image_dir(&parsed.book.id));
        fs::create_dir_all(&images).map_err(unreadable)?;
        for (name, bytes) in &parsed.images {
            write_atomic(&images.join(name), bytes).map_err(unreadable)?;
        }
    }
    store(dir, &parsed.book).map_err(unreadable)?;
    Ok(parsed.book)
}

fn keep_original(dir: &Path, book: &Book, bytes: &[u8]) -> io::Result<()> {
    let path = original_path(dir, &book.id, book.format);
    if path.is_file() {
        return Ok(());
    }
    fs::create_dir_all(dir.join("originals"))?;
    write_atomic(&path, bytes)
}

/// Saves the EPUB's cover and returns its file name. A cover that can't be saved only costs the
/// book its picture, so the import carries on without one.
fn store_cover(dir: &Path, id: &BookId, bytes: &[u8]) -> Option<String> {
    let cover = crate::epub::cover(bytes)?;
    let name = format!("{}.{}", id.0, cover.extension);
    fs::create_dir_all(dir.join("covers")).ok()?;
    write_atomic(&dir.join("covers").join(&name), &cover.bytes).ok()?;
    Some(name)
}

impl Prepared {
    fn of(book: &Book) -> Prepared {
        Prepared {
            id: book.id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            format: book.format,
            chapters: book.chapters.len(),
            cover: None,
            titles: titles(book),
            before: None,
            site: book.source.as_ref().and_then(|s| s.site.clone().or_else(|| host(&s.url))),
            words: book.chapters.iter().map(|c| c.text.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()).sum(),
            excerpt: book.source.as_ref().and_then(|s| s.excerpt.clone()),
            picture: (book.format == Format::Article).then(|| book.chapters.iter().find_map(|c| first_picture(&c.text))).flatten(),
        }
    }
}

/// The path of the first picture a chapter's Markdown shows from the library's folder: the
/// `![alt](path)` of a picture that was fetched, not the address of one that wasn't.
fn first_picture(markdown: &str) -> Option<String> {
    markdown.match_indices("![").find_map(|(at, _)| {
        let rest = &markdown[at + 2..];
        let target = &rest[rest.find("](")? + 2..];
        let path = target.split([')', ' ']).next()?;
        (!path.is_empty() && !path.contains("://")).then(|| path.to_string())
    })
}

/// The host of a web address, without `www.`.
fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_lowercase())
}

fn store(dir: &Path, book: &Book) -> io::Result<()> {
    fs::create_dir_all(dir.join("books"))?;
    write_atomic(&book_path(dir, &book.id), &serde_json::to_vec(book).map_err(io::Error::other)?)
}

pub fn book_path(dir: &Path, id: &BookId) -> PathBuf {
    dir.join("books").join(format!("{}.json", id.0))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Writes `bytes` to a sibling temporary file and renames it over `path`, so a crash leaves
/// either the old file or the new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importing_twice_reuses_the_entry_and_its_position() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("small.epub");
        fs::write(&file, crate::epub::tests::sample()).unwrap();

        let mut lib = Library::open(dir.path().join("data")).unwrap();
        let first = prepare(lib.dir(), &file).unwrap();
        let id = lib.add(&first).unwrap().id.clone();
        lib.set_position(&id, Position { chapter: 1, fraction: 0.5 }).unwrap();

        let again = prepare(lib.dir(), &file).unwrap();
        assert_eq!(again, first);
        lib.add(&again).unwrap();
        assert_eq!(lib.entries().len(), 1);

        let stamped = lib.entries()[0].position_at;
        assert!(stamped > 0, "a new position is stamped");
        lib.set_position(&id, Position { chapter: 1, fraction: 0.5 }).unwrap();
        assert_eq!(lib.entries()[0].position_at, stamped, "the same position again keeps its stamp");
        lib.set_position(&id, Position { chapter: 1, fraction: 0.6 }).unwrap();
        assert!(lib.entries()[0].position_at > stamped, "a move in the same millisecond still stamps later");
        lib.set_position(&id, Position { chapter: 1, fraction: 0.5 }).unwrap();
        lib.set_archived(&id, true).unwrap();
        lib.add(&prepare(lib.dir(), &file).unwrap()).unwrap();
        assert!(lib.entries()[0].archived > 0, "importing an archived book again leaves it archived");

        let reopened = Library::open(dir.path().join("data")).unwrap();
        assert_eq!(reopened.entries()[0].position, Position { chapter: 1, fraction: 0.5 });
        assert!(reopened.entries()[0].archived > 0, "archiving is kept");
        assert!(reopened.entries()[0].words > 0);
        let mut reopened = reopened;
        reopened.set_archived(&id, false).unwrap();
        let reopened = Library::open(dir.path().join("data")).unwrap();
        assert_eq!(reopened.entries()[0].archived, 0, "unarchiving is kept");
        assert_eq!(reopened.entries()[0].title, "A Small Book");
        assert_eq!(reopened.load(&id).unwrap().chapters.len(), 3);
        assert!((reopened.entries()[0].progress() - 0.5).abs() < 1e-6);

        assert_eq!(reopened.last_opened(), None);
        let mut reopened = reopened;
        reopened.opened(&id).unwrap();
        assert_eq!(Library::open(dir.path().join("data")).unwrap().last_opened().map(|e| &e.id), Some(&id), "the last book opened is kept");
        let other = crate::book::Book { id: BookId("b".repeat(64)), title: "Other".into(), author: None, format: Format::Epub, parser: PARSER, chapters: vec![], source: None };
        reopened.insert(&other).unwrap();
        for _ in 0..3 {
            reopened.opened(&id).unwrap();
        }
        reopened.opened(&other.id).unwrap();
        assert_eq!(reopened.last_opened().map(|e| &e.id), Some(&other.id), "the book opened last wins, however often another was opened in the same second");

        // An article saved twice, the second time from the same address with another fragment.
        let html = crate::article::tests::page();
        let mut fetches = 0;
        let mut fetch = |_: &str| {
            fetches += 1;
            Some(crate::article::tests::PNG.to_vec())
        };
        let saved = prepare_article(lib.dir(), "https://shore.example/2026/tide", &html, &mut fetch).unwrap();
        let again = prepare_article(lib.dir(), "https://shore.example/2026/tide#top", &html, &mut fetch).unwrap();
        assert_eq!((again, fetches), (saved.clone(), 1), "found again by its address, and its picture not fetched again");
        lib.add(&saved).unwrap();
        let article = lib.entry(&saved.id).unwrap();
        assert_eq!((article.format, article.title.as_str(), article.chapters), (Format::Article, "Notes on the tide", 1));
        assert_eq!(fs::read(original_path(lib.dir(), &saved.id, Format::Article)).unwrap(), html.as_bytes(), "the page is kept");
        assert!(lib.dir().join(import::image_dir(&saved.id)).join("1.png").is_file());
        let stored = load(lib.dir(), &saved.id).unwrap();
        assert_eq!(stored.source.as_ref().map(|s| s.url.as_str()), Some("https://shore.example/2026/tide"));
        assert_eq!(open(lib.dir(), &saved.id).unwrap(), (stored, None));

        let leftovers = fs::read_dir(lib.dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_book_added_before_markdown_opens_then_upgrades_in_place_at_its_place() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("small.epub");
        let bytes = crate::epub::tests::sample();
        fs::write(&file, &bytes).unwrap();
        let id = import::book_id(&bytes);
        let data = dir.path().join("data");
        fs::create_dir_all(data.join("books")).unwrap();
        // As the library stored it before: blocks, no original, one chapter fewer than a parse
        // now finds, and the reader two thirds into "Three".
        let old = format!(
            r#"{{"id":"{}","title":"A Small Book","author":null,"format":"Epub","chapters":[
                {{"title":"Chapter Two","blocks":[{{"Paragraph":"The second."}}]}},{{"title":"Three","blocks":[{{"Paragraph":"Third text."}}]}}]}}"#,
            id.0
        );
        fs::write(book_path(&data, &id), &old).unwrap();
        let place = Position { chapter: 1, fraction: 0.66 };
        let entry = format!(
            r#"{{"books":[{{"id":"{}","title":"A Small Book","author":null,"format":"Epub","position":{{"chapter":1,"fraction":0.66}},"added":1,"chapters":2}}]}}"#,
            id.0
        );
        fs::write(data.join("library.json"), entry).unwrap();

        let mut lib = Library::open(&data).unwrap();
        let (book, upgrade) = open(&data, &id).unwrap();
        assert_eq!((book.parser, upgrade), (0, None), "with no original kept it opens as it was stored");
        assert_eq!(book.chapters[1].text, "# Three\n\nThird text.\n");
        assert_eq!(lib.entries()[0].position, place);

        let prepared = prepare(&data, &file).unwrap();
        assert!(prepared.before.is_some(), "adding the same file again parses it anew");
        lib.add(&prepared).unwrap();
        let entry = &lib.entries()[0];
        assert_eq!((lib.entries().len(), entry.chapters), (1, 3));
        assert_eq!(entry.position, Position { chapter: 2, fraction: 0.66 }, "the place follows \"Three\"");
        assert_eq!(load(&data, &id).unwrap().parser, PARSER);
        assert_eq!(fs::read(original_path(&data, &id, Format::Epub)).unwrap(), bytes, "the file is kept");

        // A later parser: the stored book is stale, and opening it parses the original again.
        fs::write(book_path(&data, &id), &old).unwrap();
        let (book, upgrade) = open(&data, &id).unwrap();
        assert_eq!((book.parser, book.chapters.len()), (PARSER, 3));
        assert!(upgrade.as_ref().is_some_and(|u| u.before.as_ref().is_some_and(|b| b.len() == 2)));
        assert_eq!(load(&data, &id).unwrap(), book, "and stores what it read");
    }

    #[test]
    fn a_removed_book_takes_everything_under_its_id_and_comes_back_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("small.epub");
        fs::write(&file, crate::epub::tests::sample()).unwrap();
        let mut lib = Library::open(dir.path().join("data")).unwrap();
        let other = crate::book::Book { id: BookId("b".repeat(64)), title: "Other".into(), author: None, format: Format::Epub, parser: PARSER, chapters: vec![], source: None };
        lib.insert(&other).unwrap();
        let id = lib.add(&prepare(lib.dir(), &file).unwrap()).unwrap().id.clone();
        lib.set_position(&id, Position { chapter: 1, fraction: 0.5 }).unwrap();
        let highlight = Highlight { chapter: 1, start: 0, end: 4, words: "Some".into(), made: 1 };
        lib.set_highlights(&id, std::slice::from_ref(&highlight)).unwrap();
        fs::create_dir_all(lib.dir().join("covers")).unwrap();
        fs::write(lib.dir().join("covers").join(format!("{}.jpg", id.0)), "jpeg").unwrap();
        let files = |lib: &Library| -> Vec<PathBuf> {
            let mut all = vec![];
            for folder in fs::read_dir(lib.dir()).unwrap().flatten().filter(|f| f.path().is_dir() && f.file_name() != REMOVED) {
                all.extend(fs::read_dir(folder.path()).unwrap().flatten().map(|f| f.path()).filter(|p| p.to_string_lossy().contains(&id.0)));
            }
            all.sort();
            all
        };
        let before = files(&lib);
        assert!(before.len() >= 4, "the book, its original, its cover and its highlights: {before:?}");
        let entries = lib.entries().to_vec();

        let removed = lib.remove(&id).unwrap();
        assert!(files(&lib).is_empty());
        assert_eq!(Library::open(lib.dir()).unwrap().entries().iter().map(|e| &e.id).collect::<Vec<_>>(), [&other.id]);
        assert!(lib.load(&other.id).is_ok(), "the other book is untouched");

        assert!(lib.highlights(&id).is_empty(), "its highlights go with it");

        lib.restore(removed).unwrap();
        assert_eq!(files(&lib), before);
        assert_eq!(lib.highlights(&id), [highlight], "and come back with it");
        assert_eq!(Library::open(lib.dir()).unwrap().entries(), entries, "back in its place, where the reader left it");

        let removed = lib.remove(&id).unwrap();
        lib.forget(removed);
        lib.forget_removed();
        assert!(files(&lib).is_empty() && !lib.dir().join(REMOVED).exists());
    }

    #[test]
    fn a_damaged_index_opens_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("library.json"), "{ not json").unwrap();
        assert!(Library::open(dir.path()).unwrap().entries().is_empty());
    }

    #[test]
    fn a_failed_import_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        fs::write(&file, "hi").unwrap();
        let lib = Library::open(dir.path().join("data")).unwrap();
        assert!(matches!(prepare(lib.dir(), &file), Err(ImportError::Unsupported(_))));
        assert_eq!(fs::read_dir(lib.dir().join("books")).unwrap().count(), 0);
    }
}

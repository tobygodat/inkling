//! The books on disk, in the data dir:
//! - `library.json`: one entry per book (id, title, author, format, position, added time).
//! - `books/<id>.json`: the parsed book, its chapters in Markdown. Named by the hash of the file
//!   it came from, so importing the same file again reuses it, and two imports writing it at once
//!   write the same.
//! - `originals/<id>.epub` or `.pdf`: the file as it was added, so a newer parser can read it
//!   again without the reader adding the book again.
//! - `images/<id>/<n>.<ext>`: the images the chapters show.
//! - `covers/<id>.<ext>`: the cover image an EPUB names.
//!
//! Every write goes to a temporary file first and is renamed into place, the parsed book last.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::book::{Book, BookId, Format, PARSER, Position};
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

    pub fn set_position(&mut self, id: &BookId, position: Position) -> io::Result<()> {
        let Some(entry) = self.entries.iter_mut().find(|e| &e.id == id) else { return Ok(()) };
        if entry.position == position {
            return Ok(());
        }
        entry.position = position;
        entry.position_at = now_millis().max(entry.position_at + 1);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let index = Index { books: self.entries.clone() };
        write_atomic(&self.dir.join("library.json"), &serde_json::to_vec_pretty(&index).map_err(io::Error::other)?)
    }
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
    if book.parser >= PARSER {
        return Ok((book, None));
    }
    let Some(original) = original_path(dir, id, book.format).filter(|p| p.is_file()) else { return Ok((book, None)) };
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
pub fn original_path(dir: &Path, id: &BookId, format: Format) -> Option<PathBuf> {
    let extension = match format {
        Format::Epub => "epub",
        Format::Pdf => "pdf",
    };
    Some(dir.join("originals").join(format!("{}.{extension}", id.0)))
}

fn titles(book: &Book) -> Vec<Option<String>> {
    book.chapters.iter().map(|c| c.title.clone()).collect()
}

/// Parses the file and stores everything it needs: the original, its images, then the book.
fn parse_and_store(dir: &Path, path: &Path, bytes: &[u8]) -> Result<Book, ImportError> {
    let unreadable = |e: io::Error| ImportError::Unreadable(e.to_string().to_lowercase());
    let parsed = import::parse(path, bytes)?;
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
    let Some(path) = original_path(dir, &book.id, book.format) else { return Ok(()) };
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
        }
    }
}

fn store(dir: &Path, book: &Book) -> io::Result<()> {
    fs::create_dir_all(dir.join("books"))?;
    write_atomic(&book_path(dir, &book.id), &serde_json::to_vec(book).map_err(io::Error::other)?)
}

pub(crate) fn book_path(dir: &Path, id: &BookId) -> PathBuf {
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

        let reopened = Library::open(dir.path().join("data")).unwrap();
        assert_eq!(reopened.entries()[0].position, Position { chapter: 1, fraction: 0.5 });
        assert_eq!(reopened.entries()[0].title, "A Small Book");
        assert_eq!(reopened.load(&id).unwrap().chapters.len(), 3);
        assert!((reopened.entries()[0].progress() - 0.5).abs() < 1e-6);

        assert_eq!(reopened.last_opened(), None);
        let mut reopened = reopened;
        reopened.opened(&id).unwrap();
        assert_eq!(Library::open(dir.path().join("data")).unwrap().last_opened().map(|e| &e.id), Some(&id), "the last book opened is kept");
        let other = crate::book::Book { id: BookId("b".repeat(64)), title: "Other".into(), author: None, format: Format::Epub, parser: PARSER, chapters: vec![] };
        reopened.insert(&other).unwrap();
        for _ in 0..3 {
            reopened.opened(&id).unwrap();
        }
        reopened.opened(&other.id).unwrap();
        assert_eq!(reopened.last_opened().map(|e| &e.id), Some(&other.id), "the book opened last wins, however often another was opened in the same second");

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
        assert_eq!(fs::read(original_path(&data, &id, Format::Epub).unwrap()).unwrap(), bytes, "the file is kept");

        // A later parser: the stored book is stale, and opening it parses the original again.
        fs::write(book_path(&data, &id), &old).unwrap();
        let (book, upgrade) = open(&data, &id).unwrap();
        assert_eq!((book.parser, book.chapters.len()), (PARSER, 3));
        assert!(upgrade.as_ref().is_some_and(|u| u.before.as_ref().is_some_and(|b| b.len() == 2)));
        assert_eq!(load(&data, &id).unwrap(), book, "and stores what it read");
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

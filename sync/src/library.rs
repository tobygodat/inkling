//! The books and reading positions in step, and Readwise uploads into the library.
//!
//! A book's id is the hash of its file and books are only added, so books never conflict: each
//! device sends up what the server lacks and brings down what it lacks. A position carries when
//! it moved, and the later one wins in both directions, so it follows the device read last.
//!
//! [`sync`] runs off the UI thread. It stores the parsed files and covers it brings down (they
//! are named by their hash, so writing them is safe from any thread) and leaves the index,
//! `library.json`, to the app, which records [`Outcome`] on the UI thread.

use crate::api::{Api, Body};
use crate::transport::MAX_FILE_BYTES;
use crate::{Error, Method};
use inkling_books::library::{book_path, write_atomic};
use inkling_books::{Book, BookId, Entry, Format, Library, Position, Prepared, import};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct Listing {
    books: Vec<Row>,
    #[serde(default)]
    inbox: Vec<InboxItem>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    title: String,
    author: Option<String>,
    format: Format,
    chapters: usize,
    cover: Option<String>,
    added: u64,
    #[serde(default)]
    chapter: usize,
    #[serde(default)]
    fraction: f32,
    #[serde(default)]
    position_at: u64,
}

#[derive(Deserialize)]
struct InboxItem {
    readwise_id: String,
    category: String,
    state: String,
}

/// What the app records in its library on the UI thread.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Books another device added. Their parsed files and covers are already stored.
    pub books: Vec<Entry>,
    /// Places read on another device after this one, and when.
    pub positions: Vec<(BookId, Position, u64)>,
    /// Readwise uploads, parsed and stored.
    pub imported: Vec<Prepared>,
    /// Books that couldn't go up or come down, with why. The rest carry on.
    pub problems: Vec<String>,
}

/// Brings the library in `dir` and the server into step. A book that can't move is a problem
/// and the rest go on; being offline or signed out stops it.
pub fn sync(api: &Api, dir: &Path) -> Result<Outcome, Error> {
    let listing: Listing = api.json(Method::Get, "/v1/library", Body::None)?;
    let entries = Library::open(dir).map_err(|e| Error::Local(format!("The library couldn't be read ({e}).")))?.entries().to_vec();
    let mut out = Outcome::default();

    // Oldest first, so the server lists them in the order they were added.
    for entry in entries.iter().rev() {
        let there = listing.books.iter().find(|b| b.id == entry.id.0);
        let sent = match there {
            None => upload(api, dir, entry),
            Some(b) if b.cover.is_none() && entry.cover.is_some() => send_cover(api, dir, entry).and_then(|()| send_row(api, entry)),
            Some(_) => Ok(()),
        };
        match sent {
            Ok(()) => {}
            Err(Error::Refused { message, .. } | Error::Local(message)) => {
                out.problems.push(format!("{} didn't go up: {message}", entry.title));
                continue;
            }
            Err(stop) => return Err(stop),
        }
        match there {
            Some(b) if b.position_at > entry.position_at => {
                out.positions.push((entry.id.clone(), Position { chapter: b.chapter, fraction: b.fraction }, b.position_at));
            }
            _ if entry.position_at > there.map_or(0, |b| b.position_at) => {
                let position = json!({ "chapter": entry.position.chapter, "fraction": entry.position.fraction, "at": entry.position_at });
                api.send(Method::Put, &format!("/v1/books/{}/position", entry.id.0), Body::Json(position))?;
            }
            _ => {}
        }
    }

    for row in listing.books.iter().filter(|b| !entries.iter().any(|e| e.id.0 == b.id)) {
        match download(api, dir, row) {
            Ok(entry) => out.books.push(entry),
            Err(Error::Refused { message, .. } | Error::Local(message)) => out.problems.push(format!("{} didn't come down: {message}", row.title)),
            Err(stop) => return Err(stop),
        }
    }

    for item in listing.inbox.iter().filter(|i| i.state == "waiting") {
        let bytes = match api.send(Method::Get, &format!("/v1/inbox/{}/file", item.readwise_id), Body::None) {
            Ok(bytes) => bytes,
            // Another device took it first.
            Err(Error::Refused { status: 404, .. }) => continue,
            Err(e) => return Err(e),
        };
        let settled = match import_upload(dir, item, &bytes) {
            Ok(prepared) => {
                let settled = json!({ "state": "imported", "book_id": prepared.id.0 });
                out.imported.push(prepared);
                settled
            }
            Err(why) => json!({ "state": "failed", "error": why }),
        };
        api.send(Method::Post, &format!("/v1/inbox/{}", item.readwise_id), Body::Json(settled))?;
    }
    Ok(out)
}

/// Parses a Readwise upload into the library's folder, as Add would.
fn import_upload(dir: &Path, item: &InboxItem, bytes: &[u8]) -> Result<Prepared, String> {
    let extension = if item.category == "pdf" { "pdf" } else { "epub" };
    let incoming = dir.join("incoming");
    fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;
    let file = incoming.join(format!("{}.{extension}", import::book_id(bytes).0));
    write_atomic(&file, bytes).map_err(|e| e.to_string())?;
    let prepared = inkling_books::prepare(dir, &file).map_err(|e| e.to_string());
    let _ = fs::remove_file(&file);
    prepared
}

/// The parsed file, every chapter, its images, the cover, then the row: the server refuses the
/// row until the file and chapters are there, so a sync cut short leaves no half book in an
/// agent's list.
fn upload(api: &Api, dir: &Path, entry: &Entry) -> Result<(), Error> {
    let bytes = fs::read(book_path(dir, &entry.id)).map_err(|e| Error::Local(format!("its file couldn't be read ({e})")))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Local("it is larger than the 25 MB the server keeps".into()));
    }
    let book: Book = serde_json::from_slice(&bytes).map_err(|e| Error::Local(format!("its file couldn't be read ({e})")))?;
    api.send(Method::Put, &format!("/v1/books/{}/file", entry.id.0), Body::Bytes(&bytes))?;
    for (at, chapter) in book.chapters.iter().enumerate() {
        let title = chapter.title.clone().unwrap_or_else(|| format!("Chapter {}", at + 1));
        let body = json!({ "title": title, "text": chapter.text });
        api.send(Method::Put, &format!("/v1/books/{}/chapters/{at}", entry.id.0), Body::Json(body))?;
    }
    for name in images(&book) {
        let Ok(bytes) = fs::read(dir.join(import::image_dir(&entry.id)).join(&name)) else { continue };
        api.send(Method::Put, &format!("/v1/books/{}/images/{name}", entry.id.0), Body::Bytes(&bytes))?;
    }
    send_cover(api, dir, entry)?;
    send_row(api, entry)
}

fn send_cover(api: &Api, dir: &Path, entry: &Entry) -> Result<(), Error> {
    let Some(name) = &entry.cover else { return Ok(()) };
    let bytes = fs::read(dir.join("covers").join(name)).map_err(|e| Error::Local(format!("its cover couldn't be read ({e})")))?;
    api.send(Method::Put, &format!("/v1/covers/{name}"), Body::Bytes(&bytes)).map(drop)
}

fn send_row(api: &Api, entry: &Entry) -> Result<(), Error> {
    let row = json!({
        "title": entry.title,
        "author": entry.author,
        "format": entry.format,
        "chapters": entry.chapters,
        "cover": entry.cover,
        "added": entry.added,
    });
    api.send(Method::Put, &format!("/v1/books/{}", entry.id.0), Body::Json(row)).map(drop)
}

/// Stores a book another device added: its parsed file, checked to be one, and its cover.
fn download(api: &Api, dir: &Path, row: &Row) -> Result<Entry, Error> {
    let id = BookId(row.id.clone());
    let bytes = api.send(Method::Get, &format!("/v1/books/{}/file", row.id), Body::None)?;
    let book: Book = serde_json::from_slice(&bytes).map_err(|e| Error::Local(format!("its file couldn't be read ({e})")))?;
    if book.id != id {
        return Err(Error::Local("its file is another book".into()));
    }
    let folder = dir.join(import::image_dir(&id));
    for name in images(&book) {
        match api.send(Method::Get, &format!("/v1/books/{}/images/{name}", row.id), Body::None) {
            Ok(image) => fs::create_dir_all(&folder).and_then(|()| write_atomic(&folder.join(&name), &image)).map_err(|e| Error::Local(e.to_string()))?,
            // A picture the other device couldn't send shows as its alt text.
            Err(Error::Refused { status: 404, .. }) => {}
            Err(e) => return Err(e),
        }
    }
    write_atomic(&book_path(dir, &id), &bytes).map_err(|e| Error::Local(e.to_string()))?;
    let cover = match &row.cover {
        Some(name) if !name.contains('/') && !name.starts_with('.') => {
            let image = api.send(Method::Get, &format!("/v1/covers/{name}"), Body::None)?;
            fs::create_dir_all(dir.join("covers")).and_then(|()| write_atomic(&dir.join("covers").join(name), &image)).ok().map(|()| name.clone())
        }
        _ => None,
    };
    Ok(Entry {
        id,
        title: row.title.clone(),
        author: row.author.clone(),
        format: row.format,
        position: Position { chapter: row.chapter, fraction: row.fraction },
        added: row.added,
        chapters: row.chapters,
        cover,
        opened: 0,
        position_at: row.position_at,
        archived: 0,
        site: None,
        words: 0,
    })
}

/// The file names of the pictures a book's chapters show, as `images/<id>/<n>.<ext>` links name them.
fn images(book: &Book) -> Vec<String> {
    let prefix = format!("{}/", import::image_dir(&book.id));
    let mut names: Vec<String> = book
        .chapters
        .iter()
        .flat_map(|c| c.text.match_indices(&prefix).map(|(at, _)| &c.text[at + prefix.len()..]))
        .filter_map(|rest| {
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
            (name.contains('.') && !name.starts_with('.')).then_some(name)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fake;
    use inkling_books::Chapter;

    fn book(title: &str) -> Book {
        let id = import::book_id(title.as_bytes());
        let text = format!("{title}, page one.\n\n![The pond](images/{}/1.png \"In winter\")\n", id.0);
        Book { id, title: title.into(), author: None, format: Format::Epub, parser: inkling_books::book::PARSER, chapters: vec![Chapter { title: None, text }], source: None }
    }

    #[test]
    fn a_book_crosses_once_and_the_place_read_last_wins_both_ways() {
        let fake = Fake::memory();
        let begun = crate::account::begin(&fake, crate::FAKE_SERVER, "inkling://callback", "test").unwrap();
        let token = begun.finish(&fake, &crate::fake_way_back(&begun.url).unwrap()).unwrap().token(&fake).unwrap();
        let api = Api::new(&fake, crate::FAKE_SERVER, token);
        let (mac, phone) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let walden = book("Walden");

        let mut library = Library::open(mac.path()).unwrap();
        library.insert(&walden).unwrap();
        let pictures = mac.path().join(import::image_dir(&walden.id));
        fs::create_dir_all(&pictures).unwrap();
        fs::write(pictures.join("1.png"), b"png").unwrap();
        library.set_position(&walden.id, Position { chapter: 0, fraction: 0.25 }).unwrap();
        assert_eq!(sync(&api, mac.path()).unwrap(), Outcome::default());

        let came = sync(&api, phone.path()).unwrap();
        assert_eq!(came.books.len(), 1);
        assert_eq!(came.books[0].position, Position { chapter: 0, fraction: 0.25 });
        let mut on_phone = Library::open(phone.path()).unwrap();
        on_phone.record(came.books[0].clone()).unwrap();
        assert_eq!(on_phone.load(&walden.id).unwrap(), walden);
        assert_eq!(fs::read(phone.path().join(import::image_dir(&walden.id)).join("1.png")).unwrap(), b"png", "its pictures come with it");

        // Read further on the phone: the Mac takes that place, and doesn't send its older one back.
        let later = library.entry(&walden.id).unwrap().position_at + 10;
        on_phone.take_position(&walden.id, Position { chapter: 0, fraction: 0.75 }, later).unwrap();
        assert_eq!(sync(&api, phone.path()).unwrap(), Outcome::default());
        let back = sync(&api, mac.path()).unwrap();
        assert_eq!(back.positions, vec![(walden.id.clone(), Position { chapter: 0, fraction: 0.75 }, later)]);
        assert!(library.take_position(&walden.id, Position { chapter: 0, fraction: 0.75 }, later).unwrap());
        assert!(!library.take_position(&walden.id, Position { chapter: 0, fraction: 0.1 }, later).unwrap());
        assert_eq!(sync(&api, mac.path()).unwrap(), Outcome::default());
    }
}

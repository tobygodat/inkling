//! `inkling-mcp sync`: this Mac's library and draft go up to Inkling's server, so an agent
//! connected there reads what the Mac has, and Readwise uploads waiting in the server's inbox come
//! down into a folder that Find books looks through (ADR 0004).
//!
//! It only reads the app's data folder. The running app owns `library.json` and would write over
//! an entry added behind its back, so a Readwise file joins the library when the writer adds it.
//! Positions and the draft only go up for the same reason: the Mac is the one device that syncs.

use crate::store::{Folder, Store};
use crate::tools::{chapter_title, markdown};
use inkling_books::{Book, Entry, Format, import};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The largest file the server keeps, as `MAX_FILE_BYTES` in `server/src/http.ts`.
pub const MAX_FILE_BYTES: usize = 25 * 1024 * 1024;

pub enum Body<'a> {
    None,
    Json(Value),
    Bytes(&'a [u8]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Put,
    Post,
    Delete,
}

/// The server's sync API under `/v1`, with a token already in hand.
pub trait Remote {
    /// The response body of a 2xx answer.
    fn send(&self, method: Method, path: &str, body: Body) -> Result<Vec<u8>, Error>;

    fn json(&self, method: Method, path: &str, body: Body) -> Result<Value, Error> {
        let bytes = self.send(method, path, body)?;
        serde_json::from_slice(&bytes).map_err(|e| Error::Refused { status: 200, message: format!("The server sent an answer this can't read ({e}).") })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The sign-in has ended or was never made.
    SignedOut,
    /// The server turned the request down, and said why.
    Refused { status: u16, message: String },
    /// The server couldn't be reached.
    Offline,
    /// Something on this Mac.
    Local(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::SignedOut => write!(f, "You're signed out of Inkling's server. Run `inkling-mcp sign-in` first."),
            Error::Refused { message, .. } => write!(f, "{message}"),
            Error::Offline => write!(f, "Inkling's server couldn't be reached. Nothing was lost; run it again when you're online."),
            Error::Local(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Deserialize)]
struct Library {
    books: Vec<RemoteBook>,
    inbox: Vec<InboxItem>,
}

#[derive(Deserialize)]
struct RemoteBook {
    id: String,
    cover: Option<String>,
    position_at: u64,
}

#[derive(Deserialize)]
struct InboxItem {
    readwise_id: String,
    title: Option<String>,
    category: String,
    state: String,
}

#[derive(Deserialize)]
struct Draft {
    text: String,
    saved_at: u64,
}

/// What a sync did, for the command to print.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// Titles of the books sent up.
    pub uploaded: Vec<String>,
    /// Books that couldn't go up, with why.
    pub skipped: Vec<(String, String)>,
    pub positions: usize,
    pub draft: bool,
    /// Readwise files saved for Find books.
    pub fetched: Vec<PathBuf>,
}

/// Brings the server up to date with `folder` and saves waiting Readwise files in `inbox_dir`.
/// A book the server refuses is skipped and the rest go on; being offline or signed out stops it.
pub fn sync(folder: &Folder, remote: &impl Remote, inbox_dir: &Path) -> Result<Report, Error> {
    let library: Library = serde_json::from_value(remote.json(Method::Get, "/v1/library", Body::None)?)
        .map_err(|e| Error::Refused { status: 200, message: format!("The server's library couldn't be read ({e}).") })?;
    let entries = folder.books().map_err(|e| Error::Local(format!("The library couldn't be read ({e}).")))?;
    let mut report = Report::default();

    // Oldest first, so the server lists them in the order they were added.
    for entry in entries.iter().rev() {
        let there = library.books.iter().find(|b| b.id == entry.id.0);
        let sent = match there {
            None => upload(folder, remote, entry),
            Some(b) if b.cover.is_none() && entry.cover.is_some() => send_cover(folder, remote, entry).and_then(|()| send_row(remote, entry)),
            Some(_) => Ok(()),
        };
        match sent {
            Ok(()) => {
                if there.is_none() {
                    report.uploaded.push(entry.title.clone());
                }
            }
            Err(Error::Refused { message, .. } | Error::Local(message)) => {
                report.skipped.push((entry.title.clone(), message));
                continue;
            }
            Err(stop) => return Err(stop),
        }
        if entry.position_at > there.map_or(0, |b| b.position_at) {
            let position = json!({ "chapter": entry.position.chapter, "fraction": entry.position.fraction, "at": entry.position_at });
            remote.send(Method::Put, &format!("/v1/books/{}/position", entry.id.0), Body::Json(position))?;
            report.positions += 1;
        }
    }

    let text = folder.draft().map_err(|e| Error::Local(format!("The draft couldn't be read ({e}).")))?;
    let saved_at = folder.draft_saved_at();
    let theirs: Draft = serde_json::from_value(remote.json(Method::Get, "/v1/draft", Body::None)?)
        .map_err(|e| Error::Refused { status: 200, message: format!("The server's draft couldn't be read ({e}).") })?;
    if text != theirs.text && saved_at > theirs.saved_at {
        remote.send(Method::Put, "/v1/draft", Body::Json(json!({ "text": text, "saved_at": saved_at })))?;
        report.draft = true;
    }

    for item in library.inbox.iter().filter(|i| i.state == "waiting") {
        let bytes = match remote.send(Method::Get, &format!("/v1/inbox/{}/file", item.readwise_id), Body::None) {
            Ok(bytes) => bytes,
            // Another device took it first.
            Err(Error::Refused { status: 404, .. }) => continue,
            Err(e) => return Err(e),
        };
        let path = save_for_find_books(inbox_dir, item, &bytes).map_err(|e| Error::Local(format!("A Readwise file couldn't be saved in {} ({e}).", inbox_dir.display())))?;
        let settled = json!({ "state": "imported", "book_id": import::book_id(&bytes).0 });
        remote.send(Method::Post, &format!("/v1/inbox/{}", item.readwise_id), Body::Json(settled))?;
        report.fetched.push(path);
    }
    Ok(report)
}

/// The parsed file, every chapter, the cover, then the row: the server refuses the row until the
/// rest is there, so a sync cut short leaves no half book in an agent's list.
fn upload(folder: &Folder, remote: &impl Remote, entry: &Entry) -> Result<(), Error> {
    let bytes = folder.book_file(&entry.id).map_err(|e| Error::Local(format!("its file couldn't be read ({e})")))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Local("it is larger than the 25 MB the server keeps".into()));
    }
    let book: Book = serde_json::from_slice(&bytes).map_err(|e| Error::Local(format!("its file couldn't be read ({e})")))?;
    remote.send(Method::Put, &format!("/v1/books/{}/file", entry.id.0), Body::Bytes(&bytes))?;
    for (at, chapter) in book.chapters.iter().enumerate() {
        let body = json!({ "title": chapter_title(chapter, at), "text": markdown(chapter, at) });
        remote.send(Method::Put, &format!("/v1/books/{}/chapters/{at}", entry.id.0), Body::Json(body))?;
    }
    if entry.cover.is_some() {
        send_cover(folder, remote, entry)?;
    }
    send_row(remote, entry)
}

fn send_cover(folder: &Folder, remote: &impl Remote, entry: &Entry) -> Result<(), Error> {
    let Some(name) = &entry.cover else { return Ok(()) };
    let bytes = folder.cover_file(name).map_err(|e| Error::Local(format!("its cover couldn't be read ({e})")))?;
    remote.send(Method::Put, &format!("/v1/covers/{name}"), Body::Bytes(&bytes)).map(drop)
}

fn send_row(remote: &impl Remote, entry: &Entry) -> Result<(), Error> {
    let row = json!({
        "title": entry.title,
        "author": entry.author,
        "format": match entry.format { Format::Epub => "Epub", Format::Pdf => "Pdf" },
        "chapters": entry.chapters,
        "cover": entry.cover,
        "added": entry.added,
    });
    remote.send(Method::Put, &format!("/v1/books/{}", entry.id.0), Body::Json(row)).map(drop)
}

/// Saves the file under its title. A different file already by that name keeps it, and this one
/// takes the Readwise id as well; the same file again is left as it is.
fn save_for_find_books(dir: &Path, item: &InboxItem, bytes: &[u8]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let extension = if item.category == "pdf" { "pdf" } else { "epub" };
    let title = item.title.as_deref().map(file_name).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Readwise {}", item.readwise_id));
    let mut path = dir.join(format!("{title}.{extension}"));
    if fs::read(&path).is_ok_and(|there| there != bytes) {
        path = dir.join(format!("{title} ({}).{extension}", item.readwise_id));
    }
    inkling_books::library::write_atomic(&path, bytes)?;
    Ok(path)
}

/// A title as a file name: no path separators or control characters, and not too long.
fn file_name(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if c == '/' || c == ':' || c.is_control() { ' ' } else { c }).collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // No hidden files, and no `..`.
    let shown = joined.trim_start_matches(['.', ' ']);
    shown.chars().take(120).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkling_books::{Block, Chapter, Library, Position};
    use std::cell::RefCell;

    /// Answers with what the test set up and remembers every request.
    struct Fake {
        library: Value,
        draft: Value,
        files: Vec<(String, Vec<u8>)>,
        calls: RefCell<Vec<(Method, String, Value)>>,
    }

    impl Remote for Fake {
        fn send(&self, method: Method, path: &str, body: Body) -> Result<Vec<u8>, Error> {
            let logged = match body {
                Body::None => Value::Null,
                Body::Json(v) => v,
                Body::Bytes(b) => json!(b.len()),
            };
            self.calls.borrow_mut().push((method, path.to_string(), logged));
            let answer = match (method, path) {
                (Method::Get, "/v1/library") => self.library.to_string().into_bytes(),
                (Method::Get, "/v1/draft") => self.draft.to_string().into_bytes(),
                (Method::Get, _) => match self.files.iter().find(|(p, _)| p == path) {
                    Some((_, bytes)) => bytes.clone(),
                    None => return Err(Error::Refused { status: 404, message: "Not found.".into() }),
                },
                _ => b"{\"ok\":true}".to_vec(),
            };
            Ok(answer)
        }
    }

    fn book(title: &str, chapters: usize) -> Book {
        let id = import::book_id(title.as_bytes());
        let chapters = (0..chapters).map(|i| Chapter { title: None, blocks: vec![Block::Paragraph(format!("Page {i}."))] }).collect();
        Book { id, title: title.into(), author: Some("A. Writer".into()), format: Format::Epub, chapters }
    }

    #[test]
    fn a_sync_sends_what_the_server_lacks_in_the_order_it_accepts() {
        let data = tempfile::tempdir().unwrap();
        let mut library = Library::open(data.path().join("library")).unwrap();
        let (old, new) = (book("Old", 1), book("New", 2));
        library.insert(&old).unwrap();
        library.insert(&new).unwrap();
        library.set_position(&old.id, Position { chapter: 0, fraction: 0.5 }).unwrap();
        // New has a cover, which only an EPUB import sets, so it is written in by hand.
        let cover = format!("{}.jpg", new.id.0);
        fs::create_dir_all(data.path().join("library/covers")).unwrap();
        fs::write(data.path().join("library/covers").join(&cover), b"jpeg").unwrap();
        let mut entries = library.entries().to_vec();
        entries[0].cover = Some(cover.clone());
        fs::write(data.path().join("library/library.json"), json!({ "books": entries }).to_string()).unwrap();
        let old_at = entries[1].position_at;
        fs::write(data.path().join("draft.md"), "My draft.").unwrap();

        let fake = Fake {
            library: json!({
                "books": [{ "id": old.id.0, "cover": null, "position_at": old_at - 1 }],
                "draft": null,
                "inbox": [
                    { "readwise_id": "01waiting", "title": "Saved: A Paper/Draft", "category": "pdf", "state": "waiting" },
                    { "readwise_id": "01taken", "title": "Gone", "category": "epub", "state": "waiting" },
                    { "readwise_id": "01big", "title": "Big", "category": "epub", "state": "too_large" },
                ],
            }),
            draft: json!({ "version": 0, "text": "", "saved_at": 0 }),
            files: vec![("/v1/inbox/01waiting/file".into(), b"%PDF paper".to_vec())],
            calls: RefCell::new(Vec::new()),
        };
        let inbox = tempfile::tempdir().unwrap();
        let report = sync(&Folder::new(data.path()), &fake, inbox.path()).unwrap();

        let id = |b: &Book| b.id.0.clone();
        let calls: Vec<(Method, String)> = fake.calls.borrow().iter().map(|(m, p, _)| (*m, p.clone())).collect();
        let expected = vec![
            (Method::Get, "/v1/library".to_string()),
            (Method::Put, format!("/v1/books/{}/position", id(&old))),
            (Method::Put, format!("/v1/books/{}/file", id(&new))),
            (Method::Put, format!("/v1/books/{}/chapters/0", id(&new))),
            (Method::Put, format!("/v1/books/{}/chapters/1", id(&new))),
            (Method::Put, format!("/v1/covers/{cover}")),
            (Method::Put, format!("/v1/books/{}", id(&new))),
            (Method::Get, "/v1/draft".to_string()),
            (Method::Put, "/v1/draft".to_string()),
            (Method::Get, "/v1/inbox/01waiting/file".to_string()),
            (Method::Post, "/v1/inbox/01waiting".to_string()),
            (Method::Get, "/v1/inbox/01taken/file".to_string()),
        ];
        assert_eq!(calls, expected);

        let sent = fake.calls.borrow();
        assert_eq!(sent[1].2, json!({ "chapter": 0, "fraction": 0.5, "at": old_at }));
        assert_eq!(sent[4].2, json!({ "title": "Chapter 2", "text": "# Chapter 2\n\nPage 1.\n" }), "a chapter goes up as read_book shows it");
        assert_eq!(sent[6].2["cover"], json!(cover));
        assert_eq!(sent[6].2["format"], json!("Epub"));
        assert_eq!(sent[8].2["text"], json!("My draft."));
        assert_eq!(sent[10].2, json!({ "state": "imported", "book_id": import::book_id(b"%PDF paper").0 }));

        let saved = inbox.path().join("Saved A Paper Draft.pdf");
        assert_eq!(fs::read(&saved).unwrap(), b"%PDF paper");
        assert_eq!(report, Report { uploaded: vec!["New".into()], skipped: vec![], positions: 1, draft: true, fetched: vec![saved] });
    }

    #[test]
    fn a_sync_with_nothing_new_only_asks() {
        let data = tempfile::tempdir().unwrap();
        let mut library = Library::open(data.path().join("library")).unwrap();
        let only = book("Only", 1);
        library.insert(&only).unwrap();
        fs::write(data.path().join("draft.md"), "Same.").unwrap();
        let fake = Fake {
            library: json!({ "books": [{ "id": only.id.0, "cover": null, "position_at": 0 }], "draft": null, "inbox": [] }),
            // The server's copy is the same text, or a later save from elsewhere.
            draft: json!({ "version": 3, "text": "Same.", "saved_at": 1 }),
            files: vec![],
            calls: RefCell::new(Vec::new()),
        };
        let report = sync(&Folder::new(data.path()), &fake, data.path()).unwrap();
        assert_eq!(report, Report::default());
        assert_eq!(fake.calls.borrow().iter().filter(|(m, ..)| *m != Method::Get).count(), 0);
    }

    #[test]
    fn a_file_name_keeps_the_title_readable() {
        assert_eq!(file_name("Saved: A Paper/Draft"), "Saved A Paper Draft");
        assert_eq!(file_name("../../etc"), "etc");
        assert_eq!(file_name("  \n "), "");
    }
}

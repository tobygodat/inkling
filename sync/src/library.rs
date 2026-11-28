//! The books, articles, reading positions, highlights and incoming items in step with the server.
//!
//! A book's id is the hash of its file (or canonical URL for an article) and books are only added,
//! so books never conflict: each device sends up what the server lacks and brings down what it
//! lacks. A position carries when it moved, and the later one wins in both directions, so it
//! follows the device read last. Incoming raw HTML pages (saved URLs, feed items and newsletters)
//! are parsed here with `inkling-books` (`prepare_article`, TOB-534) so there is only one parser.
//!
//! [`sync`] runs off the UI thread. It stores the parsed files and covers it brings down (they
//! are named by their hash, so writing them is safe from any thread) and leaves the index,
//! `library.json`, and highlights files to the app, which records [`Outcome`] on the UI thread.

use crate::api::{Api, Body};
use crate::transport::MAX_FILE_BYTES;
use crate::{Error, Method};
use inkling_books::library::{book_path, write_atomic};
use inkling_books::{Book, BookId, Entry, Format, Highlight, Library, Position, Prepared, highlights, import};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct Listing {
    books: Vec<Row>,
    #[serde(default)]
    inbox: Vec<InboxItem>,
    #[serde(default)]
    incoming: Vec<IncomingItem>,
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
    #[serde(default)]
    inbox: bool,
}

#[derive(Deserialize)]
struct InboxItem {
    readwise_id: String,
    category: String,
    state: String,
}

#[derive(Deserialize)]
struct IncomingItem {
    id: String,
    destination: String,
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    site: Option<String>,
    state: String,
}

#[derive(Deserialize)]
struct HighlightsListing {
    highlights: Vec<ServerHighlight>,
}

#[derive(Deserialize)]
struct ServerHighlight {
    book_id: String,
    chapter: usize,
    start: usize,
    end: usize,
    words: String,
    made: u64,
}

/// What the app records in its library on the UI thread.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Books another device added. Their parsed files and covers are already stored.
    pub books: Vec<Entry>,
    /// Places read on another device after this one, and when.
    pub positions: Vec<(BookId, Position, u64)>,
    /// Readwise uploads, saved URLs, feed items and newsletters, parsed and stored.
    pub imported: Vec<Prepared>,
    /// Merged highlights to save for books that received highlights from another device.
    pub highlights: Vec<(BookId, Vec<Highlight>)>,
    /// Books that couldn't go up or come down, with why. The rest carry on.
    pub problems: Vec<String>,
}

/// Brings the library in `dir` and the server into step. A book that can't move is a problem
/// and the rest go on; being offline or signed out stops it.
pub fn sync(api: &Api, dir: &Path) -> Result<Outcome, Error> {
    let listing: Listing = api.json(Method::Get, "/v1/library", Body::None)?;
    let entries = Library::open(dir).map_err(|e| Error::Local(format!("The library couldn't be read ({e}).")))?.entries().to_vec();
    let mut out = Outcome::default();

    let mut on_server: Vec<String> = listing.books.iter().map(|b| b.id.clone()).collect();

    // Oldest first, so the server lists them in the order they were added.
    for entry in entries.iter().rev() {
        let there = listing.books.iter().find(|b| b.id == entry.id.0);
        let sent = match there {
            None => upload(api, dir, entry),
            Some(b) if b.cover.is_none() && entry.cover.is_some() => send_cover(api, dir, entry).and_then(|()| send_row(api, dir, entry, None)),
            Some(_) => Ok(()),
        };
        match sent {
            Ok(()) => {
                if there.is_none() {
                    on_server.push(entry.id.0.clone());
                }
            }
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

    // Sync highlights before settling any inbox or incoming items, and only for books already on
    // the server so Readwise always has the book's title, author and source URL.
    sync_highlights(api, dir, &entries, &out.books, &on_server, &mut out.highlights)?;

    // Once any item has been settled on the server, never drop `out` on a later network error:
    // the server already deleted its staged file, so the app must still record `out.imported`.
    for item in listing.inbox.iter().filter(|i| i.state == "waiting") {
        let bytes = match api.send(Method::Get, &format!("/v1/inbox/{}/file", item.readwise_id), Body::None) {
            Ok(bytes) => bytes,
            // Another device took it first.
            Err(Error::Refused { status: 404, .. }) => continue,
            Err(e) if !out.imported.is_empty() => {
                out.problems.push(format!("Inbox sync stopped early: {e}"));
                return Ok(out);
            }
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
        if let Err(e) = api.send(Method::Post, &format!("/v1/inbox/{}", item.readwise_id), Body::Json(settled)) {
            if !out.imported.is_empty() {
                out.problems.push(format!("Inbox item couldn't be settled: {e}"));
                return Ok(out);
            }
            return Err(e);
        }
    }

    for item in listing.incoming.iter().filter(|i| i.state == "waiting") {
        let bytes = match api.send(Method::Get, &format!("/v1/incoming/{}/file", item.id), Body::None) {
            Ok(bytes) => bytes,
            // Another device settled it first.
            Err(Error::Refused { status: 404, .. }) => continue,
            Err(e) if !out.imported.is_empty() => {
                out.problems.push(format!("Incoming sync stopped early: {e}"));
                return Ok(out);
            }
            Err(e) => return Err(e),
        };
        let settled = match import_incoming(api, dir, item, &bytes) {
            Ok(prepared) => {
                let settled = json!({ "state": "imported", "book_id": prepared.id.0 });
                out.imported.push(prepared);
                settled
            }
            Err(why) => json!({ "state": "failed", "error": why }),
        };
        if let Err(e) = api.send(Method::Post, &format!("/v1/incoming/{}", item.id), Body::Json(settled)) {
            if !out.imported.is_empty() {
                out.problems.push(format!("Incoming item couldn't be settled: {e}"));
                return Ok(out);
            }
            return Err(e);
        }
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

/// Parses a staged HTML article (from `/save`, a feed, or a newsletter) with `inkling-books`.
fn import_incoming(api: &Api, dir: &Path, item: &IncomingItem, bytes: &[u8]) -> Result<Prepared, String> {
    let html = String::from_utf8_lossy(bytes);
    let transport = api.transport();
    let mut fetch_picture = |url: &str| {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return None;
        }
        let response = transport.send(Method::Get, url, &[], None).ok()?;
        (200..300).contains(&response.status).then_some(response.body)
    };
    let mut prepared = inkling_books::prepare_article(dir, &item.url, &html, &mut fetch_picture).map_err(|e| e.to_string())?;
    if prepared.author.is_none() {
        prepared.author = item.author.clone();
    }
    if prepared.site.is_none() {
        prepared.site = item.site.clone();
    }
    if prepared.title.is_empty()
        && let Some(title) = &item.title
    {
        prepared.title = title.clone();
    }
    prepared.inbox = item.destination == "inbox";
    Ok(prepared)
}

fn highlight_key(h: &Highlight) -> (usize, usize, usize, &str) {
    (h.chapter, h.start, h.end, h.words.as_str())
}

/// Syncs highlights in both directions for every book on this device. Uploading pushes unsent
/// highlights to Readwise on the server when connected.
fn sync_highlights(
    api: &Api,
    dir: &Path,
    local_entries: &[Entry],
    downloaded: &[Entry],
    on_server: &[String],
    out: &mut Vec<(BookId, Vec<Highlight>)>,
) -> Result<(), Error> {
    let listed: HighlightsListing = match api.json(Method::Get, "/v1/highlights", Body::None) {
        Ok(listed) => listed,
        // Older server without highlights sync: leave local highlights untouched.
        Err(Error::Refused { status: 404, .. }) => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut remote_by_book: BTreeMap<String, Vec<Highlight>> = BTreeMap::new();
    for h in listed.highlights {
        remote_by_book.entry(h.book_id).or_default().push(Highlight {
            chapter: h.chapter,
            start: h.start,
            end: h.end,
            words: h.words,
            made: h.made,
        });
    }

    for entry in local_entries.iter().chain(downloaded.iter()) {
        let local = highlights::load(dir, &entry.id);
        let remote = remote_by_book.get(&entry.id.0).map(Vec::as_slice).unwrap_or(&[]);

        if on_server.iter().any(|id| id == &entry.id.0) {
            let to_upload: Vec<serde_json::Value> = local
                .iter()
                .filter(|h| !remote.iter().any(|r| highlight_key(r) == highlight_key(h)))
                .map(|h| json!({ "chapter": h.chapter, "start": h.start, "end": h.end, "words": h.words, "made": h.made }))
                .collect();
            if !to_upload.is_empty() {
                api.send(Method::Post, &format!("/v1/books/{}/highlights", entry.id.0), Body::Json(json!({ "highlights": to_upload })))?;
            }
        }

        let mut merged = local;
        let before = merged.len();
        for r in remote {
            if !merged.iter().any(|h| highlight_key(h) == highlight_key(r)) {
                merged.push(r.clone());
            }
        }
        if merged.len() > before {
            merged.sort_by_key(|h| (h.chapter, h.start, h.made));
            out.push((entry.id.clone(), merged));
        }
    }
    Ok(())
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
    send_row(api, dir, entry, book.source.as_ref().map(|s| s.url.as_str()))
}

fn send_cover(api: &Api, dir: &Path, entry: &Entry) -> Result<(), Error> {
    let Some(name) = &entry.cover else { return Ok(()) };
    let bytes = fs::read(dir.join("covers").join(name)).map_err(|e| Error::Local(format!("its cover couldn't be read ({e})")))?;
    api.send(Method::Put, &format!("/v1/covers/{name}"), Body::Bytes(&bytes)).map(drop)
}

fn send_row(api: &Api, dir: &Path, entry: &Entry, source_url: Option<&str>) -> Result<(), Error> {
    let loaded_url;
    let source_url = match source_url {
        Some(url) => Some(url),
        None => {
            loaded_url = inkling_books::load(dir, &entry.id).ok().and_then(|b| b.source.map(|s| s.url));
            loaded_url.as_deref()
        }
    };
    let row = json!({
        "title": entry.title,
        "author": entry.author,
        "format": entry.format,
        "chapters": entry.chapters,
        "cover": entry.cover,
        "added": entry.added,
        "source_url": source_url,
        "inbox": entry.inbox,
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
    let meta = Prepared::of(&book);
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
        site: meta.site,
        words: meta.words,
        excerpt: meta.excerpt,
        picture: meta.picture,
        icon: meta.icon,
        inbox: row.inbox,
        newsletter: meta.newsletter,
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

    fn signed_api(fake: &Fake) -> Api<'_> {
        let begun = crate::account::begin(fake, crate::FAKE_SERVER, "inkling://callback", "test").unwrap();
        let token = begun.finish(fake, &crate::fake_way_back(&begun.url).unwrap()).unwrap().token(fake).unwrap();
        Api::new(fake, crate::FAKE_SERVER, token)
    }

    #[test]
    fn a_book_crosses_once_and_the_place_read_last_wins_both_ways() {
        let fake = Fake::memory();
        let api = signed_api(&fake);
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

    #[test]
    fn incoming_saves_feeds_newsletters_and_highlights_sync_across_devices() {
        let fake = Fake::memory();
        let api = signed_api(&fake);
        let (mac, phone) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(8);
        let article_html = format!(
            "<!doctype html><html><head><title>Notes on the tide</title><meta property=\"og:site_name\" content=\"The Shore\">\
             <meta name=\"author\" content=\"Ana Lima\"><meta name=\"description\" content=\"Flats at dawn.\"></head>\
             <body><article><h1>Notes on the tide</h1><p>{words}</p><p>{words}</p></article></body></html>"
        );
        let newsletter_html = format!(
            "<!doctype html><html><head><title>Sunday Dispatch</title><meta property=\"og:site_name\" content=\"Weekly Letters\">\
             <meta name=\"author\" content=\"Weekly Letters\"></head>\
             <body><article><h1>Sunday Dispatch</h1><p>{words}</p><p>{words}</p></article></body></html>"
        );

        fake.stage_incoming("save", "queue", "https://shore.example/tide", Some("Notes on the tide"), article_html.as_bytes());
        fake.stage_incoming("newsletter", "inbox", "https://tobygodat.com/newsletters/abc", Some("Sunday Dispatch"), newsletter_html.as_bytes());

        // Mac syncs first: parses both with inkling-books, settles incoming, and adds them locally.
        let mut mac_lib = Library::open(mac.path()).unwrap();
        let first = sync(&api, mac.path()).unwrap();
        assert_eq!(first.imported.len(), 2);
        for prepared in first.imported {
            mac_lib.add(&prepared).unwrap();
        }
        let saved = mac_lib.entries().iter().find(|e| e.title == "Notes on the tide").unwrap().clone();
        let letter = mac_lib.entries().iter().find(|e| e.title == "Sunday Dispatch").unwrap().clone();
        assert_eq!((saved.format, saved.inbox, saved.newsletter, saved.site.as_deref()), (Format::Article, false, false, Some("The Shore")));
        assert_eq!((letter.format, letter.inbox, letter.newsletter, letter.site.as_deref()), (Format::Article, true, true, Some("Weekly Letters")));

        // Mac highlights a passage, then syncs up the parsed articles and highlight.
        let h_mac = Highlight { chapter: 0, start: 2, end: 19, words: "Notes on the tide".into(), made: 1_700_000_000_000 };
        mac_lib.set_highlights(&saved.id, std::slice::from_ref(&h_mac)).unwrap();
        assert_eq!(sync(&api, mac.path()).unwrap(), Outcome::default());

        // Phone syncs: downloads both articles without re-parsing and receives the highlight.
        let mut phone_lib = Library::open(phone.path()).unwrap();
        let on_phone = sync(&api, phone.path()).unwrap();
        assert!(on_phone.imported.is_empty(), "incoming items were already settled");
        assert_eq!(on_phone.books.len(), 2);
        for entry in on_phone.books {
            phone_lib.record(entry).unwrap();
        }
        for (id, hs) in on_phone.highlights {
            phone_lib.set_highlights(&id, &hs).unwrap();
        }
        let phone_saved = phone_lib.entry(&saved.id).unwrap();
        assert_eq!((phone_saved.inbox, phone_saved.site.as_deref(), phone_saved.excerpt.as_deref()), (false, Some("The Shore"), Some("Flats at dawn.")));
        assert!(phone_saved.words > 100);
        assert!(phone_lib.entry(&letter.id).unwrap().inbox, "newsletter stays in Inbox on the second device");
        assert!(phone_lib.entry(&letter.id).unwrap().newsletter, "and is still a newsletter there");
        assert_eq!(phone_lib.highlights(&saved.id), vec![h_mac.clone()]);

        // Phone adds a second highlight; Mac receives the merged list without duplicating h_mac.
        let h_phone = Highlight { chapter: 0, start: 25, end: 45, words: "The tide came in".into(), made: 1_700_000_005_000 };
        phone_lib.set_highlights(&saved.id, &[h_mac.clone(), h_phone.clone()]).unwrap();
        assert_eq!(sync(&api, phone.path()).unwrap(), Outcome::default());
        let back = sync(&api, mac.path()).unwrap();
        assert_eq!(back.highlights, vec![(saved.id.clone(), vec![h_mac, h_phone])]);
    }

    #[test]
    fn network_failure_after_settling_an_incoming_item_never_loses_it() {
        use crate::transport::{Response, Transport};
        use std::sync::atomic::{AtomicBool, Ordering};

        struct DropSecondIncoming<'a> {
            inner: &'a Fake,
            second_id: String,
            dropped: AtomicBool,
        }

        impl Transport for DropSecondIncoming<'_> {
            fn send(&self, method: Method, url: &str, headers: &[(&str, &str)], body: Option<(&str, &[u8])>) -> Result<Response, Error> {
                if method == Method::Get && url.contains(&format!("/v1/incoming/{}/file", self.second_id)) && !self.dropped.swap(true, Ordering::Relaxed) {
                    return Err(Error::Offline);
                }
                self.inner.send(method, url, headers, body)
            }
        }

        let fake = Fake::memory();
        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(8);
        let html = |title: &str| format!("<!doctype html><html><head><title>{title}</title></head><body><article><h1>{title}</h1><p>{words}</p></article></body></html>");
        let _first_id = fake.stage_incoming("save", "queue", "https://shore.example/one", Some("First"), html("First").as_bytes());
        let second_id = fake.stage_incoming("save", "queue", "https://shore.example/two", Some("Second"), html("Second").as_bytes());

        let begun = crate::account::begin(&fake, crate::FAKE_SERVER, "inkling://callback", "test").unwrap();
        let token = begun.finish(&fake, &crate::fake_way_back(&begun.url).unwrap()).unwrap().token(&fake).unwrap();
        let flaky = DropSecondIncoming { inner: &fake, second_id, dropped: AtomicBool::new(false) };
        let api = Api::new(&flaky, crate::FAKE_SERVER, token);

        let dir = tempfile::tempdir().unwrap();
        let mut library = Library::open(dir.path()).unwrap();

        // First sync settles First, then goes offline on Second: First is STILL returned and recorded.
        let partial = sync(&api, dir.path()).unwrap();
        assert_eq!(partial.imported.len(), 1);
        assert_eq!(partial.imported[0].title, "First");
        assert!(!partial.problems.is_empty());
        library.add(&partial.imported[0]).unwrap();

        // Second sync recovers and imports Second as well; neither article is lost.
        let rest = sync(&api, dir.path()).unwrap();
        assert_eq!(rest.imported.len(), 1);
        assert_eq!(rest.imported[0].title, "Second");
        library.add(&rest.imported[0]).unwrap();
        assert_eq!(library.entries().len(), 2);
    }
}

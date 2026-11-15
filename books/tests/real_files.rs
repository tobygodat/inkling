//! Real books, fetched by `books/fetch-fixtures.sh` into `target/fixtures`
//! (or `$INKLING_FIXTURES`).

use std::path::PathBuf;

use inkling_books::{Format, Library, prepare};

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::var_os("INKLING_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/fixtures"));
    let path = dir.join(name);
    assert!(path.exists(), "{} is missing: run books/fetch-fixtures.sh", path.display());
    path
}

/// A chapter's blocks: its Markdown between blank lines.
fn blocks(text: &str) -> Vec<&str> {
    text.split("\n\n").map(str::trim).collect()
}

#[test]
fn pride_and_prejudice_epub() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    let prepared = prepare(lib.dir(), &fixture("pride-and-prejudice.epub")).unwrap();
    let entry = lib.add(&prepared).unwrap().clone();
    assert_eq!(entry.title, "Pride and Prejudice");
    assert_eq!(entry.author.as_deref(), Some("Jane Austen"));
    assert_eq!(entry.format, Format::Epub);

    let book = lib.load(&entry.id).unwrap();
    let titles: Vec<&str> = book.chapters.iter().map(|c| c.title.as_deref().unwrap_or("")).collect();
    let numbered = titles.iter().filter(|t| t.to_ascii_lowercase().starts_with("chapter")).count();
    assert_eq!(numbered, 61, "{titles:#?}");
    assert_eq!(book.chapters.len(), 62, "{titles:#?}");

    let one = book.chapters.iter().find(|c| c.title.as_deref() == Some("Chapter I.")).expect("a Chapter I.");
    let text = blocks(&one.text);
    assert_eq!(text[0], "# Chapter I");
    assert!(
        text.contains(&"IT is a truth universally acknowledged, that a single man in possession of a good fortune must be in want of a wife."),
        "{text:#?}"
    );
    let two = book.chapters.iter().find(|c| c.title.as_deref() == Some("CHAPTER II.")).expect("a CHAPTER II.");
    assert!(blocks(&two.text).iter().any(|b| b.starts_with("MR. BENNET was among the earliest")), "drop cap rejoined: {}", &two.text[..400]);

    let all: String = book.chapters.iter().map(|c| c.text.as_str()).collect();
    assert!(!all.contains("Project Gutenberg"), "licence boilerplate is left out");
    assert!(all.contains("*"), "italics survive");
    let image = all.lines().find(|l| l.contains("they entered the breakfast room")).expect("an illustration with its caption");
    assert!(image.starts_with(&format!("![](images/{}/", entry.id.0)) && image.ends_with(".jpg \"“they entered the breakfast room”\")"), "{image}");
    let name = image.split(['(', ' ']).nth(1).unwrap();
    assert!(lib.dir().join(name).is_file(), "the image is stored where the chapter says");
    assert!(lib.dir().join("originals").join(format!("{}.epub", entry.id.0)).is_file(), "the original is kept");

    let again = prepare(lib.dir(), &fixture("pride-and-prejudice.epub")).unwrap();
    lib.add(&again).unwrap();
    assert_eq!(lib.entries().len(), 1);
}

#[test]
fn rfc_2616_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    let prepared = prepare(lib.dir(), &fixture("rfc2616.pdf")).unwrap();
    let entry = lib.add(&prepared).unwrap().clone();
    assert_eq!(entry.format, Format::Pdf);
    let book = lib.load(&entry.id).unwrap();
    assert_eq!(book.chapters.len(), 12, "114 pages in sections of ten");
    assert_eq!(book.chapters[0].title.as_deref(), Some("Pages 1–10"));

    let all: Vec<&str> = book.chapters.iter().flat_map(|c| blocks(&c.text)).collect();
    assert!(all.iter().all(|t| !t.contains("Standards Track [Page")), "page footers are dropped");
    assert!(all.iter().all(|t| !t.starts_with("RFC 2616 HTTP/1.1 June 1999")), "page headers are dropped");
    let abstract_ = all.iter().find(|t| t.starts_with("The Hypertext Transfer Protocol (HTTP) is an application-level")).expect("the abstract");
    assert!(abstract_.ends_with("the typing and negotiation of data representation, allowing systems to be built independently of the data being transferred."), "{abstract_}");
}

#[test]
fn pride_and_prejudice_keeps_its_own_cover() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture("pride-and-prejudice.epub");
    let prepared = inkling_books::prepare(dir.path(), &path).unwrap();
    let name = prepared.cover.as_deref().expect("this edition names a cover");
    let stored = std::fs::read(dir.path().join("covers").join(name)).unwrap();

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let entry = (0..archive.len()).map(|i| archive.name_for_index(i).unwrap().unwrap().to_string()).find(|n| n.ends_with("_cover.jpg")).unwrap();
    let mut original = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name(&entry).unwrap(), &mut original).unwrap();
    assert_eq!(stored, original, "the stored cover is the one the book names");
    assert!(name.ends_with(".jpg"));
}

//! Turning a file into a [`Book`]: pick the format, hash the bytes, parse.

use std::fmt;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::book::{Book, BookId, Format, PARSER};
use crate::{epub, pdf};

/// Why a file couldn't become a book, in words the reader is shown.
#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    Unsupported(String),
    Unreadable(String),
    BadEpub(String),
    BadPdf(String),
    /// A web page with no article in it that could be read.
    BadArticle(String),
    /// A PDF whose pages are pictures of text, with no text layer to reflow.
    Scanned,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Unsupported(name) => write!(f, "“{name}” isn’t an EPUB or a PDF."),
            ImportError::Unreadable(why) => write!(f, "The file couldn’t be read: {why}."),
            ImportError::BadEpub(why) => write!(f, "This EPUB couldn’t be opened: {why}."),
            ImportError::BadPdf(why) => write!(f, "This PDF couldn’t be opened: {why}."),
            ImportError::BadArticle(why) => write!(f, "This page couldn’t be saved as an article: {why}."),
            ImportError::Scanned => write!(f, "This PDF has no text to reflow. It looks scanned, and Inkling only opens PDFs with selectable text."),
        }
    }
}

impl std::error::Error for ImportError {}

pub fn book_id(bytes: &[u8]) -> BookId {
    let digest = Sha256::digest(bytes);
    BookId(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// The format a file is, from its first bytes, falling back to its extension.
pub fn sniff(path: &Path, bytes: &[u8]) -> Result<Format, ImportError> {
    if bytes.starts_with(b"%PDF") {
        return Ok(Format::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(Format::Epub);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("pdf") => Ok(Format::Pdf),
        Some("epub") => Ok(Format::Epub),
        _ => Err(ImportError::Unsupported(name)),
    }
}

/// A parsed book and the images its chapters show, each by its file name in `images/<id>/`.
#[derive(Debug)]
pub struct Parsed {
    pub book: Book,
    pub images: Vec<(String, Vec<u8>)>,
    /// Lead image path relative to the library (e.g. `og:image`) when the body has no picture.
    pub lead: Option<String>,
    /// Site icon path relative to the library when an article has no lead image.
    pub icon: Option<String>,
}

pub fn parse(path: &Path, bytes: &[u8]) -> Result<Parsed, ImportError> {
    let format = sniff(path, bytes)?;
    let id = book_id(bytes);
    let (title, author, chapters, images) = match format {
        Format::Epub => {
            let p = epub::parse(bytes, &format!("{}/", image_dir(&id))).map_err(ImportError::BadEpub)?;
            (p.title, p.author, p.chapters, p.images)
        }
        Format::Pdf => {
            let p = pdf::parse(bytes).map_err(|e| match e {
                pdf::PdfError::NoText => ImportError::Scanned,
                pdf::PdfError::Unreadable(why) => ImportError::BadPdf(why),
            })?;
            (p.title, p.author, p.chapters, vec![])
        }
        Format::Article => return Err(ImportError::Unsupported(title_from_file(path))),
    };
    let title = title.unwrap_or_else(|| title_from_file(path));
    Ok(Parsed { book: Book { id, title, author, format, parser: PARSER, chapters, source: None }, images, lead: None, icon: None })
}

/// Where a book's images are, relative to the library: what its chapters' image links start with.
pub fn image_dir(id: &BookId) -> String {
    format!("images/{}", id.0)
}

fn title_from_file(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    crate::epub::collapse(&stem.replace(['_', '-'], " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes_same_id() {
        assert_eq!(book_id(b"abc"), book_id(b"abc"));
        assert_ne!(book_id(b"abc"), book_id(b"abd"));
        assert_eq!(book_id(b"abc").0.len(), 64);
    }

    #[test]
    fn unknown_files_are_refused_in_plain_words() {
        let err = parse(Path::new("/x/notes.txt"), b"hello").unwrap_err();
        assert_eq!(err.to_string(), "“notes.txt” isn’t an EPUB or a PDF.");
    }

    #[test]
    fn a_title_falls_back_to_the_file_name() {
        let pdf = crate::pdf::tests::build(&[vec!["Just one page of plain text, long enough to count as text for the check."]]);
        let mut doc = pdf_extract::Document::load_mem(&pdf).unwrap();
        doc.trailer.remove(b"Info");
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let book = parse(Path::new("/x/my_field-notes.pdf"), &bytes).unwrap().book;
        assert_eq!(book.title, "my field notes");
        assert_eq!(book.format, Format::Pdf);
    }
}

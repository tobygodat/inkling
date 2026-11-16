//! Everything Inkling knows about books without a screen: an EPUB or text PDF parsed into one
//! [`Book`], the library that keeps books on disk, and the folder search that finds new ones.
//! No UI framework here, so it tests on its own.

pub mod book;
pub(crate) mod epub;
pub mod highlights;
pub mod import;
pub mod library;
mod markdown;
mod pdf;
pub mod scan;

pub use book::{Book, BookId, Chapter, Format, PARSER, Position, display_title};
pub use highlights::Highlight;
pub use import::ImportError;
pub use library::{Entry, Library, Prepared, load, prepare};

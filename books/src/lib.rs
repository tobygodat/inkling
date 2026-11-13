//! Everything Inkling knows about books without a screen: an EPUB or text PDF parsed into one
//! [`Book`], the library that keeps books on disk, and the folder search that finds new ones.
//! No UI framework here, so it tests on its own.

pub mod book;
pub(crate) mod epub;
pub mod import;
pub mod library;
mod pdf;
pub mod scan;

pub use book::{Block, Book, BookId, Chapter, Format, Position};
pub use import::ImportError;
pub use library::{Entry, Library, Prepared, load, prepare};

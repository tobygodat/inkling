use inkling_books::{Book, BookId, Entry, Library};
use inkling_core::store::Paths;
use std::io;
use std::path::Path;

/// What the tools read. On the Mac it is the app's data folder; on the server it will be the
/// database the devices sync to.
pub trait Store {
    /// Newest first, with each book's reading position.
    fn books(&self) -> io::Result<Vec<Entry>>;
    fn book(&self, id: &BookId) -> io::Result<Book>;
    fn draft(&self) -> io::Result<String>;
}

/// The data folder the app writes. It is read afresh on every call, so a book added or a page
/// turned in the app shows at once, and it is never written.
pub struct Folder {
    paths: Paths,
}

impl Folder {
    pub fn new(dir: &Path) -> Self {
        Folder { paths: Paths { dir: dir.to_path_buf() } }
    }

    /// The Mac app's folder: `INKLING_DATA_DIR` when set, else Application Support.
    pub fn mac() -> Self {
        Folder { paths: Paths::mac() }
    }

    pub fn dir(&self) -> &Path {
        &self.paths.dir
    }

    fn library(&self) -> io::Result<Library> {
        // Where `ui/src/app.rs` opens it.
        let dir = self.paths.dir.join("library");
        if !dir.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Inkling has no library here yet."));
        }
        Library::open(dir)
    }
}

impl Store for Folder {
    fn books(&self) -> io::Result<Vec<Entry>> {
        match self.library() {
            Ok(library) => Ok(library.entries().to_vec()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn book(&self, id: &BookId) -> io::Result<Book> {
        let library = self.library()?;
        if library.entry(id).is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("No book in the library has the id {}.", id.0)));
        }
        library.load(id)
    }

    fn draft(&self) -> io::Result<String> {
        Ok(self.paths.load_draft())
    }
}

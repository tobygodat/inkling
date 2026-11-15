use inkling_books::{Book, BookId, Entry, Library};
use inkling_core::store::Paths;
use inkling_core::{Document, Documents, Settings};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// What the tools read. On the Mac it is the app's data folder; on the server it will be the
/// database the devices sync to.
pub trait Store {
    /// Newest first, with each book's reading position.
    fn books(&self) -> io::Result<Vec<Entry>>;
    fn book(&self, id: &BookId) -> io::Result<Book>;
    /// The writer's documents, the one changed last first.
    fn documents(&self) -> io::Result<Vec<Document>>;
    fn document(&self, id: &str) -> io::Result<String>;
    /// The document that was open last.
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

    /// A book's parsed file as the library keeps it, `books/<id>.json`.
    pub fn book_file(&self, id: &BookId) -> io::Result<Vec<u8>> {
        fs::read(self.library_dir().join("books").join(format!("{}.json", id.0)))
    }

    /// A cover image the library keeps, by its name in `covers/`.
    pub fn cover_file(&self, name: &str) -> io::Result<Vec<u8>> {
        fs::read(self.library_dir().join("covers").join(name))
    }

    /// The file of the document that was open last. Until the app has moved `draft.md` into
    /// `documents/`, that draft.
    fn draft_file(&self) -> PathBuf {
        let documents = Documents::new(&self.paths);
        match documents.last_open(Settings::load(&self.paths).document.as_deref()) {
            Some(id) => documents.path(&id),
            None => self.paths.dir.join("draft.md"),
        }
    }

    /// When the draft last changed, in milliseconds since the Unix epoch, or 0 if it never did.
    /// Opening an older document makes it the draft without touching its file, so the time the
    /// open document was chosen, kept in `settings.json`, counts too.
    pub fn draft_saved_at(&self) -> u64 {
        let modified = |path: PathBuf| {
            fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64)
        };
        modified(self.draft_file()).max(modified(self.paths.settings()))
    }

    fn library_dir(&self) -> PathBuf {
        // Where `ui/src/app.rs` opens it.
        self.paths.dir.join("library")
    }

    fn library(&self) -> io::Result<Library> {
        let dir = self.library_dir();
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

    fn documents(&self) -> io::Result<Vec<Document>> {
        Ok(Documents::new(&self.paths).list())
    }

    fn document(&self, id: &str) -> io::Result<String> {
        let documents = Documents::new(&self.paths);
        // Only an id from the list, so no path outside the folder is ever read.
        if !documents.list().iter().any(|d| d.id == id) {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("No document has the id {id}.")));
        }
        documents.read(id)
    }

    fn draft(&self) -> io::Result<String> {
        match fs::read_to_string(self.draft_file()) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            read => read,
        }
    }
}

//! The writer's documents as Swift sees them: `<data>/documents/*.md`, the same folder the GPUI
//! app writes and sync reads.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use inkling_core::{Documents, Paths, Settings};

use crate::InklingError;

/// One document as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocInfo {
    /// The file's stem, which never changes.
    pub id: String,
    /// The first line with text, without its `#` marks.
    pub title: String,
    /// When the file last changed, in milliseconds since the Unix epoch.
    pub changed_ms: u64,
}

/// A document's title: its first line with text, without the `#`s of a heading.
#[uniffi::export]
pub fn document_title(text: String) -> String {
    inkling_core::documents::title(&text).to_string()
}

/// The documents in a data folder: `<data>/documents/`, plus the last open id in `settings.json`.
#[derive(uniffi::Object)]
pub struct DocumentStore {
    paths: Paths,
    docs: Documents,
    lock: Mutex<()>,
}

#[uniffi::export]
impl DocumentStore {
    /// Opens `<data>/documents/`, migrating any legacy `draft.md` into it once.
    #[uniffi::constructor]
    pub fn open(data: String) -> Result<Arc<DocumentStore>, InklingError> {
        let paths = Paths { dir: data.into() };
        let docs = Documents::new(&paths);
        docs.migrate(&paths)?;
        Ok(Arc::new(DocumentStore { paths, docs, lock: Mutex::new(()) }))
    }

    /// The documents folder. Local image paths in a draft are relative to it.
    pub fn folder(&self) -> String {
        self.docs.dir().to_string_lossy().into_owned()
    }

    /// Every document, newest first.
    pub fn list(&self) -> Vec<DocInfo> {
        let _guard = self.guard();
        self.docs
            .list()
            .into_iter()
            .map(|d| DocInfo { id: d.id, title: d.title, changed_ms: millis(d.changed) })
            .collect()
    }

    /// Reads one document's Markdown source.
    pub fn read(&self, id: String) -> Result<String, InklingError> {
        let _guard = self.guard();
        match self.docs.read(&id) {
            Ok(text) => Ok(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(InklingError::NotFound { id }),
            Err(e) => Err(e.into()),
        }
    }

    /// Saves one document atomically.
    pub fn write(&self, id: String, text: String) -> Result<(), InklingError> {
        let _guard = self.guard();
        self.docs.write(&id, &text)?;
        Ok(())
    }

    /// Removes a document. One that is already gone is not an error.
    pub fn remove(&self, id: String) -> Result<(), InklingError> {
        let _guard = self.guard();
        self.docs.remove(&id)?;
        Ok(())
    }

    /// Restores a removed document with the time it last changed, so it returns to its place.
    pub fn restore(&self, id: String, text: String, changed_ms: u64) -> Result<(), InklingError> {
        let _guard = self.guard();
        let changed = UNIX_EPOCH + Duration::from_millis(changed_ms);
        self.docs.restore(&id, &text, changed)?;
        Ok(())
    }

    /// An id no document has yet.
    pub fn new_id(&self) -> String {
        let _guard = self.guard();
        self.docs.new_id()
    }

    /// The id of the document open last, or the newest document if that one is gone.
    pub fn last_open(&self) -> Option<String> {
        let _guard = self.guard();
        let settings = Settings::load(&self.paths);
        self.docs.last_open(settings.document.as_deref())
    }

    /// Records which document is open in `settings.json`, preserving every other setting.
    pub fn set_last_open(&self, id: Option<String>) -> Result<(), InklingError> {
        let _guard = self.guard();
        let mut settings = Settings::load(&self.paths);
        settings.document = id;
        settings.save(&self.paths)?;
        Ok(())
    }
}

impl DocumentStore {
    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn documents_open_edit_save_remove_and_restore_through_core() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().to_string_lossy().into_owned();
        // A legacy draft.md migrates on open, matching the GPUI app.
        fs::write(data.path().join("draft.md"), "# Migrated\n\nCarried over.").unwrap();

        let store = DocumentStore::open(path.clone()).unwrap();
        assert!(!data.path().join("draft.md").exists());
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id.as_str(), list[0].title.as_str()), ("draft", "Migrated"));
        assert_eq!(store.read("draft".into()).unwrap(), "# Migrated\n\nCarried over.");

        let second = store.new_id();
        store.write(second.clone(), "# Second draft\n\nBody text.".into()).unwrap();
        store.set_last_open(Some(second.clone())).unwrap();
        assert_eq!(store.last_open().as_deref(), Some(second.as_str()));

        let info = store.list().into_iter().find(|d| d.id == second).unwrap();
        store.remove(second.clone()).unwrap();
        assert!(matches!(store.read(second.clone()), Err(InklingError::NotFound { .. })));

        store.restore(second.clone(), "# Second draft\n\nBody text.".into(), info.changed_ms).unwrap();
        assert_eq!(store.read(second.clone()).unwrap(), "# Second draft\n\nBody text.");

        // Reopening sees the same documents and last-open id.
        let reopened = DocumentStore::open(path).unwrap();
        assert_eq!(reopened.last_open().as_deref(), Some(second.as_str()));
        assert_eq!(reopened.list().len(), 2);
        assert_eq!(document_title("## Quiet desk\n\nText".into()), "Quiet desk");
    }
}

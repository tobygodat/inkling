//! The writer's documents: one Markdown file each in `<data>/documents/`. The folder is the list
//! and there is no index. A file's stem is its document's id, which never changes; the title is
//! read from the text, so retitling never moves a file.

use crate::store::{Paths, write_atomic};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// The id `draft.md` takes when it moves into the folder.
pub const MIGRATED: &str = "draft";

/// One document as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The file's stem.
    pub id: String,
    pub title: String,
    /// When the file last changed.
    pub changed: SystemTime,
}

/// The folder of documents in one data folder.
#[derive(Clone, Debug)]
pub struct Documents {
    dir: PathBuf,
}

impl Documents {
    pub fn new(paths: &Paths) -> Self {
        Documents { dir: paths.dir.join("documents") }
    }

    /// The folder. A document's image paths are read relative to it.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.md"))
    }

    /// Every document, the one changed last first. A folder that isn't there is an empty list.
    pub fn list(&self) -> Vec<Document> {
        let Ok(read) = fs::read_dir(&self.dir) else { return vec![] };
        let mut list: Vec<Document> = read
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let id = path.file_stem()?.to_str()?;
                if path.extension()? != "md" || id.starts_with('.') {
                    return None;
                }
                let changed = entry.metadata().ok()?.modified().ok()?;
                let text = fs::read_to_string(&path).ok()?;
                Some(Document { id: id.to_string(), title: title(&text).to_string(), changed })
            })
            .collect();
        list.sort_by(|a, b| b.changed.cmp(&a.changed).then_with(|| b.id.cmp(&a.id)));
        list
    }

    pub fn read(&self, id: &str) -> io::Result<String> {
        fs::read_to_string(self.path(id))
    }

    pub fn write(&self, id: &str, text: &str) -> io::Result<()> {
        write_atomic(&self.path(id), text.as_bytes())
    }

    /// Removes a document. One that is already gone is not an error.
    pub fn remove(&self, id: &str) -> io::Result<()> {
        match fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Puts a removed document back as it was, with the time it last changed, so it takes its old
    /// place in the list.
    pub fn restore(&self, id: &str, text: &str, changed: SystemTime) -> io::Result<()> {
        self.write(id, text)?;
        fs::File::options().write(true).open(self.path(id))?.set_modified(changed)
    }

    /// An id no document has: the time in milliseconds, counted on until it is free.
    pub fn new_id(&self) -> String {
        let mut n = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        while self.path(&n.to_string()).exists() {
            n += 1;
        }
        n.to_string()
    }

    /// The document that was open last: `last` while it is still there, else the newest.
    pub fn last_open(&self, last: Option<&str>) -> Option<String> {
        if let Some(id) = last
            && self.path(id).is_file()
        {
            return Some(id.to_string());
        }
        self.list().into_iter().next().map(|d| d.id)
    }

    /// Moves `draft.md`, the one draft from before documents, into the folder. It becomes document
    /// `draft`, or a new id when a different `draft` is already there, and keeps the time it last
    /// changed. An empty draft is just removed. Running it again changes nothing, and no text is
    /// ever overwritten.
    pub fn migrate(&self, paths: &Paths) -> io::Result<()> {
        let old = paths.dir.join("draft.md");
        let text = match fs::read_to_string(&old) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if text.trim().is_empty() {
            return fs::remove_file(old);
        }
        let id = match self.read(MIGRATED) {
            Ok(there) if there == text => return fs::remove_file(old),
            Ok(_) => self.new_id(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => MIGRATED.to_string(),
            Err(e) => return Err(e),
        };
        fs::create_dir_all(&self.dir)?;
        fs::rename(old, self.path(&id))
    }
}

/// A document's title: its first line with text, without the `#`s of a heading.
pub fn title(text: &str) -> &str {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.trim_start_matches('#').trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration;

    fn temp(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("inkling-docs-{name}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Paths { dir }
    }

    fn texts(docs: &Documents) -> Vec<String> {
        let mut texts: Vec<String> = docs.list().iter().map(|d| docs.read(&d.id).unwrap()).collect();
        texts.sort();
        texts
    }

    #[test]
    fn the_title_is_the_first_line_with_text_without_its_hashes() {
        assert_eq!(title("# On keeping accounts\n\nEvery list I keep"), "On keeping accounts");
        assert_eq!(title("\n\n  Letter to the board  \nDear all"), "Letter to the board");
        assert_eq!(title("### Chapter four"), "Chapter four");
        assert_eq!(title("The ferry"), "The ferry");
        assert_eq!(title("#\n\nBody"), "");
        assert_eq!(title(" \n "), "");
    }

    #[test]
    fn the_list_is_newest_first_and_only_markdown() {
        let paths = temp("list");
        let docs = Documents::new(&paths);
        assert!(docs.list().is_empty());
        let base = SystemTime::now() - Duration::from_secs(3600);
        for (id, text, minutes) in [("a", "# Old", 1), ("b", "New", 30), ("c", "Middle", 10), ("d", "Also middle", 10)] {
            docs.restore(id, text, base + Duration::from_secs(60 * minutes)).unwrap();
        }
        fs::write(docs.dir.join("notes.txt"), "not a document").unwrap();
        fs::write(docs.dir.join("x.tmp"), "half written").unwrap();
        let list = docs.list();
        let order: Vec<(&str, &str)> = list.iter().map(|d| (d.id.as_str(), d.title.as_str())).collect();
        assert_eq!(order, [("b", "New"), ("d", "Also middle"), ("c", "Middle"), ("a", "Old")]);
        assert_eq!(docs.last_open(Some("c")).as_deref(), Some("c"));
        assert_eq!(docs.last_open(Some("gone")).as_deref(), Some("b"));
        docs.remove("b").unwrap();
        docs.remove("b").unwrap();
        assert_eq!(docs.last_open(None).as_deref(), Some("d"));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Whatever was in `draft.md` and in the folder, migrating keeps every text, leaves no
        /// `draft.md` behind, and a second run changes nothing.
        #[test]
        fn migrating_the_draft_loses_nothing_and_runs_once(
            old in proptest::option::of("[a-c\\n ]{0,6}"),
            there in proptest::option::of("[a-c\\n ]{0,6}"),
        ) {
            let paths = temp("migrate");
            let docs = Documents::new(&paths);
            if let Some(there) = &there {
                docs.write(MIGRATED, there).unwrap();
            }
            if let Some(old) = &old {
                fs::write(paths.dir.join("draft.md"), old).unwrap();
            }
            docs.migrate(&paths).unwrap();
            let once = texts(&docs);
            prop_assert!(!paths.dir.join("draft.md").exists());
            for text in old.iter().filter(|t| !t.trim().is_empty()).chain(there.iter()) {
                prop_assert!(once.contains(text), "{text:?} was lost");
            }
            let expected = there.iter().count() + usize::from(old.as_ref().is_some_and(|o| !o.trim().is_empty() && Some(o) != there.as_ref()));
            prop_assert_eq!(once.len(), expected);
            docs.migrate(&paths).unwrap();
            prop_assert_eq!(texts(&docs), once);
        }
    }
}

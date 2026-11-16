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

/// Where a block of Markdown goes into `text`, and the text to put there, set apart by a blank
/// line on each side: after the top-level block that holds the caret `at` (a whole paragraph,
/// list, quote or code block), at the caret when it sits between blocks, or at the end with no
/// caret. A code fence left open there is closed first. Each place is checked by parsing the
/// result, and where the block wouldn't read as a block of its own (an open maths or HTML block
/// would swallow it) it goes before the block that holds the caret instead, else at the start.
/// The caret goes after the inserted text.
pub fn place_block(text: &str, at: Option<usize>, block: &str) -> (usize, String) {
    use markdown::mdast::Node;
    let children = match markdown::to_mdast(text, &crate::draft::options(true)) {
        Ok(Node::Root(root)) => root.children,
        _ => vec![],
    };
    let spans: Vec<(std::ops::Range<usize>, bool)> =
        children.iter().filter_map(|c| c.position().map(|p| (p.start.offset..p.end.offset, matches!(c, Node::Code(_))))).collect();
    let after = match at {
        Some(at) if at <= text.len() && text.is_char_boundary(at) => spans.iter().find(|(r, _)| r.start <= at && at <= r.end).map_or(at, |(r, _)| r.end),
        _ => text.len(),
    };
    let holder = spans.iter().find(|(r, _)| r.start <= after && after <= r.end);
    let close = holder
        .filter(|(r, code)| *code && r.end == after)
        .and_then(|(r, _)| open_fence(&text[r.clone()]))
        .map_or(String::new(), |fence| format!("{}{fence}", if text[..after].ends_with('\n') { "" } else { "\n" }));
    let tries = [(after, close), (holder.map_or(after, |(r, _)| r.start), String::new()), (0, String::new())];
    for (at, close) in &tries {
        let insert = framed(text, *at, close, block);
        if reads_as_its_own(&format!("{}{insert}{}", &text[..*at], &text[*at..]), block) {
            return (*at, insert);
        }
    }
    (0, framed(text, 0, "", block))
}

/// `close`, then `block` set apart by a blank line from the text on each side of `at`.
fn framed(text: &str, at: usize, close: &str, block: &str) -> String {
    let (before, after) = text.split_at(at);
    let closed = format!("{before}{close}");
    let lead = match closed.len() - closed.trim_end_matches('\n').len() {
        _ if closed.trim().is_empty() => "",
        0 => "\n\n",
        1 => "\n",
        _ => "",
    };
    let trail = &"\n\n"[(after.len() - after.trim_start_matches('\n').len()).min(2)..];
    format!("{close}{lead}{block}{trail}")
}

/// Whether `text` holds `block` as a top-level block of its own.
fn reads_as_its_own(text: &str, block: &str) -> bool {
    let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(text, &crate::draft::options(true)) else { return false };
    root.children.iter().filter_map(|c| c.position()).any(|p| text[p.start.offset..p.end.offset].trim_end() == block.trim_end())
}

/// The fence that would close a fenced code block whose fence was never closed: its own character,
/// as many times as it opened with.
fn open_fence(code: &str) -> Option<String> {
    let first = code.lines().next()?.trim_start();
    let c = first.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = first.chars().take_while(|x| *x == c).count();
    let closed = code.lines().skip(1).last().is_some_and(|l| {
        let l = l.trim();
        l.len() >= n && l.chars().all(|x| x == c)
    });
    (n >= 3 && !closed).then(|| c.to_string().repeat(n))
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

    fn put(text: &str, at: Option<usize>) -> String {
        let (i, block) = place_block(text, at, "> Q");
        format!("{}{block}|{}", &text[..i], &text[i..])
    }

    #[test]
    fn a_block_goes_after_the_caret_s_block_set_apart_by_blank_lines() {
        assert_eq!(put("One.\n\nTwo.", None), "One.\n\nTwo.\n\n> Q\n\n|");
        assert_eq!(put("One.\n\nTwo.", Some(1)), "One.\n\n> Q|\n\nTwo.");
        assert_eq!(put("One.\n\nTwo.\n", Some(5)), "One.\n\n> Q\n|\nTwo.\n");
        assert_eq!(put("", Some(0)), "> Q\n\n|");
        assert_eq!(put("One.\n", None), "One.\n\n> Q\n\n|");
        assert_eq!(
            put("First line\ncontinues the same paragraph.\n\nNext paragraph.", Some(3)),
            "First line\ncontinues the same paragraph.\n\n> Q|\n\nNext paragraph.",
            "a soft line break is inside the paragraph"
        );
        assert_eq!(put("- one\n- two\n\nAfter.", Some(2)), "- one\n- two\n\n> Q\n|\nAfter.", "after the whole list");
        assert_eq!(put("> a\n> b\n\nAfter.", Some(2)), "> a\n> b\n\n> Q|\n\nAfter.", "after the whole quote");
        assert_eq!(put("# Title\nBody.", Some(2)), "# Title\n\n> Q\n|\nBody.", "a heading is a block of its own");
        assert_eq!(put("```\na\n\nb\n```\n\nAfter.", Some(4)), "```\na\n\nb\n```\n\n> Q|\n\nAfter.", "a blank line inside code is still the code");
        assert_eq!(put("```rust\nlet x = 1;", Some(12)), "```rust\nlet x = 1;\n```\n\n> Q\n\n|", "a fence left open is closed first");
        assert_eq!(put("~~~~\nopen\n", None), "~~~~\nopen\n~~~~\n\n> Q\n\n|", "with its own fence");
        for text in ["```rust\nlet x = 1;\n    ```", "~~~\nx\n    ~~~", "$$\nx^2"] {
            for at in [Some(4), None] {
                assert!(quotes(&put(text, at).replace('|', "")).iter().any(|q| q == "> Q"), "the quote is swallowed in {text:?} at {at:?}");
            }
        }
    }

    proptest! {
        /// Wherever the caret is, the block lands between the document's blocks, never inside
        /// one, and reads as one block of its own.
        #[test]
        fn a_block_never_lands_inside_another(
            blocks in proptest::collection::vec(proptest::sample::select(vec![
                "Plain words.", "Soft\nbreak.", "- a\n- b", "1. one\n2. two", "> q\n> r", "# Head", "```\nx\n\ny\n```",
                "```rust\nopen", "~~~\nopen\n\nstill code", "```rust\nlet x = 1;\n    ```", "~~~\nx\n    ~~~",
                "$$\nx^2", "$$\ny\n$$", "<pre>\nraw", "<div>\nblock", "<!-- open",
            ]), 1..5),
            caret in proptest::option::of(0usize..80),
        ) {
            let text = blocks.join("\n\n");
            let caret = caret.map(|c| c.min(text.len()));
            let (at, block) = place_block(&text, caret, "> Q");
            let spans = top_level(&text);
            prop_assert!(spans.iter().all(|r| !(r.start < at && at < r.end)), "{at} splits a block of {text:?}");
            let placed = format!("{}{block}{}", &text[..at], &text[at..]);
            prop_assert!(quotes(&placed).iter().any(|q| q == "> Q"), "the block is no quote of its own in {:?}", placed);
            // Every block that was there still reads as it did; a fence closed for the quote
            // only gains its closing line.
            let was = sources(&text);
            let mut now = sources(&placed);
            if let Some(i) = now.iter().position(|q| q == "> Q") {
                now.remove(i);
            }
            prop_assert_eq!(was.len(), now.len(), "{:?} became {:?}", text, placed);
            prop_assert!(was.iter().zip(&now).all(|(a, b)| b.starts_with(a.as_str())), "{:?} became {:?}", text, placed);
        }
    }

    /// The source of each top-level block.
    fn sources(text: &str) -> Vec<String> {
        top_level(text).into_iter().map(|r| text[r].trim_end().to_string()).collect()
    }

    /// The source of each top-level block quote.
    fn quotes(text: &str) -> Vec<String> {
        let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(text, &crate::draft::options(true)) else { return vec![] };
        root.children
            .iter()
            .filter(|c| matches!(c, markdown::mdast::Node::Blockquote(_)))
            .filter_map(|c| c.position().map(|p| text[p.start.offset..p.end.offset].trim_end().to_string()))
            .collect()
    }

    fn top_level(text: &str) -> Vec<std::ops::Range<usize>> {
        let Ok(markdown::mdast::Node::Root(root)) = markdown::to_mdast(text, &crate::draft::options(true)) else { return vec![] };
        root.children.iter().filter_map(|c| c.position().map(|p| p.start.offset..p.end.offset)).collect()
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

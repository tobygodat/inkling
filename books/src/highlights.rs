//! The passages a reader highlighted, kept per book in `highlights/<id>.json`, apart from the
//! book's text: a new parse of the book leaves them alone, and sync can send the file as it is.
//! A highlight is found again by its words when the chapter's text has moved under it.

use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::book::BookId;
use crate::library::write_atomic;
use crate::markdown::{Inline, Style, inline};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Highlight {
    /// Counted from 0.
    pub chapter: usize,
    /// Where the passage starts and ends in the chapter's Markdown, in bytes.
    pub start: usize,
    pub end: usize,
    /// The passage as the reader saw it, its paragraphs on lines of their own.
    pub words: String,
    /// When it was made, in milliseconds since the Unix epoch.
    pub made: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    highlights: Vec<Highlight>,
}

fn path(dir: &Path, id: &BookId) -> PathBuf {
    dir.join("highlights").join(format!("{}.json", id.0))
}

/// A book's highlights in reading order. A missing or damaged file reads as none.
pub fn load(dir: &Path, id: &BookId) -> Vec<Highlight> {
    let mut all = fs::read(path(dir, id)).ok().and_then(|b| serde_json::from_slice::<File>(&b).ok()).map(|f| f.highlights).unwrap_or_default();
    all.sort_by_key(|h| (h.chapter, h.start, h.made));
    all
}

pub fn save(dir: &Path, id: &BookId, highlights: &[Highlight]) -> io::Result<()> {
    fs::create_dir_all(dir.join("highlights"))?;
    let file = File { highlights: highlights.to_vec() };
    write_atomic(&path(dir, id), &serde_json::to_vec_pretty(&file).map_err(io::Error::other)?)
}

impl Highlight {
    /// Where the passage is in a chapter as the reader sees it. `text` is the chapter's text as
    /// drawn and `source[i]` the Markdown byte that draws byte `i` of it, with one more entry for
    /// the end, never decreasing. The stored place while its words are still there; else the place of its words
    /// nearest to it, after a new parse moved them; else `None`, and it isn't drawn.
    pub fn locate(&self, text: &str, source: &[usize]) -> Option<Range<usize>> {
        if self.words.is_empty() || source.len() != text.len() + 1 {
            return None;
        }
        let at = |byte: usize| source.partition_point(|s| *s < byte).min(text.len());
        let stored = at(self.start)..at(self.end);
        if text.get(stored.clone()) == Some(self.words.as_str()) {
            return Some(stored);
        }
        text.match_indices(&self.words).map(|(i, _)| i..i + self.words.len()).min_by_key(|r| source[r.start].abs_diff(self.start))
    }
}

/// A run of a quoted paragraph and whether it is set in italics and bold.
pub type Run = (String, bool, bool);

/// A passage as a Markdown block quote for the writer's document, keeping its italics and bold,
/// with "— Author, *Title*" under it.
pub fn quote(paragraphs: &[Vec<Run>], author: Option<&str>, title: &str) -> String {
    let line = |runs: &[Run]| inline(&runs.iter().map(|(t, em, strong)| Inline::Text(t.clone(), Style { em: *em, strong: *strong })).collect::<Vec<_>>());
    let mut by = vec![Inline::text("— ")];
    if let Some(author) = author.filter(|a| !a.trim().is_empty()) {
        by.push(Inline::text(format!("{author}, ")));
    }
    by.push(Inline::Text(title.to_string(), Style { em: true, strong: false }));
    let mut blocks: Vec<String> = paragraphs.iter().map(|p| line(p)).filter(|p| !p.is_empty()).collect();
    blocks.push(inline(&by));
    blocks.iter().map(|b| b.lines().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")).collect::<Vec<_>>().join("\n>\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(start: usize, words: &str) -> Highlight {
        Highlight { chapter: 0, start, end: start + words.len(), words: words.into(), made: 1 }
    }

    /// Drawn text that is its own Markdown, as plain prose is, starting `from` bytes in.
    fn plain(text: &str, from: usize) -> Vec<usize> {
        (0..=text.len()).map(|i| from + i).collect()
    }

    proptest! {
        /// A highlight is drawn where it was made, and after a new parse put other text before
        /// its words, where they went; once its words are gone it isn't drawn.
        #[test]
        fn a_highlight_follows_its_words(before in "[a-z ]{0,40}", words in "[A-Z][a-z]{2,8}( [a-z]{1,8}){0,3}", after in "[a-z ]{0,40}", moved in "[a-z .]{0,60}") {
            let text = format!("{before}{words}{after}");
            let h = at(5 + before.len(), &words);
            prop_assert_eq!(h.locate(&text, &plain(&text, 5)), Some(before.len()..before.len() + words.len()));

            let reparsed = format!("{moved}{before}{words}{after}");
            let found = h.locate(&reparsed, &plain(&reparsed, 0)).expect("the words are still there");
            prop_assert_eq!(&reparsed[found], words.as_str());

            let gone = format!("{before}{after}");
            prop_assert_eq!(h.locate(&gone, &plain(&gone, 0)), None);
        }
    }

    #[test]
    fn of_two_copies_of_its_words_it_takes_the_one_nearest_where_it_was() {
        let text = "Simplicity. Then much later, simplicity again: Simplicity.";
        let h = at(48, "Simplicity");
        assert_eq!(h.locate(text, &plain(text, 3)), Some(47..57));
    }

    #[test]
    fn highlights_are_kept_per_book_in_reading_order() {
        let dir = tempfile::tempdir().unwrap();
        let id = BookId("a".repeat(64));
        assert!(load(dir.path(), &id).is_empty());
        let later = Highlight { chapter: 2, ..at(0, "Later") };
        save(dir.path(), &id, &[later.clone(), at(9, "Earlier")]).unwrap();
        assert_eq!(load(dir.path(), &id), vec![at(9, "Earlier"), later]);
        assert!(load(dir.path(), &BookId("b".repeat(64))).is_empty(), "another book has none");
    }

    #[test]
    fn a_quote_keeps_its_italics_and_names_its_book() {
        let passage = vec![
            vec![("I went to the woods because I wished to live ".into(), false, false), ("deliberately".into(), true, false), (", for $5 *a day*.".into(), false, false)],
            vec![("Simplicity, simplicity, simplicity!".into(), false, false)],
        ];
        assert_eq!(
            quote(&passage, Some("Henry David Thoreau"), "Walden"),
            "> I went to the woods because I wished to live *deliberately*, for \\$5 \\*a day\\*.\n>\n> Simplicity, simplicity, simplicity!\n>\n> — Henry David Thoreau, *Walden*"
        );
        assert_eq!(quote(&passage[1..], None, "Walden"), "> Simplicity, simplicity, simplicity!\n>\n> — *Walden*");
    }
}

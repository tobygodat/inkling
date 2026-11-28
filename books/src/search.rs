//! The paragraphs of a book that bear on a question, for the Ask bar to quote and cite (TOB-559).

use crate::Book;

/// A paragraph of a chapter: where it sits in the chapter's Markdown, in bytes, and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub chapter: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A paragraph shorter than this is a heading, a caption or a line of verse, not a passage to cite.
const SHORTEST: usize = 80;

/// The paragraphs of `book` holding the most of `words`, best first, at most `limit`. A word counts
/// once per paragraph, so a paragraph that repeats one word doesn't beat one holding several, and
/// of two that hold as many the earlier comes first. Matching ignores case. A paragraph holding none
/// of the words is never returned.
pub fn passages(book: &Book, words: &[String], limit: usize) -> Vec<Found> {
    let words: Vec<String> = words.iter().map(|w| w.to_lowercase()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() || limit == 0 {
        return vec![];
    }
    let mut scored: Vec<(usize, Found)> = vec![];
    for (chapter, text) in book.chapters.iter().map(|c| &c.text).enumerate() {
        for (start, end) in paragraphs(text) {
            let paragraph = &text[start..end];
            if paragraph.len() < SHORTEST || paragraph.starts_with('#') || paragraph.starts_with("![") {
                continue;
            }
            let folded = paragraph.to_lowercase();
            let score = words.iter().filter(|w| folded.contains(w.as_str())).count();
            if score > 0 {
                scored.push((score, Found { chapter, start, end, text: paragraph.to_string() }));
            }
        }
    }
    // A stable sort keeps reading order among equals.
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().take(limit).map(|(_, found)| found).collect()
}

/// Each paragraph's byte range, blank lines between them, trimmed of the whitespace around it.
fn paragraphs(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut at = 0;
    text.split("\n\n").filter_map(move |block| {
        let start = at + (block.len() - block.trim_start().len());
        let end = at + block.trim_end().len();
        at += block.len() + 2;
        (end > start).then_some((start, end))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BookId, Chapter, Format};
    use proptest::prelude::*;

    fn book(chapters: &[&str]) -> Book {
        Book {
            id: BookId("b".into()),
            title: "Walden".into(),
            author: None,
            format: Format::Epub,
            parser: crate::PARSER,
            source: None,
            chapters: chapters.iter().map(|t| Chapter { title: None, text: t.to_string() }).collect(),
        }
    }

    proptest! {
        /// Whatever the chapters and the words, every passage is its own bytes of its chapter, holds
        /// one of the words, and the passages come best first, no more than asked for.
        #[test]
        fn passages_are_their_chapters_own_bytes_best_first(
            chapters in prop::collection::vec("([a-zA-Zé ,.]{0,120}\n\n?){0,8}", 1..4),
            words in prop::collection::vec("[a-z]{1,5}", 0..4),
            limit in 0usize..6,
        ) {
            let refs: Vec<&str> = chapters.iter().map(String::as_str).collect();
            let book = book(&refs);
            let found = passages(&book, &words, limit);
            prop_assert!(found.len() <= limit);
            let score = |f: &Found| words.iter().filter(|w| f.text.to_lowercase().contains(w.as_str())).count();
            for f in &found {
                prop_assert_eq!(&book.chapters[f.chapter].text[f.start..f.end], f.text.as_str());
                prop_assert!(score(f) > 0);
            }
            prop_assert!(found.windows(2).all(|pair| score(&pair[0]) >= score(&pair[1])));
        }
    }

    #[test]
    fn the_paragraph_holding_more_of_the_words_comes_first() {
        let pond = "I went to the woods because I wished to live deliberately, to front only the essential facts of life.";
        let both = "The pond in winter was a field of snow, and I lived beside it deliberately, as the woods allowed me to.";
        let book = book(&[&format!("# Where I Lived\n\n{pond}"), &format!("Short.\n\n{both}")]);
        let found = passages(&book, &["deliberately".into(), "Winter".into()], 5);
        assert_eq!(found.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), [both, pond]);
        assert_eq!((found[0].chapter, found[1].chapter), (1, 0));
        assert!(passages(&book, &["sea".into()], 5).is_empty(), "nothing that holds none of the words");
    }
}

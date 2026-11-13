//! The tools, each a function of a [`Store`] and its arguments. A tool that can't do what it was
//! asked answers with `isError` and a sentence the agent can act on, not a protocol error.

use crate::store::Store;
use inkling_books::{Block, Book, BookId, Chapter, Entry, Format};
use serde_json::{Value, json};
use std::fmt::Write;
use std::ops::Range;

/// As `search_library`'s `limit` in `tools.json` says.
const DEFAULT_HITS: usize = 20;
const MAX_HITS: usize = 100;
/// Characters of context on each side of a match.
const CONTEXT: usize = 120;

/// The tools' definitions. `mcp/tools.json` is the one copy: the Cloudflare Worker serves the same
/// file (ADR 0004), so an agent sees the same tools on the Mac and away from it.
pub fn list() -> Value {
    serde_json::from_str(include_str!("../tools.json")).expect("mcp/tools.json is valid JSON")
}

/// `None` when there is no tool by that name.
pub fn call(store: &impl Store, name: &str, arguments: &Value) -> Option<Value> {
    let result = match name {
        "list_books" => list_books(store),
        "read_book" => read_book(store, arguments),
        "search_library" => search_library(store, arguments),
        "read_draft" => read_draft(store),
        _ => return None,
    };
    Some(match result {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
    })
}

fn list_books(store: &impl Store) -> Result<String, String> {
    let books = store.books().map_err(|e| format!("The library couldn't be read ({e})."))?;
    let books: Vec<Value> = books.iter().map(describe).collect();
    Ok(serde_json::to_string_pretty(&books).expect("JSON values serialise"))
}

fn describe(entry: &Entry) -> Value {
    json!({
        "id": entry.id.0,
        "title": entry.title,
        "author": entry.author,
        "format": match entry.format { Format::Epub => "epub", Format::Pdf => "pdf" },
        "chapters": entry.chapters,
        "position": { "chapter": entry.position.chapter, "fraction": entry.position.fraction },
        "progress": entry.progress(),
    })
}

fn read_book(store: &impl Store, arguments: &Value) -> Result<String, String> {
    let book = load(store, arguments)?;
    let Some(chapter) = arguments.get("chapter") else {
        let mut text = format!("{}{}\n\n", book.title, book.author.as_ref().map(|a| format!(" by {a}")).unwrap_or_default());
        for (i, chapter) in book.chapters.iter().enumerate() {
            let _ = writeln!(text, "{i}. {}", chapter_title(chapter, i));
        }
        return Ok(text);
    };
    let at = chapter.as_u64().ok_or("chapter must be a whole number from 0.")? as usize;
    let chapter = book.chapters.get(at).ok_or_else(|| {
        format!("{} has {} chapters, numbered 0 to {}.", book.title, book.chapters.len(), book.chapters.len().saturating_sub(1))
    })?;
    Ok(markdown(chapter, at))
}

fn load(store: &impl Store, arguments: &Value) -> Result<Book, String> {
    let id = arguments.get("book_id").and_then(Value::as_str).ok_or("book_id is required.")?;
    store.book(&BookId(id.to_string())).map_err(|e| format!("That book couldn't be read: {e}"))
}

pub(crate) fn chapter_title(chapter: &Chapter, at: usize) -> String {
    chapter.title.clone().unwrap_or_else(|| format!("Chapter {}", at + 1))
}

pub(crate) fn markdown(chapter: &Chapter, at: usize) -> String {
    let mut text = format!("# {}\n", chapter_title(chapter, at));
    for block in &chapter.blocks {
        text.push('\n');
        match block {
            Block::Heading { level, text: heading } => {
                let _ = writeln!(text, "{} {heading}", "#".repeat((*level).clamp(2, 6) as usize));
            }
            Block::Paragraph(paragraph) => {
                let _ = writeln!(text, "{paragraph}");
            }
            Block::Quote(quote) => {
                for line in quote.lines() {
                    let _ = writeln!(text, "> {line}");
                }
            }
            Block::Break => text.push_str("* * *\n"),
        }
    }
    text
}

fn read_draft(store: &impl Store) -> Result<String, String> {
    let draft = store.draft().map_err(|e| format!("The draft couldn't be read ({e})."))?;
    Ok(if draft.trim().is_empty() { "The draft is empty.".to_string() } else { draft })
}

fn search_library(store: &impl Store, arguments: &Value) -> Result<String, String> {
    let query = arguments.get("query").and_then(Value::as_str).unwrap_or_default();
    if query.trim().is_empty() {
        return Err("query must have some text in it.".to_string());
    }
    let limit = arguments.get("limit").and_then(Value::as_u64).map_or(DEFAULT_HITS, |l| (l as usize).clamp(1, MAX_HITS));
    let entries = store.books().map_err(|e| format!("The library couldn't be read ({e})."))?;
    let only = arguments.get("book_id").and_then(Value::as_str);
    if let Some(id) = only
        && !entries.iter().any(|e| e.id.0 == id)
    {
        return Err(format!("No book in the library has the id {id}."));
    }

    let mut hits = Vec::new();
    'books: for entry in entries.iter().filter(|e| only.is_none_or(|id| e.id.0 == id)) {
        let book = store.book(&entry.id).map_err(|e| format!("{} couldn't be read ({e}).", entry.title))?;
        for (at, chapter) in book.chapters.iter().enumerate() {
            for block in &chapter.blocks {
                let text = match block {
                    Block::Heading { text, .. } | Block::Paragraph(text) | Block::Quote(text) => text,
                    Block::Break => continue,
                };
                for range in find(text, query) {
                    hits.push(json!({
                        "book_id": book.id.0,
                        "title": book.title,
                        "chapter": at,
                        "chapter_title": chapter_title(chapter, at),
                        "passage": passage(text, range),
                    }));
                    if hits.len() == limit {
                        break 'books;
                    }
                }
            }
        }
    }
    if hits.is_empty() {
        return Ok(format!("Nothing in the library matches \"{query}\"."));
    }
    Ok(serde_json::to_string_pretty(&hits).expect("JSON values serialise"))
}

/// Where `needle` occurs in `haystack`, ignoring case, as byte ranges of `haystack` that start and
/// end on its character boundaries. Case is folded one character at a time, so a character whose
/// lower case is longer (İ) still maps back to itself.
pub(crate) fn find(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    let needle: String = needle.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // For each byte of the folded text, the character of `haystack` it came from.
    let mut folded = String::with_capacity(haystack.len());
    let mut source = Vec::with_capacity(haystack.len());
    for (at, c) in haystack.char_indices() {
        for lower in c.to_lowercase() {
            folded.push(lower);
            source.extend(std::iter::repeat_n((at, at + c.len_utf8()), lower.len_utf8()));
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut from = 0;
    while let Some(found) = folded[from..].find(&needle) {
        let start = from + found;
        let end = start + needle.len();
        let range = source[start].0..source[end - 1].1;
        if ranges.last().is_none_or(|last| last.end <= range.start) {
            ranges.push(range);
        }
        from = end;
    }
    ranges
}

/// The match with up to [`CONTEXT`] characters either side, cut at character boundaries.
fn passage(text: &str, range: Range<usize>) -> String {
    let start = text[..range.start].char_indices().rev().nth(CONTEXT - 1).map_or(0, |(i, _)| i);
    let end = text[range.end..].char_indices().nth(CONTEXT).map_or(text.len(), |(i, _)| range.end + i);
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        &text[start..end],
        if end < text.len() { "…" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn fold(s: &str) -> String {
        s.chars().flat_map(char::to_lowercase).collect()
    }

    #[test]
    fn the_search_limits_are_the_ones_tools_json_states() {
        let limit = list()[2]["inputSchema"]["properties"]["limit"].clone();
        assert_eq!((limit["default"].as_u64(), limit["maximum"].as_u64()), (Some(DEFAULT_HITS as u64), Some(MAX_HITS as u64)));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Any piece of a text is found in it, whatever its case, and every match is a slice of
        /// the original text that holds the query once folded.
        #[test]
        fn a_piece_of_the_text_is_found_in_any_case(
            text in "\\PC{1,60}",
            cut in any::<(prop::sample::Index, prop::sample::Index)>(),
            upper in any::<bool>(),
        ) {
            let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
            let (a, b) = (cut.0.index(bounds.len()), cut.1.index(bounds.len()));
            let piece = &text[bounds[a.min(b)]..bounds[a.max(b)]];
            prop_assume!(!piece.is_empty());
            let query = if upper { piece.to_uppercase() } else { piece.to_string() };
            prop_assume!(!fold(&query).is_empty());

            let ranges = find(&text, &query);
            if fold(piece) == fold(&query) {
                prop_assert!(!ranges.is_empty(), "{query:?} not found in {text:?}");
            }
            for range in ranges {
                prop_assert!(fold(&text[range.clone()]).contains(&fold(&query)));
                prop_assert!(passage(&text, range.clone()).contains(&text[range]));
            }
        }
    }
}

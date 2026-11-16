//! Find: every place a query occurs in a text, ignoring case.
//!
//! Each character is compared by its first lowercase character, so a match never changes length
//! between the text and what it is compared as, and every range is on char boundaries.

use std::ops::Range;

/// Every place `query` occurs in `text`, ignoring case, in order and not overlapping. An empty
/// query finds nothing.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    let query: Vec<char> = query.chars().map(fold).collect();
    if query.is_empty() {
        return vec![];
    }
    let chars: Vec<(usize, char)> = text.char_indices().map(|(at, c)| (at, fold(c))).collect();
    let mut found = vec![];
    let mut i = 0;
    while i + query.len() <= chars.len() {
        if chars[i..i + query.len()].iter().zip(&query).all(|((_, a), b)| a == b) {
            let end = chars.get(i + query.len()).map_or(text.len(), |(at, _)| *at);
            found.push(chars[i].0..end);
            i += query.len();
        } else {
            i += 1;
        }
    }
    found
}

/// Whether `query` occurs in `text`, ignoring case. An empty query is in every text.
pub fn contains(text: &str, query: &str) -> bool {
    query.is_empty() || !find_all(text, query).is_empty()
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Against a plain search over the lowercased text, on text whose lowercase keeps every
        /// byte where it was: the same matches, each on char boundaries.
        #[test]
        fn finds_what_a_lowercase_search_finds(text in "[a-cA-CéÉ ]{0,40}", query in "[a-cA-CéÉ]{1,3}") {
            let (lower, needle) = (text.to_lowercase(), query.to_lowercase());
            let mut expected = vec![];
            let mut from = 0;
            while let Some(at) = lower[from..].find(&needle) {
                expected.push(from + at..from + at + needle.len());
                from += at + needle.len();
            }
            prop_assert_eq!(find_all(&text, &query), expected);
        }
    }

    #[test]
    fn a_letter_whose_lowercase_is_longer_still_matches_in_place() {
        let text = "İstanbul and istanbul";
        let found = find_all(text, "istanbul");
        assert_eq!(found.iter().map(|r| &text[r.clone()]).collect::<Vec<_>>(), ["İstanbul", "istanbul"]);
        assert!(contains("Walden; or, Life in the Woods", "WOODS"));
        assert!(find_all("anything", "").is_empty());
    }
}

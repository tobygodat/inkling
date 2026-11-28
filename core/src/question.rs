//! A question asked in the Ask bar (TOB-559): what it is about, the passages of the library it
//! names, the prompt they make, and the passages the answer cites. An instruction to change the
//! draft is an [`crate::Ask`] instead, and goes through the inline review.

use crate::ai::Prompt;
use crate::ask::BadRequest;

/// What the question is asked about: the screen's document or article and the words on it, a
/// selection when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct About {
    /// The document or book and where in it, as the turn shows it: "Walden · Economy".
    pub label: String,
    pub text: String,
}

/// A passage the answer may cite, numbered from 1 in the order the prompt lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub title: String,
    pub author: Option<String>,
    pub chapter: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    asked: String,
    about: Option<About>,
    sources: Vec<Source>,
}

const SYSTEM: &str = "You answer questions inside a quiet app for reading and writing. Answer in a \
    few short paragraphs of plain prose, without headings or lists. When passages from the \
    reader's library are given, ground the answer in them and cite each one you use with its \
    number in square brackets, like [1], right after the sentence it supports. Never cite a number \
    you weren't given.";

/// Words too common to say what a question is about.
const COMMON: &[&str] = &[
    "about", "after", "again", "also", "because", "been", "before", "being", "could", "does", "doing", "each", "from",
    "have", "here", "into", "just", "like", "make", "more", "most", "much", "only", "other", "over", "said", "same",
    "says", "should", "some", "such", "than", "that", "their", "them", "then", "there", "these", "they", "thing",
    "think", "this", "those", "through", "very", "what", "when", "where", "which", "while", "with", "would", "your",
];

impl Question {
    pub fn new(asked: impl Into<String>, about: Option<About>, sources: Vec<Source>) -> Result<Self, BadRequest> {
        let asked = asked.into().trim().to_string();
        if asked.is_empty() {
            return Err(BadRequest::EmptyInstruction);
        }
        Ok(Question { asked, about, sources })
    }

    pub fn prompt(&self) -> Prompt {
        let mut user = String::new();
        if let Some(about) = &self.about {
            user.push_str(&format!("The reader is looking at {}:\n<on_screen>\n{}\n</on_screen>\n\n", about.label, about.text.trim()));
        }
        if !self.sources.is_empty() {
            user.push_str("Passages from the reader's library:\n");
            for (n, source) in self.sources.iter().enumerate() {
                let by = source.author.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
                user.push_str(&format!("<passage n=\"{}\" from=\"{}{by}, {}\">\n{}\n</passage>\n", n + 1, source.title, source.chapter, source.text.trim()));
            }
            user.push('\n');
        }
        user.push_str(&format!("Question: {}", self.asked));
        Prompt { system: SYSTEM.into(), user }
    }

    /// A canned answer, for headless runs: it quotes and cites the first two passages, or speaks
    /// of what is on screen.
    pub fn fake_answer(&self) -> String {
        let opening = |text: &str| text.split_whitespace().take(14).collect::<Vec<_>>().join(" ");
        match self.sources.as_slice() {
            [] => match &self.about {
                Some(about) => format!("{} turns on this: “{}…” Name a book in your library and I'll bring its passages.", about.label, opening(&about.text)),
                None => "Name a book or an article in your library and I'll answer from its passages.".to_string(),
            },
            [one] => format!("{} speaks to this directly: “{}…” [1]", one.title, opening(&one.text)),
            [one, two, ..] => format!(
                "{} speaks to this directly: “{}…” [1]\n\nIt comes back to the thought later, more plainly: “{}…” [2]",
                one.title,
                opening(&one.text),
                opening(&two.text)
            ),
        }
    }
}

/// The passages `answer` cites, as indexes into the sources, in the order first cited. A number
/// outside `1..=sources` is not a citation.
pub fn cited(answer: &str, sources: usize) -> Vec<usize> {
    let mut out = vec![];
    for piece in answer.split('[').skip(1) {
        let Some((number, _)) = piece.split_once(']') else { continue };
        if let Ok(n) = number.trim().parse::<usize>()
            && (1..=sources).contains(&n)
            && !out.contains(&(n - 1))
        {
            out.push(n - 1);
        }
    }
    out
}

/// The words that say what `text` is about, lower case, each once, in order, at most `most`: words
/// of four letters or more that aren't [`COMMON`].
pub fn keywords(text: &str, most: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '\'') {
        if out.len() == most {
            break;
        }
        let word = word.trim_matches('\'').to_lowercase();
        if word.chars().count() >= 4 && !COMMON.contains(&word.as_str()) && !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// The titles `asked` names, as indexes into `titles`: the whole title, or the part before its
/// subtitle ("Walden" of "Walden; or, Life in the Woods"), as whole words, ignoring case.
pub fn named(asked: &str, titles: &[&str]) -> Vec<usize> {
    let asked = fold(asked);
    titles
        .iter()
        .enumerate()
        .filter(|(_, title)| {
            let whole = fold(title);
            let short = fold(title.split([';', ':', '(', '—']).next().unwrap_or(title));
            [whole, short].iter().any(|name| name.chars().filter(|c| c.is_alphanumeric()).count() >= 3 && has_words(&asked, name))
        })
        .map(|(at, _)| at)
        .collect()
}

/// Whether `asked`, in Writing, asks for the draft to change rather than for an answer. It does
/// when it opens with a verb that changes text ("Tighten the second paragraph", "Can you cut
/// this?"), and when it is neither a question nor names a book.
pub fn is_edit(asked: &str, names_a_book: bool) -> bool {
    const CHANGES: &[&str] = &[
        "add", "change", "cut", "edit", "expand", "fix", "make", "polish", "rephrase", "remove", "replace", "rewrite",
        "reword", "shorten", "simplify", "tighten", "trim", "turn", "write", "continue", "finish", "correct",
    ];
    const ASKS: &[&str] = &[
        "what", "why", "how", "who", "whom", "where", "when", "which", "does", "do", "did", "is", "are", "was", "were",
        "explain", "summarize", "summarise", "tell", "compare", "define",
    ];
    let words: Vec<String> = fold(asked).split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).collect();
    let politeness = ["can", "could", "would", "will", "please", "you"];
    let first = words.iter().find(|w| !politeness.contains(&w.as_str())).map(String::as_str).unwrap_or("");
    if CHANGES.contains(&first) {
        return true;
    }
    !(asked.trim_end().ends_with('?') || ASKS.contains(&first) || names_a_book)
}

fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).map(|c| if c == '’' { '\'' } else { c }).collect()
}

/// Whether `name` occurs in `text` with no letter or digit touching either end.
fn has_words(text: &str, name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && text.match_indices(name).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + name.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn source(title: &str, text: &str) -> Source {
        Source { title: title.into(), author: Some("Henry David Thoreau".into()), chapter: "Economy".into(), text: text.into() }
    }

    #[test]
    fn the_prompt_numbers_each_passage_and_the_fake_answer_cites_them() {
        let about = About { label: "On Keeping a Quiet Desk".into(), text: "A desk is a promise.".into() };
        let sources = vec![source("Walden", "I went to the woods because I wished to live deliberately."), source("Walden", "Simplify, simplify.")];
        let question = Question::new("  What does Walden say about this?  ", Some(about), sources).unwrap();
        let prompt = question.prompt();
        assert!(prompt.user.contains("<passage n=\"1\" from=\"Walden by Henry David Thoreau, Economy\">"));
        assert!(prompt.user.contains("<passage n=\"2\""));
        assert!(prompt.user.contains("looking at On Keeping a Quiet Desk"));
        assert!(prompt.user.ends_with("Question: What does Walden say about this?"));
        assert_eq!(cited(&question.fake_answer(), 2), [0, 1]);
        assert_eq!(Question::new(" ", None, vec![]), Err(BadRequest::EmptyInstruction));
    }

    #[test]
    fn a_title_is_named_by_its_words_and_a_question_is_told_from_an_edit() {
        let titles = ["Walden; or, Life in the Woods", "An Apology for Idlers", "Pride and Prejudice", "It"];
        assert_eq!(named("what does walden say about this?", &titles), [0]);
        assert_eq!(named("Compare An Apology for Idlers with Pride and Prejudice", &titles), [1, 2]);
        assert!(named("Waldenesque ideas, and it is late", &titles).is_empty(), "whole words only, and never a two-letter title");

        for edit in ["Tighten the second paragraph", "Can you cut this?", "make it warmer", "the ending feels abrupt"] {
            assert!(is_edit(edit, false), "{edit}");
        }
        for question in ["What is this paragraph saying?", "Explain the second sentence", "does this hold up", "Walden on solitude"] {
            assert!(!is_edit(question, question.starts_with("Walden")), "{question}");
        }
    }

    proptest! {
        /// Only the numbers of passages that exist are citations, each once, in the order first cited.
        #[test]
        fn citations_are_passages_that_exist(numbers in prop::collection::vec(0usize..8, 0..10), sources in 0usize..6) {
            let answer: String = numbers.iter().map(|n| format!("Said so [{n}]. ")).collect();
            let found = cited(&answer, sources);
            let mut expected: Vec<usize> = vec![];
            for n in &numbers {
                if (1..=sources).contains(n) && !expected.contains(&(n - 1)) {
                    expected.push(n - 1);
                }
            }
            prop_assert_eq!(found, expected);
        }

        /// Keywords are distinct, lower case, long enough, never common, and at most as many as asked.
        #[test]
        fn keywords_are_distinct_long_and_uncommon(text in "\\PC{0,200}", most in 0usize..10) {
            let words = keywords(&text, most);
            prop_assert!(words.len() <= most);
            for (at, word) in words.iter().enumerate() {
                prop_assert!(word.chars().count() >= 4 && !COMMON.contains(&word.as_str()));
                prop_assert_eq!(word, &word.to_lowercase());
                prop_assert!(!words[..at].contains(word));
            }
        }
    }
}

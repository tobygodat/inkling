//! The quick actions: what the writer asks for, the prompt it becomes, and how a complete answer
//! becomes a [`Review`]. [`Revision`] is the one AI interaction a document can have open.

use crate::ai::{AiEvent, Prompt};
use crate::review::{Edit, Review, is_range_of};
use std::fmt;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    FixGrammar,
    /// Continue the draft at this byte offset.
    NextLine { at: usize },
    /// Other words for the selected word or phrase.
    FindWord(Range<usize>),
    /// Anything the writer types into the ask field.
    Freeform(String),
}

/// An [`Ask`] about one draft, checked against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    draft: String,
    ask: Ask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadRequest {
    /// The offset or range is outside the draft or splits a character.
    OutOfRange,
    /// Find a word with nothing but whitespace selected.
    EmptySelection,
    /// A free-form ask with no words in it.
    EmptyInstruction,
}

impl fmt::Display for BadRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BadRequest::OutOfRange => "The text changed under the request.",
            BadRequest::EmptySelection => "Select a word or phrase first.",
            BadRequest::EmptyInstruction => "Type what you'd like changed.",
        })
    }
}

impl std::error::Error for BadRequest {}

/// Why a complete answer couldn't become a review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerError {
    Empty,
    /// Find a word got something other than a list of words.
    NotAList,
    /// Find a word got a list with nothing new in it.
    NoAlternatives,
}

impl fmt::Display for AnswerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AnswerError::Empty => "The model returned nothing.",
            AnswerError::NotAList => "The model's answer wasn't a list of words. Try again.",
            AnswerError::NoAlternatives => "The model found no other words for this.",
        })
    }
}

impl std::error::Error for AnswerError {}

const SYSTEM: &str = "You are the editor inside a minimal writing app. The author reviews every \
    change you make, one at a time, so change only what the request needs. Keep everything else \
    exactly as written: the author's words, voice, tense, line breaks and formatting.";

const WHOLE_DRAFT: &str = "Reply with the whole revised draft and nothing else: no preamble, no \
    commentary, no quotation marks or code fences around it.";

impl Request {
    /// A selection for Find a word is trimmed to its words.
    pub fn new(draft: impl Into<String>, ask: Ask) -> Result<Self, BadRequest> {
        let draft = draft.into();
        let ask = match ask {
            Ask::NextLine { at } if !is_range_of(&draft, &(at..at)) => return Err(BadRequest::OutOfRange),
            Ask::FindWord(range) => {
                if !is_range_of(&draft, &range) {
                    return Err(BadRequest::OutOfRange);
                }
                let selected = &draft[range.clone()];
                let trimmed = selected.trim();
                if trimmed.is_empty() {
                    return Err(BadRequest::EmptySelection);
                }
                let start = range.start + (selected.len() - selected.trim_start().len());
                Ask::FindWord(start..start + trimmed.len())
            }
            Ask::Freeform(instruction) if instruction.trim().is_empty() => {
                return Err(BadRequest::EmptyInstruction);
            }
            Ask::Freeform(instruction) => Ask::Freeform(instruction.trim().to_string()),
            ask => ask,
        };
        Ok(Request { draft, ask })
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn ask(&self) -> &Ask {
        &self.ask
    }

    pub fn prompt(&self) -> Prompt {
        let draft = &self.draft;
        let user = match &self.ask {
            Ask::FixGrammar => format!(
                "Fix the spelling, grammar and punctuation in this draft. Don't reword anything \
                 that is already correct.\n\n<draft>\n{draft}\n</draft>\n\n{WHOLE_DRAFT}"
            ),
            Ask::Freeform(instruction) => format!(
                "Revise this draft as the author asks.\n\nRequest: {instruction}\n\n\
                 <draft>\n{draft}\n</draft>\n\n{WHOLE_DRAFT}"
            ),
            Ask::NextLine { at } => format!(
                "Continue this draft from the cursor with one to three sentences in the author's \
                 voice.\n\n<before_cursor>\n{}\n</before_cursor>\n\n<after_cursor>\n{}\n</after_cursor>\n\n\
                 Reply with only the new text that goes at the cursor: no preamble, no commentary, \
                 no quotation marks.",
                &draft[..*at],
                &draft[*at..]
            ),
            Ask::FindWord(range) => format!(
                "Suggest other words or phrases for the selected text that fit its sentence and the \
                 author's voice.\n\n<before>\n{}\n</before>\n\n<selected>\n{}\n</selected>\n\n\
                 <after>\n{}\n</after>\n\nReply with only a JSON array of 3 to 6 strings, best first, \
                 for example [\"quiet\", \"hushed\", \"still\"]. Leave out the selected text itself.",
                &draft[..range.start],
                &draft[range.clone()],
                &draft[range.end..]
            ),
        };
        Prompt { system: SYSTEM.into(), user }
    }

    /// Turns the model's complete answer into the changes to review.
    pub fn review(&self, answer: &str) -> Result<Review, AnswerError> {
        let answer = unfence(answer);
        if answer.trim().is_empty() {
            return Err(AnswerError::Empty);
        }
        let draft = self.draft.clone();
        match &self.ask {
            Ask::FixGrammar | Ask::Freeform(_) => Ok(Review::new(draft, &self.reframe(answer.trim()))),
            Ask::NextLine { at } => {
                let text = self.fit_at(*at, answer.trim_end().trim_start_matches([' ', '\t']));
                Ok(Review::alternatives(draft, *at..*at, vec![text]).expect("checked in Request::new"))
            }
            Ask::FindWord(range) => {
                let words = parse_alternatives(answer, &self.draft[range.clone()])?;
                Ok(Review::alternatives(draft, range.clone(), words).expect("checked in Request::new"))
            }
        }
    }

    /// A canned answer to this request, for headless runs and trying the app with no key.
    pub fn fake_answer(&self) -> String {
        match &self.ask {
            Ask::FixGrammar => fake_fix(&self.draft),
            Ask::Freeform(_) => fake_rewrite(&self.draft),
            Ask::NextLine { .. } => crate::store::FAKE_TEXT.to_string(),
            Ask::FindWord(_) => r#"["hushed", "quiet", "still", "muted"]"#.to_string(),
        }
    }

    /// Models drop or add blank lines at the ends. The draft's own leading and trailing
    /// whitespace stays, so it never shows up as a change.
    fn reframe(&self, body: &str) -> String {
        let draft = &self.draft;
        if draft.trim().is_empty() {
            return format!("{draft}{body}");
        }
        let lead = &draft[..draft.len() - draft.trim_start().len()];
        let trail = &draft[draft.trim_end().len()..];
        format!("{lead}{body}{trail}")
    }

    /// Adds the space a continuation needs to sit between its neighbours.
    fn fit_at(&self, at: usize, text: &str) -> String {
        let before = self.draft[..at].chars().next_back();
        let after = self.draft[at..].chars().next();
        let (first, last) = (text.chars().next(), text.chars().next_back());
        let mut out = String::with_capacity(text.len() + 2);
        if let (Some(a), Some(b)) = (before, first)
            && needs_space(a, b)
        {
            out.push(' ');
        }
        out.push_str(text);
        if let (Some(a), Some(b)) = (last, after)
            && needs_space(a, b)
        {
            out.push(' ');
        }
        out
    }
}

/// Whether two characters that end up side by side need a space between them. Scripts written
/// without spaces, such as CJK, are all three or more bytes in UTF-8, so a two-byte limit keeps
/// to Latin, Greek and Cyrillic text.
fn needs_space(a: char, b: char) -> bool {
    !a.is_whitespace()
        && !b.is_whitespace()
        && a.len_utf8() < 3
        && b.len_utf8() < 3
        && !matches!(a, '(' | '[' | '“' | '‘')
        && !matches!(b, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '”' | '’')
}

/// The answer without a code fence around it.
fn unfence(answer: &str) -> &str {
    let trimmed = answer.trim();
    let Some(inner) = trimmed.strip_prefix("```").and_then(|s| s.strip_suffix("```")) else {
        return answer;
    };
    inner.split_once('\n').map_or(inner, |(_lang, body)| body)
}

/// Reads Find a word's answer: a JSON array of strings, perhaps with words around it. Drops
/// blanks, repeats and the original. Fails when no array of strings is there or none is left.
pub fn parse_alternatives(answer: &str, original: &str) -> Result<Vec<String>, AnswerError> {
    let (Some(open), Some(close)) = (answer.find('['), answer.rfind(']')) else {
        return Err(AnswerError::NotAList);
    };
    if close < open {
        return Err(AnswerError::NotAList);
    }
    let list: Vec<String> = serde_json::from_str(&answer[open..=close]).map_err(|_| AnswerError::NotAList)?;
    let original = original.trim();
    let mut words: Vec<String> = Vec::with_capacity(list.len());
    for word in list {
        let word = word.trim();
        if !word.is_empty() && word != original && !words.iter().any(|w| w == word) {
            words.push(word.to_string());
        }
    }
    if words.is_empty() { Err(AnswerError::NoAlternatives) } else { Ok(words) }
}

/// The AI side of a document: nothing, a request being answered, a version under review, or a
/// failure to show. A UI renders this and keeps no copy of it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Revision {
    #[default]
    Idle,
    /// The answer so far. It becomes a version only at `Done`.
    Asking { request: Request, answer: String },
    Reviewing(Review),
    Failed(String),
}

impl Revision {
    pub fn asking(request: Request) -> Self {
        Revision::Asking { request, answer: String::new() }
    }

    /// Events that arrive once the revision has moved on are dropped.
    pub fn apply(&mut self, event: AiEvent) {
        let Revision::Asking { request, answer } = self else { return };
        match event {
            AiEvent::Delta(text) => answer.push_str(&text),
            AiEvent::Done => {
                *self = match request.review(answer) {
                    Ok(review) => Revision::Reviewing(review),
                    Err(e) => Revision::Failed(e.to_string()),
                }
            }
            AiEvent::Failed(message) => *self = Revision::Failed(message),
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Revision::Asking { .. })
    }

    pub fn review(&self) -> Option<&Review> {
        match self {
            Revision::Reviewing(review) => Some(review),
            _ => None,
        }
    }

    pub fn review_mut(&mut self) -> Option<&mut Review> {
        match self {
            Revision::Reviewing(review) => Some(review),
            _ => None,
        }
    }

    /// Ends the review and hands back its accepted edits, last first (see [`Review::edits`]).
    pub fn finish(&mut self) -> Vec<Edit> {
        let edits = self.review().map(Review::edits).unwrap_or_default();
        *self = Revision::Idle;
        edits
    }

    /// Back to the draft with nothing applied.
    pub fn dismiss(&mut self) {
        *self = Revision::Idle;
    }
}

const FIXES: [(&str, &str); 8] = [
    ("teh", "the"),
    ("i", "I"),
    ("dont", "don't"),
    ("cant", "can't"),
    ("recieve", "receive"),
    ("wich", "which"),
    ("alot", "a lot"),
    ("its'", "its"),
];

fn fake_fix(draft: &str) -> String {
    let mut out = String::with_capacity(draft.len() + 1);
    for token in draft.split_word_bounds() {
        let fixed = FIXES.iter().find(|(wrong, _)| *wrong == token).map(|(_, right)| *right);
        match fixed {
            Some(right) => out.push_str(right),
            None if token.len() > 1 && token.chars().all(|c| c == ' ') => out.push(' '),
            None => out.push_str(token),
        }
    }
    let body_end = out.trim_end().len();
    if out[..body_end].ends_with(|c: char| c.is_alphanumeric()) {
        out.insert(body_end, '.');
    }
    out
}

const FILLERS: [&str; 6] = ["very", "really", "just", "quite", "actually", "simply"];
const SWAPS: [(&str, &str); 6] =
    [("big", "vast"), ("said", "murmured"), ("walked", "wandered"), ("good", "fine"), ("looked", "glanced"), ("small", "slight")];

fn fake_rewrite(draft: &str) -> String {
    let tokens: Vec<&str> = draft.split_word_bounds().collect();
    let mut out = String::with_capacity(draft.len());
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if FILLERS.contains(&token.to_lowercase().as_str()) && tokens.get(i + 1) == Some(&" ") {
            i += 2;
            continue;
        }
        out.push_str(SWAPS.iter().find(|(from, _)| *from == token).map_or(token, |(_, to)| to));
        i += 1;
    }
    if out == draft {
        let body_end = out.trim_end().len();
        let gap = if body_end == 0 { "" } else { " " };
        out.insert_str(body_end, &format!("{gap}{}", crate::store::FAKE_TEXT));
    }
    out
}

#[cfg(test)]
mod tests;

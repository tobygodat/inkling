use crate::ai::AiEvent;
use std::ops::Range;

/// Text the AI proposes in place of `replace`. An empty range inserts at that offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub replace: Range<usize>,
    pub text: String,
}

/// The one AI interaction a document can have open. The document itself never changes
/// until `accept`, so dismissing is always free.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Assist {
    #[default]
    Idle,
    Streaming(Proposal),
    Ready(Proposal),
    Failed(String),
}

impl Assist {
    pub fn begin(replace: Range<usize>) -> Self {
        Assist::Streaming(Proposal { replace, text: String::new() })
    }

    /// Events that arrive after a dismiss or an accept are dropped.
    pub fn apply(&mut self, event: AiEvent) {
        let Assist::Streaming(proposal) = self else { return };
        match event {
            AiEvent::Delta(text) => proposal.text.push_str(&text),
            AiEvent::Done if proposal.text.trim().is_empty() => {
                *self = Assist::Failed("The model returned nothing.".into())
            }
            AiEvent::Done => *self = Assist::Ready(proposal.clone()),
            AiEvent::Failed(message) => *self = Assist::Failed(message),
        }
    }

    pub fn proposal(&self) -> Option<&Proposal> {
        match self {
            Assist::Streaming(p) | Assist::Ready(p) => Some(p),
            Assist::Idle | Assist::Failed(_) => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Assist::Streaming(_))
    }

    /// Writes a ready proposal into `doc` and returns the offset just after it.
    pub fn accept(&mut self, doc: &mut String) -> Option<usize> {
        let Assist::Ready(proposal) = self else { return None };
        let range = proposal.replace.clone();
        if range.end > doc.len() || !doc.is_char_boundary(range.start) || !doc.is_char_boundary(range.end) {
            *self = Assist::Failed("The text changed under the suggestion.".into());
            return None;
        }
        let end = range.start + proposal.text.len();
        doc.replace_range(range, &proposal.text);
        *self = Assist::Idle;
        Some(end)
    }

    pub fn dismiss(&mut self) {
        *self = Assist::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(replace: Range<usize>, text: &str) -> Assist {
        let mut a = Assist::begin(replace);
        a.apply(AiEvent::Delta(text.into()));
        a.apply(AiEvent::Done);
        a
    }

    #[test]
    fn accept_replaces_the_selection_and_returns_the_cursor() {
        let mut doc = String::from("a quick fox");
        let mut a = ready(2..7, "slow");
        assert_eq!(a.accept(&mut doc), Some(6));
        assert_eq!(doc, "a slow fox");
        assert_eq!(a, Assist::Idle);
    }

    #[test]
    fn an_empty_range_inserts() {
        let mut doc = String::from("Once");
        let mut a = ready(4..4, " upon a time");
        assert_eq!(a.accept(&mut doc), Some(16));
        assert_eq!(doc, "Once upon a time");
    }

    #[test]
    fn nothing_is_written_while_streaming_or_after_dismiss() {
        let mut doc = String::from("x");
        let mut a = Assist::begin(1..1);
        a.apply(AiEvent::Delta("y".into()));
        assert_eq!(a.accept(&mut doc), None);
        a.dismiss();
        a.apply(AiEvent::Delta("z".into()));
        a.apply(AiEvent::Done);
        assert_eq!((a, doc.as_str()), (Assist::Idle, "x"));
    }

    #[test]
    fn a_stale_range_fails_instead_of_corrupting() {
        let mut doc = String::from("ab");
        let mut a = ready(1..9, "z");
        assert_eq!(a.accept(&mut doc), None);
        assert!(matches!(a, Assist::Failed(_)));
        assert_eq!(doc, "ab");
    }

    #[test]
    fn an_empty_answer_is_a_failure() {
        let mut a = Assist::begin(0..0);
        a.apply(AiEvent::Done);
        assert!(matches!(a, Assist::Failed(_)));
    }
}

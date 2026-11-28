//! The Ask bar (TOB-559) as Swift sees it: the passages a question names, the answer streamed from
//! the provider with the passages it cites, and the one conversation kept on disk.

use std::sync::{Arc, Mutex, MutexGuard};

use inkling_books::{BookId, search};
use inkling_core::{AiEvent, Cancel, Paths, Question, Settings, backend_for, question};

use crate::{AiEngine, InklingError, Library};

/// The most books a question pulls passages from, and passages from each.
const BOOKS: usize = 3;
const PER_BOOK: usize = 3;

/// What the question is asked about: the screen's document or article and the words on it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AskAbout {
    pub label: String,
    pub text: String,
}

/// A passage of the library, numbered as the prompt lists it, and where it is (ADR 0006).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Citation {
    pub n: u32,
    pub item: String,
    pub title: String,
    pub place: String,
    pub chapter: u32,
    pub start: u64,
    pub end: u64,
    pub words: String,
}

/// One turn of the conversation.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Turn {
    pub asked: String,
    pub context: String,
    pub answer: String,
    pub citations: Vec<Citation>,
    pub edit: bool,
    pub made: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnswerState {
    Asking { text: String },
    Answered { text: String, citations: Vec<Citation> },
    Failed { message: String },
}

/// Called off the main thread as the answer streams in, finishes or fails.
#[uniffi::export(with_foreign)]
pub trait AnswerListener: Send + Sync {
    fn on_answer(&self, state: AnswerState);
}

/// An answer on its way. Dropping it stops it.
#[derive(uniffi::Object)]
pub struct AnswerSession {
    cancel: Cancel,
}

#[uniffi::export]
impl AnswerSession {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for AnswerSession {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[uniffi::export]
impl Library {
    /// The passages a question draws on: from each book or article it names by title, and each one
    /// `attached`, the paragraphs holding most of the question's words, or of `about`'s when the
    /// question has none of its own.
    pub fn sources(&self, asked: String, about: Option<String>, attached: Vec<String>) -> Vec<Citation> {
        let library = self.lock();
        let entries = library.entries();
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        let mut ids: Vec<String> = question::named(&asked, &titles).into_iter().map(|at| entries[at].id.0.clone()).collect();
        for id in attached {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let named: Vec<String> = ids.iter().filter_map(|id| library.entry(&BookId(id.clone()))).flat_map(|e| question::keywords(&e.title, 8)).collect();
        // The question's own words pick the passages; "what does Walden say about this?" has none,
        // so the words on screen do.
        let own = |text: &str| question::keywords(text, 24).into_iter().filter(|w| !named.contains(w)).take(12).collect::<Vec<_>>();
        let mut words = own(&asked);
        if words.is_empty() {
            words = own(&about.unwrap_or_default());
        }
        let mut out = vec![];
        for id in ids.into_iter().take(BOOKS) {
            let Ok(book) = library.load(&BookId(id)) else { continue };
            for found in search::passages(&book, &words, PER_BOOK) {
                let chapter = &book.chapters[found.chapter];
                out.push(Citation {
                    n: out.len() as u32 + 1,
                    item: book.id.0.clone(),
                    title: book.title.clone(),
                    place: if book.chapters.len() > 1 { chapter.label(found.chapter) } else { String::new() },
                    chapter: found.chapter as u32,
                    start: found.start as u64,
                    end: found.end as u64,
                    words: found.text,
                });
            }
        }
        out
    }

    /// Whether `asked`, in Writing, asks for the draft to change rather than for an answer.
    pub fn wants_edit(&self, asked: String) -> bool {
        let library = self.lock();
        let titles: Vec<&str> = library.entries().iter().map(|e| e.title.as_str()).collect();
        question::is_edit(&asked, !question::named(&asked, &titles).is_empty())
    }
}

#[uniffi::export]
impl AiEngine {
    /// Asks `asked` about `about` with `sources` to cite, streaming the answer to `listener`.
    pub fn start_question(&self, asked: String, about: Option<AskAbout>, sources: Vec<Citation>, listener: Arc<dyn AnswerListener>) -> Result<Arc<AnswerSession>, InklingError> {
        let cited: Vec<question::Source> = sources
            .iter()
            .map(|c| question::Source { title: c.title.clone(), author: None, chapter: c.place.clone(), text: c.words.clone() })
            .collect();
        let about = about.map(|a| question::About { label: a.label, text: a.text });
        let question = Question::new(asked, about, cited).map_err(|e| InklingError::InvalidRequest { message: e.to_string() })?;
        let settings = Settings::load(&self.paths);
        let backend = backend_for(&settings, self.keys.as_ref(), || question.fake_answer())
            .map_err(|missing| InklingError::MissingKey { provider: missing.label().to_string() })?;
        let mut text = String::new();
        let cancel = inkling_core::start(question.prompt(), backend, move |event| {
            let state = match event {
                AiEvent::Delta(delta) => {
                    text.push_str(&delta);
                    AnswerState::Asking { text: text.clone() }
                }
                AiEvent::Done => AnswerState::Answered {
                    citations: question::cited(&text, sources.len()).into_iter().map(|at| sources[at].clone()).collect(),
                    text: text.trim().to_string(),
                },
                AiEvent::Failed(message) => AnswerState::Failed { message },
            };
            listener.on_answer(state);
        });
        Ok(Arc::new(AnswerSession { cancel }))
    }
}

/// The one conversation, kept in `conversation.json`.
#[derive(uniffi::Object)]
pub struct Conversation {
    paths: Paths,
    kept: Mutex<inkling_core::Conversation>,
}

#[uniffi::export]
impl Conversation {
    #[uniffi::constructor]
    pub fn open(data: String) -> Arc<Conversation> {
        let paths = Paths { dir: data.into() };
        let kept = inkling_core::Conversation::load(&paths);
        Arc::new(Conversation { paths, kept: Mutex::new(kept) })
    }

    pub fn turns(&self) -> Vec<Turn> {
        self.lock().turns.iter().cloned().map(Turn::from).collect()
    }

    /// Adds a turn and keeps the conversation on disk.
    pub fn add(&self, turn: Turn) -> Result<(), InklingError> {
        let mut kept = self.lock();
        kept.push(turn.into());
        kept.save(&self.paths)?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), InklingError> {
        let mut kept = self.lock();
        kept.turns.clear();
        kept.save(&self.paths)?;
        Ok(())
    }
}

impl Conversation {
    fn lock(&self) -> MutexGuard<'_, inkling_core::Conversation> {
        self.kept.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl From<inkling_core::Citation> for Citation {
    fn from(c: inkling_core::Citation) -> Self {
        Citation { n: c.n, item: c.item, title: c.title, place: c.place, chapter: c.chapter, start: c.start, end: c.end, words: c.words }
    }
}

impl From<Citation> for inkling_core::Citation {
    fn from(c: Citation) -> Self {
        inkling_core::Citation { n: c.n, item: c.item, title: c.title, place: c.place, chapter: c.chapter, start: c.start, end: c.end, words: c.words }
    }
}

impl From<inkling_core::Turn> for Turn {
    fn from(t: inkling_core::Turn) -> Self {
        Turn { asked: t.asked, context: t.context, answer: t.answer, citations: t.citations.into_iter().map(Citation::from).collect(), edit: t.edit, made: t.made }
    }
}

impl From<Turn> for inkling_core::Turn {
    fn from(t: Turn) -> Self {
        inkling_core::Turn { asked: t.asked, context: t.context, answer: t.answer, citations: t.citations.into_iter().map(Into::into).collect(), edit: t.edit, made: t.made }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkling_books::{Book, Chapter, Format};
    use std::sync::mpsc;

    struct Collect(mpsc::Sender<AnswerState>);

    impl AnswerListener for Collect {
        fn on_answer(&self, state: AnswerState) {
            let _ = self.0.send(state);
        }
    }

    #[test]
    fn a_question_naming_a_book_is_answered_from_its_passages_with_citations() {
        // SAFETY: tests in this crate that read INKLING_FAKE_AI only ever set it.
        unsafe { std::env::set_var("INKLING_FAKE_AI", "1") };
        let data = tempfile::tempdir().unwrap();
        let folder = data.path().to_string_lossy().into_owned();
        let library = Library::open(folder.clone()).unwrap();
        let woods = "I went to the woods because I wished to live deliberately, to front only the essential facts of life.";
        let winter = "In winter the pond froze solid, and I walked across it deliberately, listening to the ice settle.";
        let book = Book {
            id: BookId("walden".into()),
            title: "Walden; or, Life in the Woods".into(),
            author: Some("Henry David Thoreau".into()),
            format: Format::Epub,
            parser: inkling_books::PARSER,
            chapters: vec![
                Chapter { title: Some("Economy".into()), text: format!("# Economy\n\n{woods}") },
                Chapter { title: Some("The Pond in Winter".into()), text: format!("Ice.\n\n{winter}") },
            ],
            source: None,
        };
        library.lock().insert(&book).unwrap();

        let sources = library.sources("What does Walden say about living deliberately?".into(), Some("I want to live deliberately.".into()), vec![]);
        assert_eq!(sources.iter().map(|c| (c.n, c.place.as_str())).collect::<Vec<_>>(), [(1, "Economy"), (2, "The Pond in Winter")]);
        let text = &book.chapters[1].text;
        assert_eq!(&text[sources[1].start as usize..sources[1].end as usize], winter, "a citation is its chapter's own bytes");
        assert!(library.sources("What is the weather?".into(), None, vec![]).is_empty(), "nothing named, nothing drawn");
        let this = library.sources("What does Walden say about this?".into(), Some("The pond froze in winter.".into()), vec![]);
        assert_eq!(this.iter().map(|c| c.place.as_str()).collect::<Vec<_>>(), ["The Pond in Winter"], "a question with no words of its own takes the screen's");
        assert!(!library.wants_edit("Walden on solitude".into()) && library.wants_edit("Tighten this".into()));

        let engine = AiEngine::open(folder.clone());
        let (tx, rx) = mpsc::channel();
        let _session = engine.start_question("What does Walden say about living deliberately?".into(), None, sources.clone(), Arc::new(Collect(tx))).unwrap();
        let answered = rx.iter().find(|s| !matches!(s, AnswerState::Asking { .. })).unwrap();
        let AnswerState::Answered { text, citations } = answered else { panic!("{answered:?}") };
        assert!(text.contains("[1]") && text.contains("[2]"), "{text}");
        assert_eq!(citations, sources);

        let conversation = Conversation::open(folder.clone());
        conversation.add(Turn { asked: "What does Walden say?".into(), context: "Inbox".into(), answer: text, citations, edit: false, made: 1 }).unwrap();
        assert_eq!(Conversation::open(folder).turns(), conversation.turns(), "the conversation is kept on disk");
    }
}

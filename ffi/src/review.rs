//! AI providers, key storage and inline review as Swift sees them, directly over
//! `inkling_core`'s `Request`, `Revision`, `Review` and `KeyStore`.

use std::sync::{Arc, Mutex, MutexGuard};

use inkling_core::{
    AiEvent, Ask, Cancel, ChangeId, Decision, KeyStore, Paths, Provider, Request, Review, Revision,
    Secret, Segment, Settings,
};

use crate::InklingError;

/// The three bring-your-own-key providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AiProvider {
    Claude,
    Gemini,
    ChatGpt,
}

impl From<Provider> for AiProvider {
    fn from(p: Provider) -> Self {
        match p {
            Provider::Claude => AiProvider::Claude,
            Provider::Gemini => AiProvider::Gemini,
            Provider::ChatGpt => AiProvider::ChatGpt,
        }
    }
}

impl From<AiProvider> for Provider {
    fn from(p: AiProvider) -> Self {
        match p {
            AiProvider::Claude => Provider::Claude,
            AiProvider::Gemini => Provider::Gemini,
            AiProvider::ChatGpt => Provider::ChatGpt,
        }
    }
}

#[uniffi::export]
pub fn provider_label(provider: AiProvider) -> String {
    Provider::from(provider).label().to_string()
}

#[uniffi::export]
pub fn provider_default_model(provider: AiProvider) -> String {
    Provider::from(provider).default_model().to_string()
}

/// What the writer asks the provider to do. Offsets are UTF-8 byte offsets into the draft.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AskKind {
    FixGrammar,
    NextLine { at: u64 },
    FindWord { start: u64, end: u64 },
    Freeform { instruction: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ChangeDecision {
    Pending,
    Accepted,
    Rejected,
}

impl From<Decision> for ChangeDecision {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Pending => ChangeDecision::Pending,
            Decision::Accepted => ChangeDecision::Accepted,
            Decision::Rejected => ChangeDecision::Rejected,
        }
    }
}

/// One change in a review.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReviewChange {
    pub id: u32,
    /// UTF-8 byte range in the draft that this change replaces.
    pub start: u64,
    pub end: u64,
    pub removed: String,
    pub added: String,
    pub decision: ChangeDecision,
    pub wordings: Vec<String>,
    pub chosen: u32,
    pub more: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReviewSegment {
    Unchanged { text: String },
    Change { change: ReviewChange },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReviewStats {
    pub words_added: u32,
    pub words_removed: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub pending: u32,
    pub total: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RevisionState {
    Idle,
    Asking { words_so_far: u32 },
    Reviewing { stats: ReviewStats, segments: Vec<ReviewSegment> },
    Failed { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RevisionSnapshot {
    pub provider: AiProvider,
    pub provider_label: String,
    pub state: RevisionState,
}

/// Replace UTF-8 byte range `start..end` of the draft with `text`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TextEdit {
    pub start: u64,
    pub end: u64,
    pub text: String,
}

/// Called off the main thread whenever a running request streams words, finishes or fails.
#[uniffi::export(with_foreign)]
pub trait RevisionListener: Send + Sync {
    fn on_update(&self, snapshot: RevisionSnapshot);
}

struct SessionInner {
    provider: Provider,
    revision: Revision,
    cancel: Option<Cancel>,
}

impl SessionInner {
    fn snapshot(&self) -> RevisionSnapshot {
        let state = match &self.revision {
            Revision::Idle => RevisionState::Idle,
            Revision::Asking { answer, .. } => RevisionState::Asking {
                words_so_far: answer.split_whitespace().count() as u32,
            },
            Revision::Reviewing(review) => {
                let s = review.stats();
                let stats = ReviewStats {
                    words_added: s.words_added as u32,
                    words_removed: s.words_removed as u32,
                    accepted: s.accepted as u32,
                    rejected: s.rejected as u32,
                    pending: s.pending as u32,
                    total: s.total as u32,
                };
                let mut next_id: u32 = 0;
                let segments = review
                    .segments()
                    .into_iter()
                    .map(|seg| match seg {
                        Segment::Unchanged(text) => ReviewSegment::Unchanged { text: text.to_string() },
                        Segment::Change(c) => {
                            let id = next_id;
                            next_id += 1;
                            ReviewSegment::Change {
                                change: ReviewChange {
                                    id,
                                    start: c.range.start as u64,
                                    end: c.range.end as u64,
                                    removed: c.removed.to_string(),
                                    added: c.added.to_string(),
                                    decision: c.decision.into(),
                                    wordings: c.wordings.to_vec(),
                                    chosen: c.chosen as u32,
                                    more: c.more as u32,
                                },
                            }
                        }
                    })
                    .collect();
                RevisionState::Reviewing { stats, segments }
            }
            Revision::Failed(message) => RevisionState::Failed { message: message.clone() },
        };
        RevisionSnapshot {
            provider: self.provider.into(),
            provider_label: self.provider.label().to_string(),
            state,
        }
    }
}

/// One AI request and the review it produces, backed directly by `inkling_core::Revision`.
#[derive(uniffi::Object)]
pub struct ReviewSession {
    inner: Arc<Mutex<SessionInner>>,
}

impl Drop for ReviewSession {
    fn drop(&mut self) {
        if let Some(cancel) = self.lock().cancel.take() {
            cancel.cancel();
        }
    }
}

#[uniffi::export]
impl ReviewSession {
    pub fn snapshot(&self) -> RevisionSnapshot {
        self.lock().snapshot()
    }

    pub fn cancel(&self) {
        let mut inner = self.lock();
        if let Some(cancel) = inner.cancel.take() {
            cancel.cancel();
        }
        inner.revision.dismiss();
    }

    pub fn accept(&self, id: u32) -> RevisionSnapshot {
        self.mutate_review(id, |review, cid| review.accept(cid))
    }

    pub fn reject(&self, id: u32) -> RevisionSnapshot {
        self.mutate_review(id, |review, cid| review.reject(cid))
    }

    pub fn undo(&self, id: u32) -> RevisionSnapshot {
        self.mutate_review(id, |review, cid| review.undo(cid))
    }

    pub fn choose(&self, id: u32, wording: u32) -> RevisionSnapshot {
        self.mutate_review(id, |review, cid| review.choose(cid, wording as usize))
    }

    pub fn accept_rest(&self) -> RevisionSnapshot {
        let mut inner = self.lock();
        if let Some(review) = inner.revision.review_mut() {
            review.accept_rest();
        }
        inner.snapshot()
    }

    pub fn next_pending(&self, after: Option<u32>) -> Option<u32> {
        let inner = self.lock();
        let review = inner.revision.review()?;
        let after_id = after.and_then(|idx| change_id(review, idx));
        let next = review.next_pending(after_id)?;
        review.changes().position(|c| c.id == next).map(|i| i as u32)
    }

    /// The draft with every accepted change applied, while a review is open.
    pub fn result_text(&self) -> Option<String> {
        self.lock().revision.review().map(Review::result)
    }

    /// Ends the review and returns the accepted edits, last first, ready to apply to the draft.
    pub fn finish(&self) -> Vec<TextEdit> {
        let mut inner = self.lock();
        if let Some(cancel) = inner.cancel.take() {
            cancel.cancel();
        }
        inner
            .revision
            .finish()
            .into_iter()
            .map(|e| TextEdit { start: e.range.start as u64, end: e.range.end as u64, text: e.text })
            .collect()
    }

    pub fn dismiss(&self) {
        self.cancel();
    }
}

impl ReviewSession {
    fn lock(&self) -> MutexGuard<'_, SessionInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn mutate_review(&self, id: u32, f: impl FnOnce(&mut Review, ChangeId)) -> RevisionSnapshot {
        let mut inner = self.lock();
        if let Some(review) = inner.revision.review_mut()
            && let Some(cid) = change_id(review, id)
        {
            f(review, cid);
        }
        inner.snapshot()
    }
}

fn change_id(review: &Review, index: u32) -> Option<ChangeId> {
    review.changes().nth(index as usize).map(|c| c.id)
}

/// Provider settings, Keychain access (in-memory in headless runs) and starting a review.
#[derive(uniffi::Object)]
pub struct AiEngine {
    paths: Paths,
    keys: Box<dyn KeyStore>,
    lock: Mutex<()>,
}

#[uniffi::export]
impl AiEngine {
    /// Opens the AI engine for `data`. Headless runs (`INKLING_FAKE_AI` or `INKLING_MEMORY_KEYS`)
    /// use in-memory keys and never touch the real Keychain.
    #[uniffi::constructor]
    pub fn open(data: String) -> Arc<AiEngine> {
        Arc::new(AiEngine {
            paths: Paths { dir: data.into() },
            keys: inkling_core::default_keys(),
            lock: Mutex::new(()),
        })
    }

    pub fn is_fake_ai(&self) -> bool {
        std::env::var_os("INKLING_FAKE_AI").is_some()
    }

    pub fn provider(&self) -> AiProvider {
        let _guard = self.guard();
        Settings::load(&self.paths).provider.into()
    }

    pub fn set_provider(&self, provider: AiProvider) -> Result<(), InklingError> {
        let _guard = self.guard();
        let mut settings = Settings::load(&self.paths);
        settings.provider = provider.into();
        settings.save(&self.paths)?;
        Ok(())
    }

    pub fn model(&self, provider: AiProvider) -> String {
        let _guard = self.guard();
        Settings::load(&self.paths).model(provider.into()).to_string()
    }

    pub fn typed_model(&self, provider: AiProvider) -> Option<String> {
        let _guard = self.guard();
        Settings::load(&self.paths).typed_model(provider.into()).map(str::to_string)
    }

    pub fn set_model(&self, provider: AiProvider, model: String) -> Result<(), InklingError> {
        let _guard = self.guard();
        let mut settings = Settings::load(&self.paths);
        settings.set_model(provider.into(), &model);
        settings.save(&self.paths)?;
        Ok(())
    }

    pub fn has_key(&self, provider: AiProvider) -> bool {
        self.keys.has(Secret::ApiKey(provider.into()))
    }

    pub fn set_key(&self, provider: AiProvider, key: String) -> Result<(), InklingError> {
        self.keys
            .set(Secret::ApiKey(provider.into()), &key)
            .map_err(|message| InklingError::Keychain { message })
    }

    pub fn remove_key(&self, provider: AiProvider) -> Result<(), InklingError> {
        self.keys
            .remove(Secret::ApiKey(provider.into()))
            .map_err(|message| InklingError::Keychain { message })
    }

    /// Starts one AI request against `draft`, streaming updates to `listener`.
    pub fn start_ask(
        &self,
        draft: String,
        ask: AskKind,
        listener: Arc<dyn RevisionListener>,
    ) -> Result<Arc<ReviewSession>, InklingError> {
        let ask = match ask {
            AskKind::FixGrammar => Ask::FixGrammar,
            AskKind::NextLine { at } => Ask::NextLine { at: at as usize },
            AskKind::FindWord { start, end } => Ask::FindWord(start as usize..end as usize),
            AskKind::Freeform { instruction } => Ask::Freeform(instruction),
        };
        let request = Request::new(draft, ask)
            .map_err(|e| InklingError::InvalidRequest { message: e.to_string() })?;
        let settings = {
            let _guard = self.guard();
            Settings::load(&self.paths)
        };
        let provider = settings.provider;
        let backend = inkling_core::backend_for_request(&settings, self.keys.as_ref(), &request)
            .map_err(|missing| InklingError::MissingKey { provider: missing.label().to_string() })?;

        let inner = Arc::new(Mutex::new(SessionInner {
            provider,
            revision: Revision::asking(request.clone()),
            cancel: None,
        }));
        let shared = Arc::clone(&inner);
        let cancel = inkling_core::start(&request, backend, move |event: AiEvent| {
            let snapshot = {
                let mut guard = shared.lock().unwrap_or_else(|p| p.into_inner());
                guard.revision.apply(event);
                guard.snapshot()
            };
            listener.on_update(snapshot);
        });
        inner.lock().unwrap_or_else(|p| p.into_inner()).cancel = Some(cancel);
        Ok(Arc::new(ReviewSession { inner }))
    }
}

impl AiEngine {
    fn guard(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkling_core::{Backend, MemoryKeys};
    use std::sync::mpsc;
    use std::time::Duration;

    struct Collect(mpsc::Sender<RevisionSnapshot>);

    impl RevisionListener for Collect {
        fn on_update(&self, snapshot: RevisionSnapshot) {
            let _ = self.0.send(snapshot);
        }
    }

    fn run_fake(draft: &str, ask: Ask) -> Arc<ReviewSession> {
        let request = Request::new(draft, ask).unwrap();
        let backend = Backend::Fake { text: request.fake_answer(), word_delay: Duration::ZERO };
        let (tx, rx) = mpsc::channel();
        let listener = Arc::new(Collect(tx));
        let inner = Arc::new(Mutex::new(SessionInner {
            provider: Provider::Claude,
            revision: Revision::asking(request.clone()),
            cancel: None,
        }));
        let shared = Arc::clone(&inner);
        let cancel = inkling_core::start(&request, backend, move |event| {
            let snapshot = {
                let mut guard = shared.lock().unwrap();
                guard.revision.apply(event);
                guard.snapshot()
            };
            listener.on_update(snapshot);
        });
        inner.lock().unwrap().cancel = Some(cancel);
        for snap in rx {
            if matches!(snap.state, RevisionState::Reviewing { .. } | RevisionState::Failed { .. }) {
                break;
            }
        }
        Arc::new(ReviewSession { inner })
    }

    #[test]
    fn review_session_accepts_rejects_chooses_and_finishes_through_core() {
        let draft = "We walked to teh quiet pier and i dont think anyone saw";
        let session = run_fake(draft, Ask::FixGrammar);
        let snap = session.snapshot();
        let RevisionState::Reviewing { stats, segments } = snap.state else {
            panic!("expected reviewing, got {:?}", snap.state);
        };
        assert!(stats.total >= 3, "got {:?}", stats);
        assert_eq!(stats.pending, stats.total);
        assert!(segments.iter().any(|s| matches!(s, ReviewSegment::Change { change } if change.removed == "teh" && change.added == "the")));

        // Accept change 0, reject change 1, undo change 1, then accept the rest.
        let s1 = session.accept(0);
        assert!(matches!(s1.state, RevisionState::Reviewing { stats, .. } if stats.accepted == 1));
        let s2 = session.reject(1);
        assert!(matches!(s2.state, RevisionState::Reviewing { stats, .. } if stats.rejected == 1));
        let s3 = session.undo(1);
        assert!(matches!(s3.state, RevisionState::Reviewing { stats, .. } if stats.rejected == 0));

        // Leave change 1 rejected, accept the rest, verify edits produce the exact result_text.
        session.reject(1);
        session.accept_rest();
        let expected = session.result_text().unwrap();
        let edits = session.finish();
        let mut applied = draft.to_string();
        for edit in edits {
            applied.replace_range(edit.start as usize..edit.end as usize, &edit.text);
        }
        assert_eq!(applied, expected);
        assert!(applied.contains("the quiet pier"));

        // FindWord exposes multiple wordings and lets the writer choose one.
        let find = run_fake("A loud room", Ask::FindWord(2..6));
        let choose = find.choose(0, 2);
        let RevisionState::Reviewing { segments, .. } = choose.state else { panic!("expected reviewing") };
        let change = segments.iter().find_map(|s| match s { ReviewSegment::Change { change } => Some(change), _ => None }).unwrap();
        assert_eq!(change.added, "still");
        assert_eq!(change.more, 3);
    }

    #[test]
    fn engine_uses_memory_keys_and_reports_missing_key() {
        let data = tempfile::tempdir().unwrap();
        let engine = AiEngine {
            paths: Paths { dir: data.path().to_path_buf() },
            keys: Box::new(MemoryKeys::default()),
            lock: Mutex::new(()),
        };
        assert!(!engine.has_key(AiProvider::Claude));
        engine.set_provider(AiProvider::Gemini).unwrap();
        assert_eq!(engine.provider(), AiProvider::Gemini);
        engine.set_model(AiProvider::Gemini, "custom-gemini".into()).unwrap();
        assert_eq!(engine.model(AiProvider::Gemini), "custom-gemini");
        engine.set_key(AiProvider::Gemini, "secret-key".into()).unwrap();
        assert!(engine.has_key(AiProvider::Gemini));
        engine.remove_key(AiProvider::Gemini).unwrap();
        assert!(!engine.has_key(AiProvider::Gemini));
    }
}

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Provider {
    Claude,
    Gemini,
    ChatGpt,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Claude, Provider::Gemini, Provider::ChatGpt];

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Gemini => "Gemini",
            Provider::ChatGpt => "ChatGPT",
        }
    }

    /// Stable name for storage: the keychain account and the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Gemini => "gemini",
            Provider::ChatGpt => "chatgpt",
        }
    }

    pub fn default_model(self) -> &'static str {
        crate::providers::default_model(self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Keep writing from the cursor.
    Continue,
    /// Rewrite the selection as the instruction says.
    Rewrite(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiRequest {
    pub action: Action,
    pub before: String,
    pub selection: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

impl AiRequest {
    pub fn prompt(&self) -> Prompt {
        const SYSTEM: &str = "You are the writing partner inside a minimal writing app. \
            Match the author's voice, tense and formatting. Reply with only the text that goes \
            into the document: no preamble, no quotation marks around it, no commentary.";
        let user = match &self.action {
            Action::Continue => format!(
                "Continue this draft from where it stops. Write one to three sentences.\n\n\
                 <draft_before_cursor>\n{}\n</draft_before_cursor>\n\n\
                 <draft_after_cursor>\n{}\n</draft_after_cursor>",
                self.before, self.after
            ),
            Action::Rewrite(instruction) => format!(
                "Rewrite the selected passage. Instruction: {instruction}\n\n\
                 <before>\n{}\n</before>\n\n<selected>\n{}\n</selected>\n\n<after>\n{}\n</after>\n\n\
                 Reply with only the replacement for the selected passage.",
                self.before, self.selection, self.after
            ),
        };
        Prompt { system: SYSTEM.into(), user }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiEvent {
    Delta(String),
    Done,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Backend {
    /// Canned text, one word per `word_delay`. For headless runs and for trying the app with no key.
    Fake { text: String, word_delay: Duration },
    Live { provider: Provider, model: String, key: String },
}

#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Runs the request on its own thread. `sink` gets zero or more `Delta`s, then exactly one
/// `Done` or `Failed`, unless the request is cancelled, after which it gets nothing more.
pub fn start(request: AiRequest, backend: Backend, mut sink: impl FnMut(AiEvent) + Send + 'static) -> Cancel {
    let cancel = Cancel::default();
    let seen = cancel.clone();
    std::thread::spawn(move || {
        let mut emit = |event: AiEvent| {
            if !seen.is_cancelled() {
                sink(event)
            }
        };
        let result = match backend {
            Backend::Fake { text, word_delay } => {
                for word in text.split_inclusive(' ') {
                    if seen.is_cancelled() {
                        return;
                    }
                    std::thread::sleep(word_delay);
                    emit(AiEvent::Delta(word.into()));
                }
                Ok(())
            }
            Backend::Live { provider, model, key } => {
                let prompt = request.prompt();
                crate::providers::stream(provider, &model, &key, &prompt, &seen, &mut |text| {
                    emit(AiEvent::Delta(text))
                })
            }
        };
        emit(match result {
            Ok(()) => AiEvent::Done,
            Err(message) => AiEvent::Failed(message),
        });
    });
    cancel
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn request() -> AiRequest {
        AiRequest { action: Action::Continue, before: "Once".into(), selection: String::new(), after: String::new() }
    }

    #[test]
    fn the_fake_backend_streams_its_text_then_finishes() {
        let (tx, rx) = mpsc::channel();
        let backend = Backend::Fake { text: "upon a time".into(), word_delay: Duration::ZERO };
        start(request(), backend, move |e| tx.send(e).unwrap());
        let events: Vec<_> = rx.iter().collect();
        let text: String = events.iter().filter_map(|e| if let AiEvent::Delta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(text, "upon a time");
        assert_eq!(events.last(), Some(&AiEvent::Done));
    }

    #[test]
    fn a_rewrite_prompt_carries_the_instruction_and_the_selection() {
        let p = AiRequest { action: Action::Rewrite("tighten".into()), selection: "very big".into(), ..request() }.prompt();
        assert!(p.user.contains("tighten") && p.user.contains("<selected>\nvery big\n</selected>"));
    }
}

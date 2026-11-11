//! Inkling's writing slice on Rune. No Mac-only code, so an iPhone shell can host the same view.
//!
//! Host contract:
//! - [`configure`] once, before [`spec`], with where this install keeps its files.
//! - [`spec`] builds the `AppSpec` the host runs.

use std::ops::Range;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use inkling_core::{Action, AiEvent, AiRequest, Assist, Cancel, KeyStore, MemoryKeys, Paths, Provider, Settings, backend_for, default_keys};
use rune::{AppSpec, Key, KeyEvent, TextEdit, Tx, Waker};

mod view;

static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn configure(paths: Paths) {
    let _ = PATHS.set(paths);
}

const SAVE_AFTER: f64 = 0.5;

/// The request whose events feed `State::assist`. A new request replaces the receiver, so a
/// cancelled one's queued events are dropped with it.
struct Request {
    events: Receiver<AiEvent>,
    cancel: Cancel,
}

pub struct CommandBar {
    pub edit: TextEdit,
}

pub struct SettingsForm {
    pub provider: Provider,
    pub key: TextEdit,
    pub model: TextEdit,
    /// Opened because an action needed this provider's key.
    pub missing: bool,
}

pub struct State {
    paths: Paths,
    pub settings: Settings,
    keys: Box<dyn KeyStore>,
    pub doc: TextEdit,
    pub assist: Assist,
    request: Option<Request>,
    pub bar: Option<CommandBar>,
    pub form: Option<SettingsForm>,
    seen: String,
    edited_at: Option<f64>,
    waker: Option<Waker>,
    last_frame: f64,
}

fn init() -> State {
    state(PATHS.get().cloned().unwrap_or_else(Paths::mac))
}

fn state(paths: Paths) -> State {
    let settings = Settings::load(&paths);
    // Headless and fake-AI runs never touch the real Keychain, whatever `default_keys` becomes.
    let isolated = rune::tooling::headless_run().is_some() || std::env::var_os("INKLING_FAKE_AI").is_some();
    let keys: Box<dyn KeyStore> = if isolated { Box::new(MemoryKeys::default()) } else { default_keys() };
    let text = paths.load_draft();
    let mut doc = TextEdit::new(text.clone());
    doc.set_caret(text.len());
    doc.focused = true;
    State {
        paths,
        settings,
        keys,
        doc,
        assist: Assist::Idle,
        request: None,
        bar: None,
        form: None,
        seen: text,
        edited_at: None,
        waker: None,
        last_frame: 0.0,
    }
}

impl State {
    fn cancel(&mut self) {
        if let Some(r) = self.request.take() {
            r.cancel.cancel();
        }
    }

    pub fn dismiss(&mut self) {
        self.cancel();
        self.assist.dismiss();
        self.doc.focused = true;
    }

    /// Cmd-J, or Cmd-K's Return: ask for `action` over `replace`.
    fn start(&mut self, action: Action, replace: Range<usize>) {
        self.cancel();
        let backend = match backend_for(&self.settings, self.keys.as_ref()) {
            Ok(b) => b,
            Err(provider) => {
                self.assist.dismiss();
                self.open_settings(provider);
                if let Some(f) = &mut self.form {
                    f.missing = true;
                }
                return;
            }
        };
        let t = &self.doc.text;
        let request = AiRequest {
            action,
            before: t[..replace.start].into(),
            selection: t[replace.clone()].into(),
            after: t[replace.end..].into(),
        };
        let (tx, events) = channel();
        let waker = self.waker.clone();
        let cancel = inkling_core::start(request, backend, move |e| {
            let _ = tx.send(e);
            if let Some(w) = &waker {
                w();
            }
        });
        self.assist = Assist::begin(replace);
        self.request = Some(Request { events, cancel });
        self.doc.focused = true;
    }

    fn continue_at_caret(&mut self) {
        let (_, b) = self.doc.range();
        self.start(Action::Continue, b..b);
    }

    fn open_bar(&mut self) {
        self.dismiss();
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.bar = Some(CommandBar { edit });
    }

    pub fn submit_bar(&mut self) {
        let Some(bar) = self.bar.take() else { return };
        let instruction = bar.edit.text.trim().to_string();
        let (a, b) = self.doc.range();
        if instruction.is_empty() || a == b {
            self.doc.focused = true;
            return;
        }
        self.start(Action::Rewrite(instruction), a..b);
    }

    pub fn close_bar(&mut self) {
        self.bar = None;
        self.doc.focused = true;
    }

    /// Writes the ready proposal through `Assist::accept`, then lands the same change in the
    /// editor as one undo step.
    pub fn accept(&mut self) {
        let Some(p) = self.assist.proposal().cloned() else { return };
        let mut text = self.doc.text.clone();
        if self.assist.accept(&mut text).is_none() {
            return;
        }
        self.request = None;
        self.doc.set_selection(p.replace.start, p.replace.end);
        self.doc.break_history_group();
        self.doc.insert(&p.text);
        self.doc.break_history_group();
        debug_assert_eq!(self.doc.text, text);
        self.doc.focused = true;
    }

    pub fn open_settings(&mut self, provider: Provider) {
        self.bar = None;
        self.doc.focused = false;
        self.form = Some(self.form_for(provider));
    }

    fn form_for(&self, provider: Provider) -> SettingsForm {
        let mut key = TextEdit::new(self.keys.get(provider).unwrap_or_default()).secure();
        key.focused = true;
        let chosen = self.settings.model(provider);
        let model = TextEdit::new(if chosen == provider.default_model() { "" } else { chosen });
        SettingsForm { provider, key, model, missing: false }
    }

    pub fn pick_provider(&mut self, provider: Provider) {
        self.form = Some(self.form_for(provider));
    }

    pub fn save_settings(&mut self) {
        let Some(form) = self.form.take() else { return };
        let key = form.key.text.trim();
        if !key.is_empty() {
            let _ = self.keys.set(form.provider, key);
        }
        self.settings.provider = form.provider;
        self.settings.set_model(form.provider, &form.model.text);
        let _ = self.settings.save(&self.paths);
        self.doc.focused = true;
    }

    pub fn close_settings(&mut self) {
        self.form = None;
        self.doc.focused = true;
    }

    pub fn words(&self) -> usize {
        self.doc.text.split_whitespace().count()
    }
}

fn setup(s: &mut State, waker: Waker) {
    s.waker = Some(waker);
}

/// Streamed events into `assist`. True when any arrived.
fn drain(s: &mut State) -> bool {
    let Some(r) = &s.request else { return false };
    let mut changed = false;
    while let Ok(e) = r.events.try_recv() {
        s.assist.apply(e);
        changed = true;
    }
    if !s.assist.is_busy() {
        s.request = None;
    }
    changed
}

/// Before every frame: streamed text into `assist`, and the draft saved once typing settles.
fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    let dt = (now - std::mem::replace(&mut s.last_frame, now)).clamp(0.0, 0.1);
    if s.request.is_some() && rune::tooling::headless_run().is_some() {
        // A headless run's clock is virtual and runs far ahead of the request's thread, which
        // sleeps in wall time. Hold each frame for its virtual length while a request streams.
        std::thread::sleep(Duration::from_secs_f64(dt));
    }
    changed |= drain(s);
    if s.doc.text != s.seen {
        s.seen = s.doc.text.clone();
        s.edited_at = Some(now);
    }
    if let Some(at) = s.edited_at {
        if now - at >= SAVE_AFTER {
            let _ = s.paths.save_draft(&s.doc.text);
            s.edited_at = None;
        } else {
            tx.wake_after(Duration::from_secs_f64(SAVE_AFTER - (now - at)));
        }
    }
    changed
}

/// The editor's keys before it edits. While a proposal shows, Tab (and Return for a rewrite)
/// accepts, Esc dismisses, and anything else dismisses and then edits as usual.
pub(crate) fn editor_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.is_command('j') || ev.is_command('k') || ev.is_command(',') {
        return on_key(s, ev, tx);
    }
    match (&s.assist, &ev.key) {
        (Assist::Idle | Assist::Failed(_), Key::Enter) if !ev.mods.cmd => {
            s.doc.insert("\n");
            true
        }
        (Assist::Idle, _) => false,
        (Assist::Failed(_), Key::Escape) => {
            s.assist.dismiss();
            true
        }
        (Assist::Failed(_), _) => false,
        (_, Key::Escape) => {
            s.dismiss();
            true
        }
        (Assist::Ready(_), Key::Tab) => {
            s.accept();
            true
        }
        (Assist::Ready(p), Key::Enter) if !p.replace.is_empty() => {
            s.accept();
            true
        }
        (Assist::Streaming(_), Key::Tab) => true,
        _ => {
            s.dismiss();
            false
        }
    }
}

fn on_key(s: &mut State, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if ev.is_command(',') {
        s.open_settings(s.settings.provider);
        return true;
    }
    if s.form.is_some() {
        if ev.key == Key::Escape {
            s.close_settings();
            return true;
        }
        return false;
    }
    if ev.is_command('j') {
        s.bar = None;
        s.continue_at_caret();
        return true;
    }
    if ev.is_command('k') {
        let (a, b) = s.doc.range();
        if a == b {
            s.bar = None;
            s.continue_at_caret();
        } else {
            s.open_bar();
        }
        return true;
    }
    if ev.key == Key::Escape {
        if s.bar.is_some() {
            s.close_bar();
        } else {
            s.dismiss();
        }
        return true;
    }
    false
}

pub fn spec() -> AppSpec<State> {
    AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        min_size: Some((360.0, 420.0)),
        ..AppSpec::new("Inkling", (900.0, 680.0), init, view::view)
    }
}

#[cfg(test)]
mod tests;

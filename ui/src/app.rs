use crate::keys::KeyScreen;
use crate::{FormFactor, Metrics, SANS, SERIF, color};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{AiEvent, Ask, Cancel, ChangeId, KeyStore, Paths, Provider, Request, Revision, Settings};
use std::sync::mpsc;
use std::time::Duration;

gpui_kit::actions!(inkling, [OpenKeys]);

const AUTOSAVE: Duration = Duration::from_millis(500);

/// Whether the page on screen is the page on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saved {
    Saved,
    Pending,
    Failed(String),
}

/// A request on its way. Dropping it cancels the request.
struct Running {
    cancel: Cancel,
    events: mpsc::Receiver<AiEvent>,
    _pump: Task<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// What the version bar names: the version's number and who wrote it.
#[derive(Clone, Copy)]
pub(crate) struct Version {
    pub number: u32,
    pub source: Provider,
}

pub struct Inkling {
    pub(crate) form: FormFactor,
    pub(crate) paths: Paths,
    pub(crate) settings: Settings,
    pub(crate) keys: Box<dyn KeyStore>,
    /// Asked of the key store when the provider or its key changes, never on a frame.
    pub(crate) key_saved: bool,
    pub(crate) page: Entity<TextareaState>,
    pub(crate) ask_field: Entity<InputState>,
    /// All AI state. The views render it and keep no copy.
    pub(crate) revision: Revision,
    running: Option<Running>,
    pub(crate) version: Version,
    /// A line for something that went right but needs saying, such as an empty review.
    pub(crate) notice: Option<SharedString>,
    pub(crate) key_screen: Option<KeyScreen>,
    /// The change whose other wordings are open.
    pub(crate) wordings_open: Option<ChangeId>,
    /// The change tapped on iPhone, which the sheet acts on.
    pub(crate) selected: Option<ChangeId>,
    saved: Saved,
    save: Task<()>,
    pub(crate) focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Inkling {
    pub fn new(form: FormFactor, paths: Paths, keys: Box<dyn KeyStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load(&paths);
        let draft = paths.load_draft();
        let page = cx.new(|cx| {
            let mut page = TextareaState::new(window, cx).soft_wrap(true);
            page.set_value(draft, window, cx);
            page
        });
        let ask_field = cx.new(|cx| InputState::new(window, cx).placeholder("Ask for fixes, words or a nudge"));
        let edited = cx.subscribe_in(&page, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.edited(cx);
            }
        });
        let sent = cx.subscribe_in(&ask_field, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.send(window, cx);
            }
        });
        let view = cx.entity().downgrade();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            let handled = view.update(cx, |this, cx| this.key(&event.keystroke, window, cx)).unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        page.read(cx).focus_handle(cx).focus(window, cx);
        let key_saved = keys.has(settings.provider);
        Inkling {
            form,
            paths,
            version: Version { number: 1, source: settings.provider },
            settings,
            keys,
            key_saved,
            page,
            ask_field,
            revision: Revision::Idle,
            running: None,
            notice: None,
            key_screen: None,
            wordings_open: None,
            selected: None,
            saved: Saved::Saved,
            save: Task::ready(()),
            focus: cx.focus_handle(),
            _subs: vec![edited, sent, intercept],
        }
    }

    pub fn text(&self, cx: &App) -> String {
        self.page.read(cx).value().to_string()
    }

    pub fn revision(&self) -> &Revision {
        &self.revision
    }

    pub fn saved(&self) -> &Saved {
        &self.saved
    }

    pub fn page(&self) -> &Entity<TextareaState> {
        &self.page
    }

    pub fn ask_field(&self) -> &Entity<InputState> {
        &self.ask_field
    }

    pub fn key_screen_provider(&self) -> Option<Provider> {
        self.key_screen.as_ref().map(|k| k.provider)
    }

    /// Whether the key store holds a key for the chosen provider.
    pub fn key_saved(&self) -> bool {
        self.key_saved
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Forwards one event from the request to the revision. The pump calls this for every
    /// event the backend sends.
    pub fn feed(&mut self, event: AiEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.revision.apply(event);
        if !self.revision.is_busy() {
            self.running = None;
        }
        if let Some(review) = self.revision.review()
            && review.is_empty()
        {
            self.revision.finish();
            self.notice = Some(format!("{} found nothing to change.", self.version.source.label()).into());
            self.back_to_page(window, cx);
        }
        cx.notify();
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        if self.revision.is_busy() {
            self.cancel(cx);
        }
        self.notice = None;
        if matches!(self.revision, Revision::Failed(_)) {
            self.revision.dismiss();
        }
        self.saved = Saved::Pending;
        self.save = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE).await;
            let _ = this.update(cx, |this, cx| {
                this.saved = match this.paths.save_draft(&this.text(cx)) {
                    Ok(()) => Saved::Saved,
                    Err(e) => Saved::Failed(e.to_string()),
                };
                cx.notify();
            });
        });
        cx.notify();
    }

    /// Keys the review, a running request, the key screen or a message own. Runs before any
    /// binding, so the editor never sees them while they mean something here.
    fn key(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if key.modifiers.modified() {
            return false;
        }
        match key.key.as_str() {
            "escape" => self.escape(window, cx),
            "tab" | "enter" if self.revision.review().is_some() && self.key_screen.is_none() => {
                let typing = key.key == "enter"
                    && self.ask_field.read(cx).focus_handle(cx).is_focused(window)
                    && !self.ask_field.read(cx).value().trim().is_empty();
                if typing {
                    return false;
                }
                self.accept_next(window, cx);
                true
            }
            _ => false,
        }
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.key_screen.is_some() {
            self.close_keys(window, cx);
        } else if self.wordings_open.is_some() {
            self.wordings_open = None;
        } else if self.revision.is_busy() {
            self.cancel(cx);
        } else if self.revision.review().is_some() {
            self.finish(window, cx);
        } else if matches!(self.revision, Revision::Failed(_)) || self.notice.is_some() {
            self.revision.dismiss();
            self.notice = None;
        } else {
            return false;
        }
        cx.notify();
        true
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.running = None;
        self.revision.dismiss();
        cx.notify();
    }

    pub fn fix_grammar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(|_| Ask::FixGrammar, window, cx);
    }

    pub fn find_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(|page| Ask::FindWord(page.selected_range()), window, cx);
    }

    pub fn next_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(|page| Ask::NextLine { at: page.cursor() }, window, cx);
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let instruction = self.ask_field.read(cx).value().to_string();
        self.ask(move |_| Ask::Freeform(instruction), window, cx);
        if self.revision.is_busy() {
            self.ask_field.update(cx, |field, cx| field.set_value("", window, cx));
        }
    }

    /// Asks about the page as it stands. A review still open ends first, keeping what was accepted.
    fn ask(&mut self, ask: impl FnOnce(&TextareaState) -> Ask, window: &mut Window, cx: &mut Context<Self>) {
        if self.revision.review().is_some() {
            self.finish(window, cx);
        }
        self.running = None;
        self.notice = None;
        let ask = ask(self.page.read(cx));
        let request = match Request::new(self.text(cx), ask) {
            Ok(request) => request,
            Err(bad) => {
                self.revision = Revision::Failed(bad.to_string());
                return cx.notify();
            }
        };
        let backend = match inkling_core::backend_for_request(&self.settings, self.keys.as_ref(), &request) {
            Ok(backend) => backend,
            Err(provider) => return self.open_keys(provider, window, cx),
        };
        let (tx, events) = mpsc::channel();
        let cancel = inkling_core::start(&request, backend, move |event| {
            let _ = tx.send(event);
        });
        let pump = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let busy = this.update_in(cx, |this, window, cx| this.drain(window, cx)).unwrap_or(false);
                if !busy {
                    break;
                }
            }
        });
        self.version = Version { number: self.version.number + 1, source: self.settings.provider };
        self.revision = Revision::asking(request);
        self.running = Some(Running { cancel, events, _pump: pump });
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut events = vec![];
        if let Some(running) = &self.running {
            events.extend(running.events.try_iter());
        }
        for event in events {
            self.feed(event, window, cx);
        }
        cx.notify();
        self.revision.is_busy()
    }

    pub fn accept(&mut self, id: ChangeId, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.accept(id));
    }

    pub fn reject(&mut self, id: ChangeId, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.reject(id));
    }

    pub fn undo(&mut self, id: ChangeId, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.undo(id));
    }

    pub fn choose(&mut self, id: ChangeId, wording: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.wordings_open = None;
        self.decide(window, cx, |review| review.choose(id, wording));
    }

    pub fn accept_rest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.accept_rest());
    }

    pub fn accept_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.revision.review().and_then(|r| r.next_pending(None));
        match next {
            Some(id) => self.accept(id, window, cx),
            None => self.finish(window, cx),
        }
    }

    pub fn toggle_wordings(&mut self, id: ChangeId, cx: &mut Context<Self>) {
        self.wordings_open = if self.wordings_open == Some(id) { None } else { Some(id) };
        cx.notify();
    }

    pub fn select(&mut self, id: Option<ChangeId>, cx: &mut Context<Self>) {
        self.selected = id;
        self.wordings_open = None;
        cx.notify();
    }

    /// Applies a decision, and ends the review once nothing is left to decide.
    fn decide(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut inkling_core::Review)) {
        let Some(review) = self.revision.review_mut() else { return };
        f(review);
        if review.stats().pending == 0 {
            self.finish(window, cx);
        }
        cx.notify();
    }

    /// Ends the review: each accepted change goes into the page through the editor, one undo
    /// step per change, and the page is editable again.
    pub fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edits = self.revision.finish();
        self.page.update(cx, |page, cx| {
            for edit in edits {
                page.set_selected_range(edit.range, cx);
                page.replace(edit.text, window, cx);
            }
        });
        self.back_to_page(window, cx);
    }

    fn back_to_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.wordings_open = None;
        self.selected = None;
        self.page.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn open_keys(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let screen = KeyScreen::new(provider, self, window, cx);
        screen.key.read(cx).focus_handle(cx).focus(window, cx);
        self.key_screen = Some(screen);
        cx.notify();
    }

    pub fn close_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_screen = None;
        self.key_saved = self.keys.has(self.settings.provider);
        if self.revision.review().is_none() {
            self.page.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn on_open_keys(&mut self, _: &OpenKeys, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.settings.provider;
        self.open_keys(provider, window, cx);
    }

    pub(crate) fn metrics(&self) -> Metrics {
        self.form.metrics()
    }

    /// The version bar's slot. It keeps its height when empty, so the page doesn't move when a
    /// review starts or ends.
    fn render_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let button = self.metrics().control.max(px(32.));
        let slot = div().w_full().h(button + px(30.)).flex_none();
        match self.render_version_bar(cx) {
            Some(bar) => slot.child(bar).into_any_element(),
            None => slot.into_any_element(),
        }
    }

    fn render_version_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.metrics();
        let source = self.version.source.label();
        let content: AnyElement = match &self.revision {
            Revision::Asking { answer, .. } => {
                let words = answer.split_whitespace().count();
                let progress = match words {
                    0 => format!("asking {source}…"),
                    1 => format!("asking {source}… 1 word so far"),
                    n => format!("asking {source}… {n} words so far"),
                };
                bar_row()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Version {}", self.version.number)))
                    .child(div().flex_grow(1.).text_color(rgb(color::MUTED)).child(progress))
                    .child(ghost_button("cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| this.cancel(cx))).test_support())
                    .into_any_element()
            }
            Revision::Reviewing(review) => {
                let s = review.stats();
                let name = div().font_weight(FontWeight::SEMIBOLD).child(format!("Version {}", self.version.number));
                let numbers = div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .text_color(rgb(color::MUTED))
                    .child(format!("from {source} ·"))
                    .child(div().text_color(rgb(color::ADDED_COUNT)).child(format!("+{}", s.words_added)))
                    .child(div().text_color(rgb(color::REMOVED_LINE)).child(format!("−{}", s.words_removed)))
                    .child(format!("· {} of {} accepted", s.accepted, s.total));
                let label = match self.form {
                    FormFactor::Mac => div().flex_grow(1.).flex().items_center().gap(px(10.)).child(name).child(numbers),
                    FormFactor::Phone => div().flex_grow(1.).min_w_0().flex().flex_col().child(name).child(numbers.text_size(px(13.))),
                };
                bar_row()
                    .child(label)
                    .when(self.form == FormFactor::Mac, |row| {
                        row.child(
                            ghost_button("back", "Back to draft")
                                .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx)))
                                .test_support(),
                        )
                    })
                    .child(
                        primary_button("accept-rest", "Accept the rest", m)
                            .flex_none()
                            .on_click(cx.listener(|this, _, window, cx| this.accept_rest(window, cx)))
                            .test_support(),
                    )
                    .into_any_element()
            }
            _ => return None,
        };
        Some(
            div()
                .w_full()
                .flex()
                .justify_center()
                .pt_4()
                .px(m.gutter)
                .child(
                    div()
                        .w_full()
                        .max_w(m.column)
                        .pl(px(14.))
                        .pr(px(6.))
                        .py(px(6.))
                        .bg(rgb(color::SURFACE))
                        .border_1()
                        .border_color(rgb(color::HAIRLINE))
                        .rounded(px(10.))
                        .text_size(m.ui)
                        .child(content),
                )
                .into_any_element(),
        )
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.revision.review().is_some() {
            return self.render_review(window, cx);
        }
        let m = self.metrics();
        Textarea::new(&self.page)
            .appearance(false)
            .h_full()
            .font_family(SERIF)
            .text_size(m.body)
            .line_height(relative(m.line))
            .text_color(rgb(color::INK))
            .into_any_element()
    }

    fn render_message(&self) -> Option<impl IntoElement + use<>> {
        let line: SharedString = match (&self.revision, &self.notice) {
            (Revision::Failed(message), _) => message.clone().into(),
            (_, Some(notice)) => notice.clone(),
            _ => return None,
        };
        Some(div().id("message").text_size(px(13.)).text_color(rgb(color::MUTED)).truncate().child(line))
    }

    fn render_asks(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.metrics();
        let quick = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            div()
                .id(id)
                .h(px(30.))
                .px_3()
                .flex()
                .items_center()
                .bg(rgb(color::SURFACE))
                .border_1()
                .border_color(rgb(color::HAIRLINE))
                .rounded(px(15.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| f(this, window, cx)))
                .test_support()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(quick("fix-grammar", "Fix grammar", Self::fix_grammar))
                    .child(quick("find-word", "Find a word", Self::find_word))
                    .child(quick("next-line", "Next line", Self::next_line)),
            )
            .child(
                div()
                    .h(px(46.))
                    .pl(px(14.))
                    .pr(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .bg(rgb(color::SURFACE))
                    .border_1()
                    .border_color(rgb(color::HAIRLINE))
                    .rounded(px(12.))
                    .child(div().flex_1().min_w_0().text_size(px(14.)).child(Input::new(&self.ask_field).id("ask").appearance(false)))
                    .child(primary_button("send", "Send", m).on_click(cx.listener(|this, _, window, cx| this.send(window, cx))).test_support()),
            )
    }

    fn render_footer(&self) -> impl IntoElement + use<> {
        let saved = match &self.saved {
            Saved::Saved => "Saved".to_string(),
            Saved::Pending => "Editing".to_string(),
            Saved::Failed(e) => format!("Couldn't save the draft ({e})."),
        };
        let provider = self.settings.provider.label();
        let key = if self.key_saved {
            format!("{provider}, on your key")
        } else {
            format!("{provider}, no key yet · ⌘, to add one")
        };
        div()
            .w_full()
            .flex()
            .justify_between()
            .gap_3()
            .px_6()
            .pt_2()
            .pb(px(14.))
            .text_size(px(12.))
            .text_color(rgb(color::MUTED))
            .child(saved)
            .child(key)
    }
}

impl Render for Inkling {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.metrics();
        let root = div()
            .id("inkling")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(color::GROUND))
            .text_color(rgb(color::INK))
            .font_family(SANS)
            .on_action(cx.listener(Self::on_open_keys));
        if let Some(screen) = &self.key_screen {
            return root.child(self.render_keys(screen, cx));
        }
        let page = self.render_page(window, cx);
        root.child(self.render_bar(cx))
            .child(
                div().flex_1().min_h_0().w_full().flex().justify_center().px(m.gutter).pt(px(48.)).pb(px(24.)).child(
                    div().w_full().max_w(m.column).h_full().child(page),
                ),
            )
            .children(self.render_sheet(cx))
            .child(
                div().w_full().flex().justify_center().px(m.gutter).pb(px(12.)).child(
                    div()
                        .w_full()
                        .max_w(m.column)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(self.render_message())
                        .child(self.render_asks(cx)),
                ),
            )
            .child(self.render_footer())
    }
}

fn bar_row() -> Div {
    div().flex().items_center().gap(px(10.))
}

pub(crate) fn ghost_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(6.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(color::MUTED))
        .cursor_pointer()
        .child(label)
}

pub(crate) fn primary_button(id: &'static str, label: &'static str, m: Metrics) -> Stateful<Div> {
    let height = if m.control > px(32.) { m.control } else { px(32.) };
    div()
        .id(id)
        .h(height)
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .bg(rgb(color::INK))
        .text_color(rgb(color::SURFACE))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .child(label)
}

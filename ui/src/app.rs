use crate::feedback::FeedbackScreen;
use crate::keys::KeyScreen;
use crate::theme::{Colors, Theme};
use crate::{FormFactor, Metrics, SANS};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{
    AiEvent, Ask, Build, BuildNumber, Cancel, ChangeId, Desk, Face, KeyStore, Paths, Provider, Request, Revision, Secret, Settings,
};
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
    /// All feedback state. The views render it and keep no copy.
    pub(crate) feedback: Desk,
    /// Whether a feedback link is saved, asked at launch and after a link save, never on a frame.
    /// False hides every way to feedback but the key screen's link field.
    pub(crate) feedback_on: bool,
    /// Lives here, not on the screen, so the text survives closing the screen and a failed send.
    pub(crate) feedback_field: Entity<TextareaState>,
    pub(crate) feedback_screen: Option<FeedbackScreen>,
    /// The send on the background executor. Replaced, never cancelled: a POST that has left
    /// can't be called back, and the webhook dedupes on the id.
    pub(crate) sending: Task<()>,
    /// The build on disk, as the Mac host last reported it.
    pub(crate) installed: Option<BuildNumber>,
    pub(crate) saved: Saved,
    save: Task<()>,
    /// The body faces this device has, asked of the text system once at launch.
    pub(crate) faces: Vec<Face>,
    pub(crate) focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl Inkling {
    /// `build` is what the host is running: `Build::running()`, or the Mac snap's next build.
    pub fn new(
        form: FormFactor,
        paths: Paths,
        keys: Box<dyn KeyStore>,
        build: Build,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = Settings::load(&paths);
        let draft = paths.load_draft();
        let page = cx.new(|cx| {
            let mut page = TextareaState::new(window, cx)
                .soft_wrap(true)
                .autocorrect(true)
                .autocapitalize(Autocapitalize::Sentences);
            if form == FormFactor::Phone {
                page = page.placeholder("Start writing");
            }
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
        if form == FormFactor::Mac {
            page.read(cx).focus_handle(cx).focus(window, cx);
        }
        let key_saved = keys.has(Secret::ApiKey(settings.provider));
        let feedback = Desk::open(paths.clone(), build);
        let feedback_on = Desk::has_link(keys.as_ref());
        let feedback_field = cx.new(|cx| {
            TextareaState::new(window, cx)
                .soft_wrap(true)
                .auto_grow(3, 6)
                .autocorrect(true)
                .autocapitalize(Autocapitalize::Sentences)
                .placeholder("Say what you'd like Inkling to do differently")
        });
        let installed = cx.text_system().all_font_names();
        let faces: Vec<Face> = Face::ALL.into_iter().filter(|f| installed.iter().any(|n| n == f.family())).collect();
        let feedback_edited = cx.subscribe_in(&feedback_field, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.feedback.edited();
                cx.notify();
            }
        });
        let inkling = Inkling {
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
            feedback,
            feedback_on,
            feedback_field,
            feedback_screen: None,
            sending: Task::ready(()),
            installed: None,
            saved: Saved::Saved,
            save: Task::ready(()),
            focus: cx.focus_handle(),
            faces,
            _subs: vec![edited, sent, intercept, feedback_edited],
        };
        inkling.theme().apply_to_kit(cx);
        inkling
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

    /// Whether a feedback link is saved, so the ways to feedback show.
    pub fn feedback_on(&self) -> bool {
        self.feedback_on
    }

    /// What the key screen's last Save link did.
    pub fn link_status(&self) -> Option<&Result<(), String>> {
        self.key_screen.as_ref()?.link_status.as_ref()
    }

    pub fn feedback_open(&self) -> bool {
        self.feedback_screen.is_some()
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
        if self.feedback_screen.is_some() && key.key == "enter" && key.modifiers.platform {
            self.send_feedback(window, cx);
            return true;
        }
        if key.modifiers.modified() {
            return false;
        }
        match key.key.as_str() {
            "escape" => self.escape(window, cx),
            "tab" | "enter" if self.revision.review().is_some() && self.key_screen.is_none() && self.feedback_screen.is_none() => {
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
        if self.feedback_screen.is_some() {
            self.close_feedback(window, cx);
        } else if self.key_screen.is_some() {
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
        window.dismiss_virtual_keyboard();
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
        self.select_next_from(id);
    }

    pub fn reject(&mut self, id: ChangeId, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.reject(id));
        self.select_next_from(id);
    }

    /// iPhone: after a decision the sheet moves on to the next change still pending.
    fn select_next_from(&mut self, id: ChangeId) {
        if self.form != FormFactor::Phone {
            return;
        }
        if let Some(review) = self.revision.review() {
            self.selected = review.next_pending(Some(id)).or(Some(id));
        }
    }

    pub fn undo(&mut self, id: ChangeId, window: &mut Window, cx: &mut Context<Self>) {
        self.decide(window, cx, |review| review.undo(id));
    }

    pub fn choose(&mut self, id: ChangeId, wording: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.wordings_open = None;
        self.decide(window, cx, |review| review.choose(id, wording));
        self.select_next_from(id);
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
        self.focus_page(window, cx);
        cx.notify();
    }

    /// On Mac the page takes focus back. On iPhone the page waits for a tap, which also
    /// raises the keyboard.
    pub(crate) fn focus_page(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form == FormFactor::Mac {
            self.page.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    /// iPhone: puts the keyboard away and leaves no field focused.
    pub fn done_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        cx.notify();
    }

    fn typing(&self, window: &Window, cx: &App) -> bool {
        self.page.read(cx).focus_handle(cx).is_focused(window) || self.ask_field.read(cx).focus_handle(cx).is_focused(window)
    }

    pub fn open_keys(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        self.feedback_screen = None;
        let screen = KeyScreen::new(provider, self, window, cx);
        screen.key.read(cx).focus_handle(cx).focus(window, cx);
        self.key_screen = Some(screen);
        cx.notify();
    }

    pub fn close_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_screen = None;
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        self.key_saved = self.keys.has(Secret::ApiKey(self.settings.provider));
        if self.revision.review().is_none() {
            self.focus_page(window, cx);
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

    pub(crate) fn theme(&self) -> Theme {
        Theme::new(self.form, &self.settings.appearance, &self.faces)
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
        let c = self.theme().c;
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
                    .child(div().flex_grow(1.).text_color(c.muted).child(progress))
                    .child(ghost_button(&c, "cancel", "Cancel").h(m.control.max(px(32.))).on_click(cx.listener(|this, _, _, cx| this.cancel(cx))).test_support())
                    .into_any_element()
            }
            Revision::Reviewing(review) => {
                let s = review.stats();
                let name = div().font_weight(FontWeight::SEMIBOLD).child(format!("Version {}", self.version.number));
                let numbers = div()
                    .flex()
                    .gap_1()
                    .text_color(c.muted)
                    .when(self.form == FormFactor::Mac, |numbers| numbers.flex_wrap().child(format!("from {source} ·")))
                    .when(self.form == FormFactor::Phone, |numbers| numbers.whitespace_nowrap().child(format!("{source} ·")))
                    .child(div().text_color(c.added_count).child(format!("+{}", s.words_added)))
                    .child(div().text_color(c.removed_line).child(format!("−{}", s.words_removed)))
                    .child(format!("· {} of {} accepted", s.accepted, s.total));
                let label = match self.form {
                    FormFactor::Mac => div().flex_grow(1.).flex().items_center().gap(px(10.)).child(name).child(numbers),
                    FormFactor::Phone => div()
                        .flex_grow(1.)
                        .min_w_0()
                        .pl_2()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(name.text_size(px(15.)))
                        .child(numbers.text_size(px(13.))),
                };
                bar_row()
                    .child(label)
                    .when(self.form == FormFactor::Mac, |row| {
                        row.child(
                            ghost_button(&c, "back", "Back to draft")
                                .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx)))
                                .test_support(),
                        )
                    })
                    .child(
                        primary_button(&c, "accept-rest", "Accept the rest", m)
                            .flex_none()
                            .on_click(cx.listener(|this, _, window, cx| this.accept_rest(window, cx)))
                            .test_support(),
                    )
                    .into_any_element()
            }
            _ => return None,
        };
        if self.form == FormFactor::Phone {
            return Some(div().w_full().px(m.gutter).pt_2().pb_1().text_size(px(14.)).child(content).into_any_element());
        }
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
                        .max_w(self.theme().body.column)
                        .pl(px(14.))
                        .pr(px(6.))
                        .py(px(6.))
                        .bg(c.surface)
                        .border_1()
                        .border_color(c.hairline)
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
        let t = self.theme();
        Textarea::new(&self.page)
            .appearance(false)
            .h_full()
            .font_family(t.body.face)
            .text_size(t.body.size)
            .line_height(relative(t.body.line))
            .text_color(t.c.ink)
            .into_any_element()
    }

    fn render_message(&self) -> Option<impl IntoElement + use<>> {
        let c = self.theme().c;
        let line: SharedString = match (&self.revision, &self.notice) {
            (Revision::Failed(message), _) => message.clone().into(),
            (_, Some(notice)) => notice.clone(),
            _ => return None,
        };
        Some(div().id("message").text_size(px(13.)).text_color(c.muted).truncate().child(line))
    }

    fn render_asks(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let phone = self.form == FormFactor::Phone;
        let chip = if phone { m.control } else { px(30.) };
        let quick = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            div()
                .id(id)
                .flex_none()
                .h(chip)
                .map(|chip| if phone { chip.px(px(14.)) } else { chip.px_3() })
                .flex()
                .items_center()
                .bg(c.surface)
                .border_1()
                .border_color(c.hairline)
                .rounded(chip / 2.)
                .text_size(if phone { px(14.) } else { px(12.) })
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
                    .id("quick-actions")
                    .flex()
                    .gap(px(6.))
                    .overflow_x_scroll()
                    .child(quick("fix-grammar", "Fix grammar", Self::fix_grammar))
                    .child(quick("find-word", "Find a word", Self::find_word))
                    .child(quick("next-line", "Next line", Self::next_line)),
            )
            .when(!(phone && self.revision.review().is_some()), |asks| asks.child(
                div()
                    .h(if phone { m.control + px(8.) } else { px(46.) })
                    .pl(px(14.))
                    .pr(if phone { px(4.) } else { px(6.) })
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .bg(c.surface)
                    .border_1()
                    .border_color(c.hairline)
                    .rounded(px(12.))
                    .child(div().flex_1().min_w_0().text_size(px(14.)).child(Input::new(&self.ask_field).id("ask").appearance(false)))
                    .child(primary_button(&c, "send", "Send", m).on_click(cx.listener(|this, _, window, cx| this.send(window, cx))).test_support()),
            ))
    }

    /// Left to right: whether the page is saved, then a newer build's Restart, Feedback once a
    /// link is saved, and the provider, which opens the key screen like Cmd-Comma.
    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
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
            .items_center()
            .justify_between()
            .gap_3()
            .px_6()
            .pt_2()
            .pb(px(14.))
            .text_size(px(12.))
            .text_color(c.muted)
            .child(div().flex_1().min_w_0().truncate().child(saved))
            .children(self.render_restart(cx))
            .when(self.feedback_on, |footer| {
                footer.child(
                    footer_button(&c, "open-feedback", "Feedback")
                        .on_click(cx.listener(|this, _, window, cx| this.open_feedback(window, cx)))
                        .test_support(),
                )
            })
            .child(
                footer_button(&c, "open-keys", key)
                    .on_click(cx.listener(|this, _, window, cx| this.on_open_keys(&OpenKeys, window, cx)))
                    .test_support(),
            )
    }
}

impl Render for Inkling {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.theme().c;
        let m = self.metrics();
        let root = div()
            .id("inkling")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(c.ground)
            .text_color(c.ink)
            .font_family(SANS)
            .on_action(cx.listener(Self::on_open_keys));
        if let Some(screen) = &self.key_screen {
            return root.child(self.render_keys(screen, cx));
        }
        if self.feedback_screen.is_some() {
            return root.child(self.render_feedback(window, cx));
        }
        if self.form == FormFactor::Phone {
            return root.child(self.render_phone(window, cx));
        }
        let page = self.render_page(window, cx);
        root.child(self.render_bar(cx))
            .child(
                div().flex_1().min_h_0().w_full().flex().justify_center().px(m.gutter).pt(px(48.)).pb(px(24.)).child(
                    div().w_full().max_w(self.theme().body.column).h_full().child(page),
                ),
            )
            .children(self.render_sheet(cx))
            .child(
                div().w_full().flex().justify_center().px(m.gutter).pb(px(12.)).child(
                    div()
                        .w_full()
                        .max_w(self.theme().body.column)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(self.render_message())
                        .child(self.render_asks(cx)),
                ),
            )
            .child(self.render_footer(cx))
    }
}

impl Inkling {
    /// iPhone, top to bottom: the version bar or a status row, the page, the sheet for the
    /// tapped change, then the quick actions and the ask field, which the host keeps above the
    /// keyboard.
    fn render_phone(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = self.theme().c;
        let m = self.metrics();
        let top = match self.render_version_bar(cx) {
            Some(bar) => bar,
            None => self.render_status_row(window, cx).into_any_element(),
        };
        let reviewing = self.revision.review().is_some();
        let page = self.render_page(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(top)
            .child(div().flex_1().min_h_0().w_full().px(px(14.)).pt_2().child(page))
            .children(self.render_sheet(cx))
            .child(
                div()
                    .w_full()
                    .px(m.gutter)
                    .pt_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(reviewing, |bottom| bottom.bg(c.surface))
                    .children(self.render_message())
                    .child(self.render_asks(cx)),
            )
            .into_any_element()
    }

    /// iPhone, outside a review: whether the page is saved, the way to the key screen, and
    /// Done while a field has the keyboard.
    fn render_status_row(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let saved = match &self.saved {
            Saved::Saved => "Saved".to_string(),
            Saved::Pending => "Editing".to_string(),
            Saved::Failed(e) => format!("Couldn't save ({e})"),
        };
        let provider = self.settings.provider.label();
        let key = if self.key_saved { provider.to_string() } else { format!("{provider} · add a key") };
        div()
            .w_full()
            .h(m.control)
            .pl(px(24.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(14.))
            .text_color(c.muted)
            .child(div().flex_1().min_w_0().truncate().child(saved))
            .when(self.feedback_on, |row| {
                row.child(
                    ghost_button(&c, "open-feedback", "Feedback")
                        .h(m.control)
                        .text_color(c.ink)
                        .on_click(cx.listener(|this, _, window, cx| this.open_feedback(window, cx)))
                        .test_support(),
                )
            })
            .child(
                ghost_button(&c, "open-keys", "")
                    .h(m.control)
                    .child(key)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let provider = this.settings.provider;
                        this.open_keys(provider, window, cx);
                    }))
                    .test_support(),
            )
            .when(self.typing(window, cx), |row| {
                row.child(
                    ghost_button(&c, "done", "Done")
                        .h(m.control)
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(c.ink)
                        .on_click(cx.listener(|this, _, window, cx| this.done_typing(window, cx)))
                        .test_support(),
                )
            })
    }
}

fn bar_row() -> Div {
    div().flex().items_center().gap(px(10.))
}

pub(crate) fn ghost_button(c: &Colors, id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(6.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c.muted)
        .cursor_pointer()
        .child(label)
}

/// A Mac footer action: a small outlined pill, so it reads as a button beside the footer's text.
pub(crate) fn footer_button(c: &Colors, id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .bg(c.surface)
        .border_1()
        .border_color(c.hairline)
        .rounded(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c.ink)
        .cursor_pointer()
        .child(label.into())
}

pub(crate) fn primary_button(c: &Colors, id: &'static str, label: &'static str, m: Metrics) -> Stateful<Div> {
    let height = if m.control > px(32.) { m.control } else { px(32.) };
    div()
        .id(id)
        .h(height)
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .bg(c.ink)
        .text_color(c.surface)
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .child(label)
}

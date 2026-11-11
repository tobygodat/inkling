use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::*;
use inkling_core::{Action as AiAction, AiEvent, AiRequest, Assist, Cancel, KeyStore, Paths, Provider, Settings};
use std::ops::Range;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(feature = "snap")]
mod snap;

gpui_kit::actions!(inkling, [ContinueWriting, OpenRewrite, OpenSettings]);

const GROUND: u32 = 0xF4F5F1;
const INK: u32 = 0x1C211E;
const MUTED: u32 = 0x7C857E;
const ACCENT: u32 = 0x2F6B4F;
const HAIRLINE: u32 = 0xDDE0DA;
const SERIF: &str = "Iowan Old Style";
const BODY: f32 = 18.;
const LINE: f32 = 1.6;
/// The kit's editor reserves this much on the right of the wrap width (`RIGHT_MARGIN`).
const KIT_RIGHT_MARGIN: f32 = 10.;

struct Request {
    cancel: Cancel,
    rx: mpsc::Receiver<AiEvent>,
    _pump: Task<()>,
}

struct SettingsForm {
    provider: Provider,
    key: Entity<InputState>,
    model: Entity<InputState>,
}

enum Panel {
    None,
    Bar { input: Entity<InputState>, range: Range<usize> },
    Settings(SettingsForm),
}

pub struct Inkling {
    paths: Paths,
    settings: Settings,
    keys: Box<dyn KeyStore>,
    page: Entity<TextareaState>,
    assist: Assist,
    request: Option<Request>,
    panel: Panel,
    save: Task<()>,
    _subs: Vec<Subscription>,
}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-j", ContinueWriting, None),
        KeyBinding::new("cmd-k", OpenRewrite, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
    ]);
}

pub fn window_options(focus: bool, show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Default::default(), size: size(px(900.), px(720.)) })),
        focus,
        show,
        ..Default::default()
    }
}

impl Inkling {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let paths = Paths::mac();
        let settings = Settings::load(&paths);
        let draft = paths.load_draft();
        let page = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx).soft_wrap(true);
            s.set_value(draft, window, cx);
            s
        });
        let changed = cx.subscribe_in(&page, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                if this.assist.proposal().is_some() {
                    this.dismiss(cx);
                }
                this.schedule_save(cx);
            }
        });
        let view = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let handled = view.update(cx, |this, cx| this.intercept(&event.keystroke, window, cx)).unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        page.read(cx).focus_handle(cx).focus(window, cx);
        Inkling {
            paths,
            settings,
            keys: inkling_core::default_keys(),
            page,
            assist: Assist::Idle,
            request: None,
            panel: Panel::None,
            save: Task::ready(()),
            _subs: vec![changed, keys],
        }
    }

    fn text(&self, cx: &App) -> String {
        self.page.read(cx).value().to_string()
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |this, cx| {
                let _ = this.paths.save_draft(&this.text(cx));
            });
        });
    }

    /// Keys a proposal, the command bar, settings or a failure line own. Runs before any binding,
    /// so the editor's own Tab (indent) and Escape never see them.
    fn intercept(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if key.modifiers.modified() {
            return false;
        }
        let k = key.key.as_str();
        if let Some(p) = self.assist.proposal() {
            let rewrite = !p.replace.is_empty();
            return match k {
                "escape" => {
                    self.dismiss(cx);
                    true
                }
                "tab" => {
                    self.accept(window, cx);
                    true
                }
                "enter" if rewrite => {
                    self.accept(window, cx);
                    true
                }
                _ => false,
            };
        }
        if k == "escape" {
            if matches!(self.assist, Assist::Failed(_)) {
                self.assist.dismiss();
                cx.notify();
                return true;
            }
            if !matches!(self.panel, Panel::None) {
                self.close_panel(window, cx);
                return true;
            }
        }
        false
    }

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::None;
        self.page.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.request.take() {
            r.cancel.cancel();
        }
        self.assist.dismiss();
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.assist.proposal().cloned() else { return };
        let mut doc = self.text(cx);
        if self.assist.accept(&mut doc).is_some() {
            self.request = None;
            self.page.update(cx, |s, cx| {
                s.set_selected_range(p.replace.clone(), cx);
                s.replace(p.text.clone(), window, cx);
            });
            self.schedule_save(cx);
        }
        cx.notify();
    }

    fn ask(&mut self, action: AiAction, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(cx);
        let backend = match inkling_core::backend_for(&self.settings, self.keys.as_ref()) {
            Ok(b) => b,
            Err(provider) => return self.open_settings(provider, window, cx),
        };
        let doc = self.text(cx);
        let request = AiRequest {
            action,
            before: doc[..range.start].to_string(),
            selection: doc[range.clone()].to_string(),
            after: doc[range.end..].to_string(),
        };
        self.assist = Assist::begin(range);
        let (tx, rx) = mpsc::channel();
        let cancel = inkling_core::start(request, backend, move |e| {
            let _ = tx.send(e);
        });
        let pump = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            let busy = this.update(cx, |this, cx| this.drain(cx)).unwrap_or(false);
            if !busy {
                break;
            }
        });
        self.request = Some(Request { cancel, rx, _pump: pump });
        cx.notify();
    }

    fn drain(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(r) = &self.request else { return false };
        while let Ok(event) = r.rx.try_recv() {
            self.assist.apply(event);
        }
        cx.notify();
        self.assist.is_busy()
    }

    fn on_continue(&mut self, _: &ContinueWriting, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.page.read(cx).cursor();
        self.ask(AiAction::Continue, at..at, window, cx);
    }

    fn on_rewrite(&mut self, _: &OpenRewrite, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.page.read(cx).selected_range();
        if range.is_empty() {
            return self.on_continue(&ContinueWriting, window, cx);
        }
        self.dismiss(cx);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Tell Inkling what to change"));
        let sub = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::PressEnter { .. }) {
                return;
            }
            let instruction = input.read(cx).value().trim().to_string();
            let Panel::Bar { range, .. } = &this.panel else { return };
            if instruction.is_empty() {
                return;
            }
            let range = range.clone();
            this.close_panel(window, cx);
            this.ask(AiAction::Rewrite(instruction), range, window, cx);
        });
        self._subs.push(sub);
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.panel = Panel::Bar { input, range };
        cx.notify();
    }

    fn on_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.settings.provider;
        self.open_settings(provider, window, cx);
    }

    fn open_settings(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("API key"));
        let model = cx.new(|cx| InputState::new(window, cx));
        let mut form = SettingsForm { provider, key, model };
        self.load_form(&mut form, window, cx);
        form.key.read(cx).focus_handle(cx).focus(window, cx);
        self.panel = Panel::Settings(form);
        cx.notify();
    }

    fn load_form(&self, form: &mut SettingsForm, window: &mut Window, cx: &mut Context<Self>) {
        let p = form.provider;
        let key = self.keys.get(p).unwrap_or_default();
        let model = self.settings.model(p);
        let model = if model == p.default_model() { String::new() } else { model.to_string() };
        form.key.update(cx, |s, cx| s.set_value(key, window, cx));
        form.model.update(cx, |s, cx| {
            s.set_value(model, window, cx);
            s.set_placeholder(p.default_model(), window, cx);
        });
    }

    fn pick_provider(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Panel::Settings(mut form) = std::mem::replace(&mut self.panel, Panel::None) else { return };
        form.provider = Provider::ALL[ix];
        self.load_form(&mut form, window, cx);
        self.panel = Panel::Settings(form);
        cx.notify();
    }

    fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Panel::Settings(form) = &self.panel else { return };
        let p = form.provider;
        let key = form.key.read(cx).value().trim().to_string();
        let model = form.model.read(cx).value().to_string();
        let result = if key.is_empty() { self.keys.remove(p) } else { self.keys.set(p, &key) };
        self.settings.provider = p;
        self.settings.set_model(p, &model);
        let saved = self.settings.save(&self.paths).map_err(|e| e.to_string());
        match result.and(saved) {
            Ok(()) => self.close_panel(window, cx),
            Err(message) => self.assist = Assist::Failed(message),
        }
        cx.notify();
    }

    /// The proposal drawn as one wrapped paragraph over the editor's own text box: the document
    /// before it, then ghost text or the dimmed selection and its replacement, then the rest.
    fn render_proposal(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let p = self.assist.proposal()?;
        let state = self.page.read(cx);
        let frame = state.text_bounds()?;
        let origin = state.range_to_bounds(&(0..0))?.origin;
        let doc = state.value();
        let r = p.replace.clone();
        let mut text = String::with_capacity(doc.len() + p.text.len());
        text.push_str(&doc[..r.end]);
        let ghost = text.len()..text.len() + p.text.len();
        text.push_str(&p.text);
        text.push_str(&doc[r.end..]);
        let style = |c: u32| HighlightStyle { color: Some(rgb(c).into()), ..Default::default() };
        let mut highlights = vec![];
        if !r.is_empty() {
            highlights.push((r.clone(), style(MUTED)));
            highlights.push((ghost, style(ACCENT)));
        } else {
            highlights.push((ghost, style(MUTED)));
        }
        let left = origin.x - frame.origin.x;
        Some(
            div()
                .absolute()
                .left(frame.origin.x)
                .top(frame.origin.y)
                .w(frame.size.width)
                .h(frame.size.height)
                .overflow_hidden()
                .bg(rgb(GROUND))
                .child(
                    div()
                        .absolute()
                        .left(left)
                        .top(origin.y - frame.origin.y)
                        .w(frame.size.width - left - px(KIT_RIGHT_MARGIN))
                        .font_family(SERIF)
                        .text_size(px(BODY))
                        .line_height(relative(LINE))
                        .text_color(rgb(INK))
                        .child(StyledText::new(text).with_highlights(highlights)),
                ),
        )
    }

    fn render_settings(&self, form: &SettingsForm, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = Provider::ALL.iter().position(|p| *p == form.provider);
        div().absolute().inset_0().flex().items_center().justify_center().bg(rgba(0x1C211E22)).child(
            div()
                .id("settings")
                .w(px(420.))
                .p_5()
                .gap_3()
                .flex()
                .flex_col()
                .bg(rgb(GROUND))
                .border_1()
                .border_color(rgb(HAIRLINE))
                .rounded_lg()
                .text_size(px(13.))
                .child(div().text_color(rgb(INK)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                .child(
                    RadioGroup::horizontal("provider")
                        .children(Provider::ALL.iter().map(|p| Radio::new(p.id()).label(p.label())))
                        .selected_index(selected)
                        .on_click(cx.listener(|this, ix: &usize, window, cx| this.pick_provider(*ix, window, cx))),
                )
                .child(div().text_color(rgb(MUTED)).child(format!("{} key", form.provider.label())))
                .child(Input::new(&form.key).id("key").mask_toggle())
                .child(div().text_color(rgb(MUTED)).child("Model"))
                .child(Input::new(&form.model).id("model"))
                .child(
                    div().flex().justify_end().child(
                        Button::new("save")
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|this, _, window, cx| this.save_settings(window, cx))),
                    ),
                ),
        )
    }
}

impl Render for Inkling {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let words = self.text(cx).split_whitespace().count();
        let bottom = match (&self.panel, &self.assist) {
            (Panel::Bar { input, .. }, _) => Some(
                div()
                    .w_full()
                    .max_w(px(680.))
                    .px_4()
                    .pb_2()
                    .child(Input::new(input).id("command-bar"))
                    .into_any_element(),
            ),
            (_, Assist::Failed(message)) => Some(
                div()
                    .w_full()
                    .max_w(px(680.))
                    .px_4()
                    .pb_1()
                    .text_size(px(13.))
                    .text_color(rgb(MUTED))
                    .truncate()
                    .child(message.clone())
                    .into_any_element(),
            ),
            _ => None,
        };
        let settings = match &self.panel {
            Panel::Settings(form) => Some(self.render_settings(form, cx)),
            _ => None,
        };
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgb(GROUND))
            .text_color(rgb(INK))
            .on_action(cx.listener(Self::on_continue))
            .on_action(cx.listener(Self::on_rewrite))
            .on_action(cx.listener(Self::on_settings))
            .child(
                div().flex_1().min_h_0().w_full().max_w(px(680.)).pt_10().child(
                    Textarea::new(&self.page)
                        .appearance(false)
                        .h_full()
                        .font_family(SERIF)
                        .text_size(px(BODY))
                        .line_height(relative(LINE))
                        .text_color(rgb(INK)),
                ),
            )
            .children(bottom)
            .child(
                div()
                    .w_full()
                    .max_w(px(680.))
                    .px_4()
                    .py_2()
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    .text_color(rgb(MUTED))
                    .child(format!("{words} {}", if words == 1 { "word" } else { "words" }))
                    .child(self.settings.provider.label()),
            )
            .children(self.render_proposal(cx))
            .children(settings)
    }
}

fn main() {
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap").nth(1) {
        return snap::run(std::path::Path::new(&dir));
    }
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        init(cx);
        gpui_kit::open_window(window_options(true, true), cx, |window, cx| cx.new(|cx| Inkling::new(window, cx)))
            .expect("open the window");
        cx.activate(true);
    });
}

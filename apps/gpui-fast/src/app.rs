use crate::editor::{self, Editor, EditorEvent, Look, Overlay};
use gpui::{
    App, Context, Entity, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla,
    KeyBinding, ScrollHandle, Subscription, Task, Window, actions, div, prelude::*, px, rgb,
};
use inkling_core::{Action, AiRequest, Assist, Cancel, KeyStore, Paths, Provider, Settings, backend_for};
use std::time::Duration;

actions!(inkling, [Continue, Rewrite, OpenSettings, Accept, Dismiss, Quit]);

const GROUND: u32 = 0xF4F5F1;
const INK: u32 = 0x1C211E;
const MUTED: u32 = 0x7C857E;
const ACCENT: u32 = 0x2F6B4F;
const HAIRLINE: u32 = 0xDDE0DA;

pub fn init(cx: &mut App) {
    editor::bind_keys(cx);
    let c = Some("Inkling");
    cx.bind_keys([
        KeyBinding::new("cmd-j", Continue, c),
        KeyBinding::new("cmd-k", Rewrite, c),
        KeyBinding::new("cmd-,", OpenSettings, c),
        KeyBinding::new("tab", Accept, c),
        KeyBinding::new("escape", Dismiss, c),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn look(serif: bool) -> Look {
    let (family, fallback, size, line_height) = if serif {
        ("Iowan Old Style", "Georgia", px(18.), px(18. * 1.6))
    } else {
        (".SystemUIFont", "Helvetica", px(13.), px(20.))
    };
    let accent = color(ACCENT);
    Look {
        font: Font {
            family: family.into(),
            features: FontFeatures::default(),
            fallbacks: Some(FontFallbacks::from_fonts(vec![fallback.into()])),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        },
        size,
        line_height,
        ink: color(INK),
        muted: color(MUTED),
        accent,
        selection: Hsla { a: 0.18, ..accent },
    }
}

pub struct Form {
    pub provider: Provider,
    pub missing: bool,
}

pub struct Inkling {
    paths: Paths,
    pub settings: Settings,
    keys: Box<dyn KeyStore>,
    pub doc: Entity<Editor>,
    pub bar: Entity<Editor>,
    pub key_field: Entity<Editor>,
    pub model_field: Entity<Editor>,
    pub assist: Assist,
    cancel: Option<Cancel>,
    ai_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    pub bar_open: bool,
    pub form: Option<Form>,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl Inkling {
    pub fn new(paths: Paths, keys: Box<dyn KeyStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let doc = cx.new(|cx| Editor::new(paths.load_draft(), look(true), cx));
        let bar = cx.new(|cx| Editor::new(String::new(), look(false), cx).single_line("Tell Inkling what to change"));
        let key_field = cx.new(|cx| Editor::new(String::new(), look(false), cx).single_line("Paste your API key").masked());
        let model_field = cx.new(|cx| Editor::new(String::new(), look(false), cx).single_line(""));
        let mut subs = vec![
            cx.subscribe_in(&doc, window, |this, _, e, _, cx| {
                if let EditorEvent::Edited = e {
                    if this.assist.proposal().is_some() {
                        this.stop_ai();
                    }
                    this.schedule_save(cx);
                }
            }),
            cx.subscribe_in(&bar, window, |this, _, e, window, cx| {
                if let EditorEvent::Submit = e {
                    this.submit_rewrite(window, cx);
                }
            }),
            cx.subscribe_in(&key_field, window, |this, _, e, window, cx| {
                if let EditorEvent::Submit = e {
                    this.save_settings(window, cx);
                }
            }),
            cx.subscribe_in(&model_field, window, |this, _, e, window, cx| {
                if let EditorEvent::Submit = e {
                    this.save_settings(window, cx);
                }
            }),
        ];
        for e in [&doc, &bar, &key_field, &model_field] {
            subs.push(cx.observe(e, |_, _, cx| cx.notify()));
        }
        focus(window, &doc, cx);
        Inkling {
            settings: Settings::load(&paths),
            paths,
            keys,
            doc,
            bar,
            key_field,
            model_field,
            assist: Assist::Idle,
            cancel: None,
            ai_task: None,
            save_task: None,
            bar_open: false,
            form: None,
            scroll: ScrollHandle::new(),
            _subs: subs,
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = this.paths.save_draft(&this.doc.read(cx).text) {
                    eprintln!("autosave failed: {e}");
                }
            });
        }));
    }

    fn stop_ai(&mut self) {
        if let Some(c) = self.cancel.take() {
            c.cancel();
        }
        self.ai_task = None;
        self.assist.dismiss();
    }

    pub fn start(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_ai();
        let backend = match backend_for(&self.settings, self.keys.as_ref()) {
            Ok(b) => b,
            Err(provider) => return self.open_settings(provider, true, window, cx),
        };
        let (text, sel) = {
            let d = self.doc.read(cx);
            (d.text.clone(), d.selected.clone())
        };
        let replace = match action {
            Action::Continue => sel.end..sel.end,
            Action::Rewrite(_) => sel.clone(),
        };
        self.doc.update(cx, |d, cx| d.select(sel.end..sel.end, cx));
        let request = AiRequest {
            action,
            before: text[..replace.start].into(),
            selection: text[replace.clone()].into(),
            after: text[replace.end..].into(),
        };
        self.assist = Assist::begin(replace);
        let (tx, rx) = std::sync::mpsc::channel();
        self.cancel = Some(inkling_core::start(request, backend, move |e| {
            let _ = tx.send(e);
        }));
        // Drained once a frame on the main thread: GPUI's test scheduler forbids wakes from
        // other threads, and this keeps the app and the headless proof on one code path.
        self.ai_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let events: Vec<_> = rx.try_iter().collect();
                let busy = this.update(cx, |this, cx| {
                    for event in events {
                        this.assist.apply(event);
                    }
                    cx.notify();
                    this.assist.is_busy()
                });
                if !matches!(busy, Ok(true)) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn on_continue(&mut self, _: &Continue, window: &mut Window, cx: &mut Context<Self>) {
        self.start(Action::Continue, window, cx);
    }

    pub fn on_rewrite(&mut self, _: &Rewrite, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.read(cx).selected.is_empty() {
            return self.start(Action::Continue, window, cx);
        }
        self.bar_open = true;
        self.bar.update(cx, |b, cx| b.set_text(String::new(), cx));
        focus(window, &self.bar, cx);
        cx.notify();
    }

    fn submit_rewrite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let instruction = self.bar.read(cx).text.trim().to_string();
        if instruction.is_empty() {
            return;
        }
        self.bar_open = false;
        focus(window, &self.doc, cx);
        self.start(Action::Rewrite(instruction), window, cx);
    }

    pub fn accept(&mut self, _: &Accept, _: &mut Window, cx: &mut Context<Self>) {
        let assist = &mut self.assist;
        if matches!(assist, Assist::Ready(_)) {
            self.doc.update(cx, |d, cx| d.edit_with(cx, |text| assist.accept(text)));
            cx.notify();
        }
    }

    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.take().is_some() || std::mem::take(&mut self.bar_open) {
            focus(window, &self.doc, cx);
        } else {
            self.stop_ai();
        }
        cx.notify();
    }

    fn open_settings_action(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(self.settings.provider, false, window, cx);
    }

    pub fn open_settings(&mut self, provider: Provider, missing: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.form = Some(Form { provider, missing });
        self.bar_open = false;
        self.fill_form(provider, cx);
        focus(window, &self.key_field, cx);
        cx.notify();
    }

    fn fill_form(&mut self, p: Provider, cx: &mut Context<Self>) {
        let model = self.settings.model(p);
        let custom = if model == p.default_model() { String::new() } else { model.to_string() };
        self.model_field.update(cx, |f, cx| {
            f.placeholder = p.default_model().into();
            f.set_text(custom, cx);
        });
        let saved = self.keys.get(p).is_some();
        self.key_field.update(cx, |f, cx| {
            f.placeholder = if saved { "Saved. Paste a new key to replace it." } else { "Paste your API key" }.into();
            f.set_text(String::new(), cx);
        });
    }

    pub fn choose_provider(&mut self, p: Provider, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            form.provider = p;
            self.fill_form(p, cx);
            cx.notify();
        }
    }

    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.take() else { return };
        let p = form.provider;
        self.settings.provider = p;
        self.settings.set_model(p, &self.model_field.read(cx).text);
        let key = self.key_field.read(cx).text.trim().to_string();
        if !key.is_empty()
            && let Err(e) = self.keys.set(p, &key)
        {
            eprintln!("saving the key failed: {e}");
        }
        self.key_field.update(cx, |f, cx| f.set_text(String::new(), cx));
        if let Err(e) = self.settings.save(&self.paths) {
            eprintln!("saving settings failed: {e}");
        }
        focus(window, &self.doc, cx);
        cx.notify();
    }

    fn overlay(&self) -> Option<Overlay> {
        let p = self.assist.proposal()?;
        Some(if p.replace.is_empty() {
            Overlay { at: p.replace.start, text: p.text.clone().into(), color: color(MUTED), dim: 0..0 }
        } else {
            Overlay { at: p.replace.end, text: format!(" {}", p.text).into(), color: color(ACCENT), dim: p.replace.clone() }
        })
    }

    fn settings_card(&self, form: &Form, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field = |label: &'static str, editor: &Entity<Editor>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(color(MUTED)).child(label))
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(color(HAIRLINE))
                        .bg(rgb(0xFFFFFF))
                        .child(editor::view(editor, None, window, cx)),
                )
        };
        let choice = div().flex().gap_1().p_1().rounded_lg().bg(color(HAIRLINE)).children(Provider::ALL.map(|p| {
            let on = p == form.provider;
            div()
                .id(p.id())
                .flex_1()
                .py_1()
                .flex()
                .justify_center()
                .rounded_md()
                .when(on, |d| d.bg(rgb(0xFFFFFF)).text_color(color(ACCENT)))
                .when(!on, |d| d.text_color(color(INK)))
                .on_click(cx.listener(move |this, _, _, cx| this.choose_provider(p, cx)))
                .child(p.label())
        }));
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .when(primary, |d| d.bg(color(ACCENT)).text_color(rgb(0xFFFFFF)))
                .when(!primary, |d| d.border_1().border_color(color(HAIRLINE)))
                .child(label)
        };
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(Hsla { a: 0.35, ..color(GROUND) })
            .child(
                div()
                    .occlude()
                    .w(px(420.))
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_xl()
                    .bg(rgb(0xFBFBF9))
                    .border_1()
                    .border_color(color(HAIRLINE))
                    .shadow_lg()
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                    .when(form.missing, |d| {
                        d.child(div().text_color(color(ACCENT)).child(format!(
                            "Add a {} key to use Continue and Rewrite.",
                            form.provider.label()
                        )))
                    })
                    .child(choice)
                    .child(field("API key", &self.key_field))
                    .child(field("Model", &self.model_field))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("cancel", "Cancel", false).on_click(cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx))))
                            .child(button("save", "Save", true).on_click(cx.listener(|this, _, window, cx| this.save_settings(window, cx)))),
                    ),
            )
    }
}

impl Render for Inkling {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let words = self.doc.read(cx).text.split_whitespace().count();
        let failed = match &self.assist {
            Assist::Failed(m) => Some(m.clone()),
            _ => None,
        };
        let card = self.form.as_ref().map(|f| self.settings_card(f, window, cx).into_any_element());
        let doc_focus = self.doc.read(cx).focus.clone();
        div()
            .key_context("Inkling")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(color(GROUND))
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .text_color(color(INK))
            .on_action(cx.listener(Self::on_continue))
            .on_action(cx.listener(Self::on_rewrite))
            .on_action(cx.listener(Self::open_settings_action))
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::dismiss))
            .capture_action(cx.listener(|this, _: &editor::Newline, window, cx| {
                let rewrite_ready = matches!(&this.assist, Assist::Ready(p) if !p.replace.is_empty());
                if rewrite_ready && this.doc.read(cx).focus.is_focused(window) {
                    this.accept(&Accept, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .id("scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_click(move |_, window, cx| window.focus(&doc_focus, cx))
                    .child(
                        div().w_full().flex().justify_center().px_8().pt(px(72.)).pb(px(160.)).child(
                            div().w_full().max_w(px(680.)).child(editor::view(&self.doc, self.overlay(), window, cx)),
                        ),
                    ),
            )
            .when_some(failed, |d, m| d.child(div().px_6().pb_1().text_color(color(MUTED)).child(m)))
            .when(self.bar_open, |d| {
                d.child(
                    div()
                        .mx_6()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(color(HAIRLINE))
                        .bg(rgb(0xFFFFFF))
                        .child(editor::view(&self.bar, None, window, cx)),
                )
            })
            .child(
                div()
                    .px_6()
                    .py_2()
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    .text_color(color(MUTED))
                    .child(format!("{words} {}", if words == 1 { "word" } else { "words" }))
                    .child(self.settings.provider.label()),
            )
            .children(card)
    }
}

fn focus(window: &mut Window, editor: &Entity<Editor>, cx: &mut App) {
    let handle = editor.read(cx).focus.clone();
    window.focus(&handle, cx);
}

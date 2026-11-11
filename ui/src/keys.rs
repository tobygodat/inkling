//! The key screen: which provider writes with you, its key and its model. The key goes only to
//! the key store, and the screen says a key is saved without ever reading it back.

use crate::app::{Inkling, ghost_button, primary_button};
use crate::{FormFactor, SANS, SERIF, color};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::Provider;

pub(crate) struct KeyScreen {
    pub provider: Provider,
    pub key: Entity<InputState>,
    pub model: Entity<InputState>,
    /// Whether the store holds a key for `provider`.
    pub has_key: bool,
    /// What the last Save did.
    pub status: Option<Result<(), String>>,
}

impl KeyScreen {
    pub fn new(provider: Provider, app: &Inkling, window: &mut Window, cx: &mut Context<Inkling>) -> Self {
        let key = cx.new(|cx| InputState::new(window, cx).masked(true));
        let model = cx.new(|cx| InputState::new(window, cx));
        let mut screen = KeyScreen { provider, key, model, has_key: false, status: None };
        screen.show(provider, app, window, cx);
        screen
    }

    fn show(&mut self, provider: Provider, app: &Inkling, window: &mut Window, cx: &mut App) {
        self.provider = provider;
        self.has_key = app.keys.has(provider);
        self.status = None;
        self.clear_key(window, cx);
        let typed = app.settings.typed_model(provider).unwrap_or_default().to_string();
        self.model.update(cx, |field, cx| {
            field.set_value(typed, window, cx);
            field.set_placeholder(provider.default_model(), window, cx);
        });
    }
}

impl KeyScreen {
    /// Empties the key field. Its hint says whether a key is already saved.
    fn clear_key(&self, window: &mut Window, cx: &mut App) {
        let hint = if self.has_key { "Saved in the Keychain. Paste a new key to replace it." } else { "Paste your API key" };
        self.key.update(cx, |field, cx| {
            field.set_value("", window, cx);
            field.set_placeholder(hint, window, cx);
        });
    }
}

fn key_page(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "https://console.anthropic.com/settings/keys",
        Provider::Gemini => "https://aistudio.google.com/apikey",
        Provider::ChatGpt => "https://platform.openai.com/api-keys",
    }
}

impl Inkling {
    fn pick_provider(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut screen) = self.key_screen.take() else { return };
        screen.show(provider, self, window, cx);
        self.key_screen = Some(screen);
        cx.notify();
    }

    /// Writes the key through the key store and the provider and model through `Settings`.
    /// An empty key field keeps the key already saved.
    pub fn save_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = &self.key_screen else { return };
        let provider = screen.provider;
        let key = screen.key.read(cx).value().trim().to_string();
        let model = screen.model.read(cx).value().to_string();
        let stored = if key.is_empty() { Ok(()) } else { self.keys.set(provider, &key) };
        self.settings.provider = provider;
        self.settings.set_model(provider, &model);
        let written = self.settings.save(&self.paths).map_err(|e| format!("Couldn't save settings ({e})."));
        let status = stored.and(written);
        self.key_saved = self.keys.has(provider);
        if let Some(screen) = &mut self.key_screen {
            screen.has_key = self.key_saved;
            screen.status = Some(status);
            screen.clear_key(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn render_keys(&self, screen: &KeyScreen, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.metrics();
        let device = if self.form == FormFactor::Phone { "iPhone" } else { "Mac" };
        let label = |text: String| div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(rgb(color::MUTED)).child(text);
        let field = |input: &Entity<InputState>, id: &'static str, masked: bool| {
            let input = Input::new(input).id(id).appearance(false);
            let input = if masked { input.mask_toggle() } else { input };
            div()
                .h(px(48.))
                .px(px(14.))
                .flex()
                .items_center()
                .bg(rgb(color::SURFACE))
                .border_1()
                .border_color(rgb(color::HAIRLINE))
                .rounded(px(12.))
                .text_size(px(15.))
                .child(div().flex_1().child(input))
        };
        let tabs = div().flex().gap_1().p_1().bg(rgb(color::SEGMENT)).rounded(px(12.)).children(Provider::ALL.map(|p| {
            let on = p == screen.provider;
            div()
                .id(p.id())
                .flex_1()
                .h(m.control.max(px(40.)))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.))
                .text_size(px(14.))
                .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(rgb(if on { color::INK } else { color::MUTED }))
                .when(on, |tab| tab.bg(rgb(color::SURFACE)).border_1().border_color(rgb(color::HAIRLINE)))
                .cursor_pointer()
                .child(p.label())
                .on_click(cx.listener(move |this, _, window, cx| this.pick_provider(p, window, cx)))
                .test_support()
        }));
        let status = match &screen.status {
            Some(Err(message)) => Some(message.clone()),
            Some(Ok(())) if screen.has_key => Some(format!("Saved. Inkling will write with {}.", screen.provider.label())),
            Some(Ok(())) => Some("Saved. Add a key to start asking.".to_string()),
            None if screen.has_key => Some("Saved".to_string()),
            None => None,
        };
        let provider = screen.provider;
        div().id("keys").size_full().flex().justify_center().overflow_y_scroll().child(
            div()
                .w_full()
                .max_w(px(480.))
                .px(m.gutter)
                .pt_6()
                .pb_8()
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    div().flex().child(
                        ghost_button("back-to-page", "‹ Back to the page")
                            .on_click(cx.listener(|this, _, window, cx| this.close_keys(window, cx)))
                            .test_support(),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .font_family(SERIF)
                                .text_size(px(30.))
                                .line_height(relative(1.2))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Bring your own key"),
                        )
                        .child(div().text_size(px(15.)).line_height(relative(1.5)).text_color(rgb(color::MUTED)).child(format!(
                            "Inkling has no account and no server. It talks to the model you choose with your own key, kept in this {device}'s Keychain."
                        ))),
                )
                .child(div().flex().flex_col().gap_2().child(label("Writing partner".into())).child(tabs))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(label(format!("{} API key", provider.label())))
                                .children(status.map(|s| div().text_size(px(13.)).text_color(rgb(color::INK)).child(s))),
                        )
                        .child(field(&screen.key, "key", true))
                        .child(
                            div()
                                .id("where")
                                .text_size(px(14.))
                                .text_color(rgb(color::LINK))
                                .cursor_pointer()
                                .child("Where do I get a key?")
                                .on_click(move |_, _, cx| cx.open_url(key_page(provider))),
                        ),
                )
                .child(div().flex().flex_col().gap_2().child(label("Model".into())).child(field(&screen.model, "model", false)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            primary_button("save-key", "Save key", m)
                                .h(px(48.))
                                .justify_center()
                                .text_size(px(15.))
                                .on_click(cx.listener(|this, _, window, cx| this.save_keys(window, cx)))
                                .test_support(),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(color::MUTED))
                                .text_center()
                                .font_family(SANS)
                                .child("Your key and your writing go only to the provider you pick."),
                        ),
                ),
        )
    }
}

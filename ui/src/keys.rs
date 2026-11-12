//! The key screen: which provider writes with you, its key and its model, and the feedback link.
//! Keys and the link go only to the key store, and the screen says one is saved without ever
//! reading it back. The link field is the only feedback UI shown before a link is saved.

use crate::app::{Inkling, Screen, ghost_button, primary_button};
use crate::FormFactor;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{Desk, Provider, Secret};

pub(crate) struct KeyScreen {
    pub provider: Provider,
    pub key: Entity<InputState>,
    pub model: Entity<InputState>,
    /// Whether the store holds a key for `provider`.
    pub has_key: bool,
    /// What the last Save did.
    pub status: Option<Result<(), String>>,
    /// Masked like the key. Its placeholder says whether a link is saved, never what it is.
    pub link: Entity<InputState>,
    /// What the last Save link did.
    pub link_status: Option<Result<(), String>>,
    _save_on_enter: [Subscription; 2],
}

impl KeyScreen {
    pub fn new(provider: Provider, app: &Inkling, window: &mut Window, cx: &mut Context<Inkling>) -> Self {
        let key = cx.new(|cx| InputState::new(window, cx).masked(true));
        let model = cx.new(|cx| InputState::new(window, cx));
        let link = cx.new(|cx| InputState::new(window, cx).masked(true));
        let save_key = cx.subscribe_in(&key, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_keys(window, cx);
                window.dismiss_virtual_keyboard();
            }
        });
        let save_link = cx.subscribe_in(&link, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.save_link(window, cx);
                window.dismiss_virtual_keyboard();
            }
        });
        let mut screen = KeyScreen {
            provider,
            key,
            model,
            has_key: false,
            status: None,
            link,
            link_status: None,
            _save_on_enter: [save_key, save_link],
        };
        screen.clear_link(app.feedback_on, window, cx);
        screen.show(provider, app, window, cx);
        screen
    }

    fn show(&mut self, provider: Provider, app: &Inkling, window: &mut Window, cx: &mut App) {
        self.provider = provider;
        self.has_key = app.keys.has(Secret::ApiKey(provider));
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

impl KeyScreen {
    fn clear_link(&self, saved: bool, window: &mut Window, cx: &mut App) {
        let hint = if saved { "Saved in the Keychain. Paste a new link to replace it." } else { "Paste the feedback link" };
        self.link.update(cx, |field, cx| {
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
        let Screen::Keys(mut screen) = std::mem::replace(&mut self.screen, Screen::Page(None)) else { return };
        screen.show(provider, self, window, cx);
        self.screen = Screen::Keys(screen);
        cx.notify();
    }

    /// Writes the key through the key store and the provider and model through `Settings`.
    /// An empty key field keeps the key already saved.
    pub fn save_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = self.screen.keys() else { return };
        let provider = screen.provider;
        let key = screen.key.read(cx).value().trim().to_string();
        let model = screen.model.read(cx).value().to_string();
        let stored = if key.is_empty() { Ok(()) } else { self.keys.set(Secret::ApiKey(provider), &key) };
        self.settings.provider = provider;
        self.settings.set_model(provider, &model);
        let written = self.settings.save(&self.paths).map_err(|e| format!("Couldn't save settings ({e})."));
        let status = stored.and(written);
        self.key_saved = self.keys.has(Secret::ApiKey(provider));
        if let Screen::Keys(screen) = &mut self.screen {
            screen.has_key = self.key_saved;
            screen.status = Some(status);
            screen.clear_key(window, cx);
        }
        cx.notify();
    }

    /// Save link, or Return in the link field. An empty field keeps the link already saved.
    pub fn save_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = self.screen.keys() else { return };
        let pasted = screen.link.read(cx).value().trim().to_string();
        if pasted.is_empty() {
            return;
        }
        let status = Desk::save_link(self.keys.as_ref(), &pasted);
        self.feedback_on = Desk::has_link(self.keys.as_ref());
        if let Screen::Keys(screen) = &mut self.screen {
            screen.link_status = Some(status);
            screen.clear_link(self.feedback_on, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn render_keys(&self, screen: &KeyScreen, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let label = |text: String| div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(c.muted).child(text);
        let field = |input: &Entity<InputState>, id: &'static str, masked: bool| {
            let input = Input::new(input).id(id).appearance(false);
            let input = if masked { input.mask_toggle() } else { input };
            div()
                .h(px(48.))
                .px(px(14.))
                .flex()
                .items_center()
                .bg(c.surface)
                .border_1()
                .border_color(c.hairline)
                .rounded(px(12.))
                .text_size(px(15.))
                .child(div().flex_1().child(input))
        };
        let tabs = div().flex().gap_1().p_1().bg(c.segment).rounded(px(12.)).children(Provider::ALL.map(|p| {
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
                .text_color(if on { c.ink } else { c.muted })
                .when(on, |tab| tab.bg(c.surface).border_1().border_color(c.hairline))
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
        let link_status = match &screen.link_status {
            Some(Err(message)) => Some(message.clone()),
            Some(Ok(())) => Some("Saved. Feedback is on.".to_string()),
            None if self.feedback_on => Some("Saved".to_string()),
            None => None,
        };
        let provider = screen.provider;
        div().id("keys").size_full().flex().flex_col().items_center().overflow_y_scroll().child(
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
                        ghost_button(&c, "back-to-page", "‹ Back to the page")
                            .h(m.control.max(px(32.)))
                            .on_click(cx.listener(|this, _, window, cx| this.close_keys(window, cx)))
                            .test_support(),
                    ),
                )
                .child(
                    div()
                        .font_family(self.theme().body.face)
                        .text_size(px(30.))
                        .line_height(relative(1.2))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Bring your own key"),
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
                                .children(status.map(|s| div().text_size(px(13.)).text_color(c.ink).child(s))),
                        )
                        .child(field(&screen.key, "key", true))
                        .child(
                            div()
                                .id("where")
                                .when(self.form == FormFactor::Phone, |link| link.min_h(m.control).flex().items_center())
                                .text_size(px(14.))
                                .text_color(c.link)
                                .cursor_pointer()
                                .child("Where do I get a key?")
                                .on_click(move |_, _, cx| cx.open_url(key_page(provider))),
                        ),
                )
                .child(div().flex().flex_col().gap_2().child(label("Model".into())).child(field(&screen.model, "model", false)))
                .child(
                    primary_button(&c, "save-key", "Save key", m)
                        .h(if self.form == FormFactor::Phone { px(52.) } else { px(48.) })
                        .justify_center()
                        .text_size(px(15.))
                        .on_click(cx.listener(|this, _, window, cx| this.save_keys(window, cx)))
                        .test_support(),
                )
                .child(div().h(px(1.)).bg(c.hairline))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .gap_3()
                                .child(label("Feedback link".into()))
                                .children(link_status.map(|s| {
                                    div().id("link-status").text_size(px(13.)).text_color(c.ink).text_right().child(s)
                                })),
                        )
                        .child(field(&screen.link, "feedback-link", true))
                        .child(
                            div().flex().child(
                                ghost_button(&c, "save-link", "Save link")
                                    .h(m.control.max(px(32.)))
                                    .px(px(14.))
                                    .border_1()
                                    .border_color(c.hairline)
                                    .rounded(px(8.))
                                    .text_size(px(14.))
                                    .text_color(c.ink)
                                    .on_click(cx.listener(|this, _, window, cx| this.save_link(window, cx)))
                                    .test_support(),
                            ),
                        ),
                ),
        )
    }
}

//! What sits in the Mac window's corners on every screen: the cog and its menu at the top left,
//! and the round feedback button at the bottom right.

use crate::app::{Inkling, OpenKeys, Screen};
use crate::icons::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

/// Inkling's one duration for motion. `DESIGN.md` says why.
pub(crate) const MOTION: Duration = Duration::from_millis(200);

/// The cog's menu. Hovering opens it unpinned and leaving closes it again; a click pins it open
/// until a second click, Escape or a choice. `Closing` lasts while it fades out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Closed,
    Open { pinned: bool },
    Closing,
}

impl Menu {
    pub fn is_open(self) -> bool {
        matches!(self, Menu::Open { .. })
    }
}

impl Inkling {
    fn hover_cog(&mut self, hovered: bool, cx: &mut Context<Self>) {
        match (hovered, self.menu) {
            (true, Menu::Closed | Menu::Closing) => self.menu = Menu::Open { pinned: false },
            (false, Menu::Open { pinned: false }) => return self.close_menu(cx),
            _ => return,
        }
        cx.notify();
    }

    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        match self.menu {
            Menu::Open { pinned: true } => self.close_menu(cx),
            _ => {
                self.menu = Menu::Open { pinned: true };
                cx.notify();
            }
        }
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if !self.menu.is_open() {
            return;
        }
        self.menu = Menu::Closing;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(MOTION).await;
            let _ = this.update(cx, |this, cx| {
                if this.menu == Menu::Closing {
                    this.menu = Menu::Closed;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub fn menu(&self) -> Menu {
        self.menu
    }

    /// Whether the books are on screen, so the menu's first row leads back to writing.
    fn reading_or_library(&self) -> bool {
        matches!(self.screen, Screen::Library(_) | Screen::Reading(_))
    }

    pub(crate) fn render_cog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let open = self.menu.is_open();
        let cog = div()
            .id("cog")
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .when(open, |cog| cog.bg(c.segment))
            .hover(|cog| cog.bg(c.segment))
            .cursor_pointer()
            .child(Icon::Cog.draw(px(18.), c.ink))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx)))
            .test_support();
        let row = |id: &'static str, icon: Icon, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .pl(px(8.))
                .pr(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .hover(|row| row.bg(c.segment))
                .whitespace_nowrap()
                .cursor_pointer()
                .child(icon.draw(px(16.), c.ink))
                .child(label)
        };
        let menu = (self.menu != Menu::Closed).then(|| {
            let menu = div()
                .id("menu")
                .p(px(4.))
                .flex()
                .flex_col()
                .bg(c.surface)
                .border_1()
                .border_color(c.hairline)
                .rounded(px(10.))
                .shadow_md()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.ink)
                .child(if self.reading_or_library() {
                    row("menu-writing", Icon::Pen, "Writing")
                        .on_click(cx.listener(|this, _, window, cx| this.back_to_writing(window, cx)))
                        .test_support()
                } else {
                    row("menu-library", Icon::Book, "Library")
                        .on_click(cx.listener(|this, _, window, cx| this.open_library(window, cx)))
                        .test_support()
                })
                .child(
                    row("menu-appearance", Icon::Artboard, "Appearance")
                        .on_click(cx.listener(|this, _, window, cx| this.open_appearance(window, cx)))
                        .test_support(),
                )
                .child(
                    row("menu-provider", Icon::provider(self.settings.provider), "Provider")
                        .on_click(cx.listener(|this, _, window, cx| this.on_open_keys(&OpenKeys, window, cx)))
                        .test_support(),
                )
                .test_support();
            let (id, opening) = if open { ("menu-in", true) } else { ("menu-out", false) };
            menu.with_animation(id, Animation::new(MOTION).with_easing(ease_out_quint()), move |menu, t| {
                let shown = if opening { t } else { 1. - t };
                menu.opacity(shown).mt(px(-6. * (1. - shown)))
            })
        });
        div()
            .id("cog-corner")
            .absolute()
            .top_0()
            .left_0()
            .p(px(12.))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(6.))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover_cog(*hovered, cx)))
            // The page keeps the caret while the writer opens the menu.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(cog)
            .children(menu)
    }

    /// Round, outlined in ink, at the bottom right. Hidden until a feedback link is saved.
    pub(crate) fn render_feedback_button(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.feedback_on {
            return None;
        }
        let c = self.theme().c;
        Some(
            div().absolute().bottom(px(14.)).right(px(16.)).child(
                div()
                    .id("open-feedback")
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(c.surface)
                    .border(px(1.5))
                    .border_color(c.ink)
                    .hover(|b| b.bg(c.segment))
                    .cursor_pointer()
                    .child(Icon::Message.draw(px(18.), c.ink))
                    .on_click(cx.listener(|this, _, window, cx| this.open_feedback(window, cx)))
                    .test_support(),
            ),
        )
    }
}

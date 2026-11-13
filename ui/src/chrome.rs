//! What sits in the Mac window's corners on every screen: the cog and its menu at the top left,
//! and the round feedback button at the bottom right.

use crate::app::{Inkling, OpenKeys, Screen, ShowReading, ShowWriting};
use crate::FormFactor;
use crate::icons::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use inkling_core::View;
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
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hover_cog(true, cx)
                }
            }))
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
        let keep_reading = !matches!(self.screen, Screen::Reading(_)) && self.last_book().is_some();
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
                .when(keep_reading, |menu| {
                    menu.child(
                        row("menu-keep-reading", Icon::Book, "Keep reading")
                            .on_click(cx.listener(|this, _, window, cx| this.keep_reading(window, cx)))
                            .test_support(),
                    )
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
            // Only the cog opens the menu; leaving the corner closes it.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.hover_cog(false, cx)
                }
            }))
            // The page keeps the caret while the writer opens the menu.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(div().flex().items_center().gap(px(6.)).child(cog).child(self.render_views(cx)))
            .children(menu)
    }

    /// The toggle beside the cog: Write and Read, the view on screen on a filled square.
    pub(crate) fn render_views(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let segment = toggle_segment(self.form);
        let current = self.view();
        // The chosen half sits on a square lighter than the pill: the surface on a light ground,
        // the hairline tone on a dark one, where the surface is darker than the pill.
        let dark = c.ground.r + c.ground.g + c.ground.b < 1.5;
        let raised = if dark { c.hairline } else { c.surface };
        let half = |id: &'static str, view: View, icon: Icon, label: &'static str, keys: &'static str| {
            let on = current == view;
            div()
                .id(id)
                .size(segment)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .when(on, |half| half.bg(raised).border_1().border_color(c.hairline))
                .when(!on, |half| half.hover(|half| half.bg(c.hairline)))
                .cursor_pointer()
                .aria_label(label)
                .aria_selected(on)
                .aria_keyshortcuts(keys)
                .child(icon.draw(px(16.), if on { c.ink } else { c.muted }))
                .on_click(cx.listener(move |this, _, window, cx| this.show_view(view, window, cx)))
                .when(self.form == FormFactor::Mac, |half| {
                    half.tooltip(move |window, cx| match view {
                        View::Writing => Tooltip::new(label).action(&ShowWriting, None).build(window, cx),
                        View::Reading => Tooltip::new(label).action(&ShowReading, None).build(window, cx),
                    })
                })
                .test_support()
        };
        // On iPhone the halves are full 44 pt targets and the pill is exactly as tall as the status row.
        let inset = if self.form == FormFactor::Phone { px(0.) } else { px(2.) };
        div()
            .id("views")
            .flex()
            .gap(inset)
            .p(inset)
            .rounded(px(8.))
            .bg(c.segment)
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(half("show-writing", View::Writing, Icon::Pen, "Writing", "Command+1"))
            .child(half("show-reading", View::Reading, Icon::Book, "Reading", "Command+2"))
            .test_support()
    }

    /// Write or Read in the toggle, or Cmd-1 and Cmd-2. Read goes back to where reading was left:
    /// the library, or the book read last at its place. Before any reading since launch it is the
    /// library. The view already on screen stays as it is.
    pub fn show_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if self.view() == view {
            return;
        }
        if self.screen.keys().is_some() {
            self.close_keys(window, cx);
        } else if self.screen.feedback().is_some() {
            self.close_feedback(window, cx);
        }
        match view {
            View::Writing => self.back_to_writing(window, cx),
            View::Reading if self.books.left_in_book && self.last_book().is_some() => self.keep_reading(window, cx),
            View::Reading => self.open_library(window, cx),
        }
    }

    pub(crate) fn on_show_writing(&mut self, _: &ShowWriting, window: &mut Window, cx: &mut Context<Self>) {
        self.show_view(View::Writing, window, cx);
    }

    pub(crate) fn on_show_reading(&mut self, _: &ShowReading, window: &mut Window, cx: &mut Context<Self>) {
        self.show_view(View::Reading, window, cx);
    }

    /// How far in from the left the window's top corner reaches, so bars along the top start after it.
    pub(crate) fn corner_width(&self) -> Pixels {
        match self.form {
            FormFactor::Mac => px(12. + 32. + 6.) + toggle_segment(self.form) * 2. + px(6.) + px(16.),
            FormFactor::Phone => px(4.) + self.metrics().control + px(2.) + toggle_segment(self.form) * 2. + px(12.),
        }
    }

    /// iPhone: the toggle alone at the top left, as the phone has no cog. The key and feedback
    /// screens have their own way back, and a review its own bar, so it stays off those.
    pub(crate) fn render_phone_corner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let shown = match self.screen {
            Screen::Page => self.revision.review().is_none(),
            Screen::Library(..) | Screen::Reading(_) => true,
            Screen::Keys(_) | Screen::Feedback(_) => false,
        };
        shown.then(|| {
            let c = self.theme().c;
            let size = self.metrics().control;
            let cog = div()
                .id("cog")
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .when(self.panel.is_some(), |cog| cog.bg(c.segment))
                .aria_label("Appearance")
                .child(Icon::Cog.draw(px(20.), c.ink))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.panel.is_some() {
                        this.close_appearance(cx)
                    } else {
                        this.open_appearance(window, cx)
                    }
                }))
                .test_support();
            div().absolute().top_0().left(px(4.)).flex().items_center().gap(px(2.)).child(cog).child(self.render_views(cx))
        })
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

/// One half of the Write and Read toggle: 28 pt on the Mac, a full touch target on iPhone.
fn toggle_segment(form: FormFactor) -> Pixels {
    match form {
        FormFactor::Mac => px(28.),
        FormFactor::Phone => form.metrics().control,
    }
}

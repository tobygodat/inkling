//! The writing page. On the Mac it is the live editor, which draws the Markdown and maths while
//! the writer types. On iPhone it is still the kit's text area, until the live editor takes touch.
//! Everything the app asks of the page goes through here, in bytes of the Markdown source.

use crate::app::Inkling;
use crate::live::{Edited, LiveEditor, Options};
use crate::theme::Theme;
use crate::FormFactor;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::*;
use inkling_core::Appearance;
use std::ops::Range;
use std::path::PathBuf;

pub enum Page {
    Live(Entity<LiveEditor>),
    Plain(Entity<TextareaState>),
}

impl Page {
    pub(crate) fn new(
        form: FormFactor,
        text: String,
        appearance: &Appearance,
        theme: Theme,
        image_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Inkling>,
    ) -> Self {
        match form {
            FormFactor::Mac => Page::Live(cx.new(|cx| {
                let mut page = LiveEditor::new(&text, options(appearance), form, appearance, Some(image_dir), window, cx);
                page.set_theme(theme, cx);
                page
            })),
            FormFactor::Phone => Page::Plain(cx.new(|cx| {
                let mut page = TextareaState::new(window, cx)
                    .soft_wrap(true)
                    .autocorrect(true)
                    .autocapitalize(Autocapitalize::Sentences)
                    .placeholder("Start writing");
                page.set_value(text, window, cx);
                page
            })),
        }
    }

    /// Calls `edited` after every change to the source.
    pub(crate) fn on_edit(&self, window: &mut Window, cx: &mut Context<Inkling>, edited: fn(&mut Inkling, &mut Context<Inkling>)) -> Subscription {
        match self {
            Page::Live(page) => cx.subscribe_in(page, window, move |this, _, _: &Edited, _, cx| edited(this, cx)),
            Page::Plain(page) => cx.subscribe_in(page, window, move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    edited(this, cx);
                }
            }),
        }
    }

    pub fn value(&self, cx: &App) -> String {
        match self {
            Page::Live(page) => page.read(cx).value().to_string(),
            Page::Plain(page) => page.read(cx).value().to_string(),
        }
    }

    pub fn selected_range(&self, cx: &App) -> Range<usize> {
        match self {
            Page::Live(page) => page.read(cx).selection().range(),
            Page::Plain(page) => page.read(cx).selected_range(),
        }
    }

    pub fn cursor(&self, cx: &App) -> usize {
        match self {
            Page::Live(page) => page.read(cx).selection().head,
            Page::Plain(page) => page.read(cx).cursor(),
        }
    }

    pub fn set_selection(&self, range: Range<usize>, cx: &mut App) {
        match self {
            Page::Live(page) => page.update(cx, |p, cx| p.set_selection(range, cx)),
            Page::Plain(page) => page.update(cx, |p, cx| p.set_selected_range(range, cx)),
        }
    }

    /// Replaces `range` of the source with `text`, as one undo step.
    pub fn replace(&self, range: Range<usize>, text: &str, window: &mut Window, cx: &mut App) {
        match self {
            Page::Live(page) => page.update(cx, |p, cx| p.replace(range, text, cx)),
            Page::Plain(page) => page.update(cx, |p, cx| {
                p.set_selected_range(range, cx);
                p.replace(text.to_string(), window, cx);
            }),
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Page::Live(page) => page.read(cx).focus_handle().clone(),
            Page::Plain(page) => page.read(cx).focus_handle(cx),
        }
    }

    /// The live editor, for the headless proof.
    pub fn live(&self) -> Option<&Entity<LiveEditor>> {
        match self {
            Page::Live(page) => Some(page),
            Page::Plain(_) => None,
        }
    }

    /// Draws the writer's appearance and switches. The text area takes its look when rendered.
    pub(crate) fn set_look(&self, appearance: &Appearance, theme: Theme, cx: &mut App) {
        if let Page::Live(page) = self {
            page.update(cx, |p, cx| {
                p.set_theme(theme, cx);
                p.set_options(options(appearance), cx);
            });
        }
    }

    pub(crate) fn render(&self, theme: &Theme) -> AnyElement {
        match self {
            Page::Live(page) => page.clone().into_any_element(),
            Page::Plain(page) => Textarea::new(page)
                .appearance(false)
                .h_full()
                .font_family(theme.body.face)
                .text_size(theme.body.size)
                .line_height(relative(theme.body.line))
                .text_color(theme.c.ink)
                .into_any_element(),
        }
    }
}

fn options(a: &Appearance) -> Options {
    Options { markdown: a.markdown, maths: a.maths }
}

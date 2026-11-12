//! The writing page: the live editor, which draws the Markdown and maths while the writer types,
//! on the Mac and on iPhone. Everything the app asks of the page goes through here, in bytes of
//! the Markdown source.

use crate::app::Inkling;
use crate::live::{Edited, LiveEditor, Options};
use crate::theme::Theme;
use crate::FormFactor;
use gpui_kit::*;
use inkling_core::Appearance;
use std::ops::Range;
use std::path::PathBuf;

pub struct Page(Entity<LiveEditor>);

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
        Page(cx.new(|cx| {
            let mut page = LiveEditor::new(&text, options(appearance), form, appearance, Some(image_dir), window, cx);
            page.set_theme(theme, cx);
            // The phone's page has no caret until it is tapped, so it says where to start.
            if form == FormFactor::Phone {
                page.set_placeholder("Start writing");
            }
            page
        }))
    }

    /// Calls `edited` after every change to the source.
    pub(crate) fn on_edit(&self, window: &mut Window, cx: &mut Context<Inkling>, edited: fn(&mut Inkling, &mut Context<Inkling>)) -> Subscription {
        cx.subscribe_in(&self.0, window, move |this, _, _: &Edited, _, cx| edited(this, cx))
    }

    pub fn value(&self, cx: &App) -> String {
        self.0.read(cx).value().to_string()
    }

    pub fn selected_range(&self, cx: &App) -> Range<usize> {
        self.0.read(cx).selection().range()
    }

    pub fn cursor(&self, cx: &App) -> usize {
        self.0.read(cx).selection().head
    }

    pub fn set_selection(&self, range: Range<usize>, cx: &mut App) {
        self.0.update(cx, |p, cx| p.set_selection(range, cx));
    }

    /// Replaces `range` of the source with `text`, as one undo step.
    pub fn replace(&self, range: Range<usize>, text: &str, cx: &mut App) {
        self.0.update(cx, |p, cx| p.replace(range, text, cx));
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.read(cx).focus_handle().clone()
    }

    /// The live editor, for the headless proof.
    pub fn live(&self) -> &Entity<LiveEditor> {
        &self.0
    }

    /// Draws the writer's appearance and switches.
    pub(crate) fn set_look(&self, appearance: &Appearance, theme: Theme, cx: &mut App) {
        self.0.update(cx, |p, cx| {
            p.set_theme(theme, cx);
            p.set_options(options(appearance), cx);
        });
    }

    pub(crate) fn render(&self) -> AnyElement {
        self.0.clone().into_any_element()
    }
}

fn options(a: &Appearance) -> Options {
    Options { markdown: a.markdown, maths: a.maths }
}

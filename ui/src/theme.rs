//! The look every view draws with, made from the writer's `Appearance` in `Theme::new`. Views take
//! colours and the body's face and sizes from here and nowhere else.

use crate::FormFactor;
use gpui_kit::*;
use inkling_core::{Appearance, Face, Rgb, Tokens};

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub c: Colors,
    pub body: Body,
}

/// `inkling_core::Tokens` as GPUI colours.
#[derive(Clone, Copy)]
pub(crate) struct Colors {
    pub ground: Rgba,
    pub surface: Rgba,
    pub ink: Rgba,
    pub muted: Rgba,
    pub hairline: Rgba,
    pub added_text: Rgba,
    pub added_fill: Rgba,
    pub added_line: Rgba,
    pub added_count: Rgba,
    pub removed_text: Rgba,
    pub removed_fill: Rgba,
    pub removed_line: Rgba,
    pub link: Rgba,
    pub selection: Rgba,
    pub segment: Rgba,
}

impl From<Tokens> for Colors {
    fn from(t: Tokens) -> Self {
        let c = |c: Rgb| rgb(c.hex());
        Colors {
            ground: c(t.ground),
            surface: c(t.surface),
            ink: c(t.ink),
            muted: c(t.muted),
            hairline: c(t.hairline),
            added_text: c(t.added_text),
            added_fill: c(t.added_fill),
            added_line: c(t.added_line),
            added_count: c(t.added_count),
            removed_text: c(t.removed_text),
            removed_fill: c(t.removed_fill),
            removed_line: c(t.removed_line),
            link: c(t.link),
            selection: c(t.selection),
            segment: c(t.segment),
        }
    }
}

/// The page's text.
#[derive(Clone, Copy)]
pub(crate) struct Body {
    pub face: &'static str,
    pub size: Pixels,
    pub line: f32,
    /// The widest the column grows.
    pub column: Pixels,
}

impl Theme {
    /// The appearance in a face this device has. The iPhone sets the same choices a little smaller
    /// and tighter, so its default is `DESIGN.md`'s 19 pt at 1.65, and its column is the screen.
    pub fn new(form: FormFactor, appearance: &Appearance, faces: &[Face]) -> Self {
        let face = if faces.contains(&appearance.face) { appearance.face } else { Face::SourceSerif };
        let size = phone_scale(form, f32::from(appearance.text_size.get()), PHONE_SIZE);
        let line = phone_scale(form, appearance.line_height(), PHONE_LINE);
        let column = match form {
            FormFactor::Mac => px(appearance.page_width.get().into()),
            FormFactor::Phone => px(700.),
        };
        Theme { c: appearance.tokens().into(), body: Body { face: face.family(), size: px(size), line, column } }
    }

    /// GPUI Kit's components (fields, carets, sliders) in the same colours.
    pub fn apply_to_kit(&self, cx: &mut App) {
        let c = &self.c;
        let theme = gpui_kit::component::Theme::global_mut(cx);
        let kit = &mut theme.colors;
        kit.background = c.ground.into();
        kit.foreground = c.ink.into();
        kit.border = c.hairline.into();
        kit.input = c.hairline.into();
        kit.ring = c.ink.into();
        kit.caret = c.ink.into();
        kit.selection = c.selection.into();
        kit.muted_foreground = c.muted.into();
        kit.primary = c.ink.into();
    }
}

/// The iPhone's text size and line height as fractions of the Mac's, at the defaults.
const PHONE_SIZE: f32 = 19. / 21.;
const PHONE_LINE: f32 = 1.65 / 1.75;

/// What a stored size or spacing becomes on `form`.
pub(crate) fn phone_scale(form: FormFactor, value: f32, phone: f32) -> f32 {
    match form {
        FormFactor::Mac => value,
        FormFactor::Phone => (value * phone * 100.).round() / 100.,
    }
}

/// The text size the panel shows on `form`, in whole points.
pub(crate) fn shown_size(form: FormFactor, stored: u16) -> u16 {
    phone_scale(form, f32::from(stored), PHONE_SIZE).round() as u16
}

/// The line height the panel shows on `form`.
pub(crate) fn shown_line(form: FormFactor, stored: f32) -> f32 {
    phone_scale(form, stored, PHONE_LINE)
}

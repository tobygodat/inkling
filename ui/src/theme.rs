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
    /// The Mac draws the writer's appearance, in a face this device has. The iPhone keeps the
    /// default look at its own sizes.
    pub fn new(form: FormFactor, appearance: &Appearance, faces: &[Face]) -> Self {
        match form {
            FormFactor::Mac => {
                let face = if faces.contains(&appearance.face) { appearance.face } else { Face::SourceSerif };
                Theme {
                    c: appearance.tokens().into(),
                    body: Body {
                        face: face.family(),
                        size: px(appearance.text_size.get().into()),
                        line: appearance.line_height(),
                        column: px(appearance.page_width.get().into()),
                    },
                }
            }
            FormFactor::Phone => Theme {
                c: Appearance::default().tokens().into(),
                body: Body { face: Face::SourceSerif.family(), size: px(19.), line: 1.65, column: px(700.) },
            },
        }
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

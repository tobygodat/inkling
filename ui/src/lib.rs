//! Inkling's views, shared by the Mac app and the iPhone host. Nothing here reads the platform:
//! the host passes the form factor, the data folder and the key store.

mod app;
mod inline;
mod keys;
mod review;

pub use app::{Inkling, OpenKeys, Saved};

use gpui_kit::*;
use std::borrow::Cow;

/// Which device the views lay out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFactor {
    Mac,
    Phone,
}

/// Sizes from `DESIGN.md` for one form factor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub body: Pixels,
    pub line: f32,
    pub ui: Pixels,
    pub column: Pixels,
    pub gutter: Pixels,
    /// Inline accept and reject buttons on Mac, the smallest tap target on iPhone.
    pub control: Pixels,
}

impl FormFactor {
    pub(crate) fn metrics(self) -> Metrics {
        match self {
            FormFactor::Mac => Metrics {
                body: px(21.),
                line: 1.75,
                ui: px(13.),
                column: px(700.),
                gutter: px(24.),
                control: px(28.),
            },
            FormFactor::Phone => Metrics {
                body: px(19.),
                line: 1.65,
                ui: px(15.),
                column: px(700.),
                gutter: px(16.),
                control: px(44.),
            },
        }
    }
}

pub(crate) mod color {
    pub const GROUND: u32 = 0xFBFBF9;
    pub const SURFACE: u32 = 0xFFFFFF;
    pub const INK: u32 = 0x1F2328;
    pub const MUTED: u32 = 0x5C6168;
    pub const HAIRLINE: u32 = 0xDEDFDA;
    pub const ADDED_TEXT: u32 = 0x116329;
    pub const ADDED_FILL: u32 = 0xDAFBE1;
    pub const ADDED_LINE: u32 = 0x8DD6A0;
    pub const ADDED_COUNT: u32 = 0x1A7F37;
    pub const REMOVED_TEXT: u32 = 0x82071E;
    pub const REMOVED_FILL: u32 = 0xFFEBE9;
    pub const REMOVED_LINE: u32 = 0xCF222E;
    pub const LINK: u32 = 0x0969DA;
    pub const SELECTION: u32 = 0xB6D4FE;
    pub const SEGMENT: u32 = 0xEEEFEB;
}

pub(crate) const SERIF: &str = "Source Serif 4";
pub(crate) const SANS: &str = "Inter";

const FONTS: [&[u8]; 5] = [
    include_bytes!("../fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../fonts/SourceSerif4-Semibold.ttf"),
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
];

/// Call once after `gpui_kit::init`: loads the bundled fonts, themes the kit and binds the keys.
pub fn init(cx: &mut App) {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect())
        .expect("the bundled fonts load");
    let theme = gpui_kit::component::Theme::global_mut(cx);
    theme.font_family = SANS.into();
    theme.font_size = px(14.);
    theme.shadow = false;
    let c = &mut theme.colors;
    c.background = rgb(color::GROUND).into();
    c.foreground = rgb(color::INK).into();
    c.border = rgb(color::HAIRLINE).into();
    c.input = rgb(color::HAIRLINE).into();
    c.ring = rgb(color::INK).into();
    c.caret = rgb(color::INK).into();
    c.selection = rgb(color::SELECTION).into();
    c.muted_foreground = rgb(color::MUTED).into();
    c.primary = rgb(color::INK).into();
    cx.bind_keys([KeyBinding::new("cmd-,", OpenKeys, None)]);
}

//! Inkling's views, shared by the Mac app and the iPhone host. Nothing here reads the platform:
//! the host passes the form factor, the data folder and the key store.

mod app;
mod appearance;
mod chrome;
mod feedback;
mod icons;
mod inline;
mod keys;
mod library;
pub mod live;
mod page;
mod picker;
mod reading;
mod review;
mod theme;

pub use app::{Ground, Inkling, OpenKeys, Saved, ShowReading, ShowWriting};
pub use chrome::Menu;
pub use feedback::Restart;
pub use icons::Assets;
pub use library::BookHost;
pub use page::Page;

use gpui_kit::*;
use std::borrow::Cow;

/// Which device the views lay out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFactor {
    Mac,
    Phone,
}

/// Sizes from `DESIGN.md` for one form factor that the writer can't change.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub ui: Pixels,
    pub gutter: Pixels,
    /// Inline accept and reject buttons on Mac, the smallest tap target on iPhone.
    pub control: Pixels,
}

impl FormFactor {
    /// For the feedback body: "mac" or "phone".
    pub fn id(self) -> &'static str {
        match self {
            FormFactor::Mac => "mac",
            FormFactor::Phone => "phone",
        }
    }

    /// Whether the system draws the page's caret, selection and handles. On iPhone UIKit does,
    /// while the page has focus.
    pub(crate) fn system_selection(self) -> bool {
        self == FormFactor::Phone
    }

    pub(crate) fn metrics(self) -> Metrics {
        match self {
            FormFactor::Mac => Metrics {
                ui: px(13.),
                gutter: px(24.),
                control: px(28.),
            },
            FormFactor::Phone => Metrics {
                ui: px(15.),
                gutter: px(16.),
                control: px(44.),
            },
        }
    }
}

pub(crate) const SANS: &str = "Inter";

const FONTS: [&[u8]; 5] = [
    include_bytes!("../fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../fonts/SourceSerif4-Semibold.ttf"),
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Medium.ttf"),
    include_bytes!("../fonts/Inter-SemiBold.ttf"),
];

/// Call once after `gpui_kit::init`: loads the bundled fonts, sets the kit's face and binds the
/// keys. Each `Inkling` colours the kit from its appearance.
pub fn init(cx: &mut App) {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect())
        .expect("the bundled fonts load");
    let theme = gpui_kit::component::Theme::global_mut(cx);
    theme.font_family = SANS.into();
    theme.font_size = px(14.);
    theme.shadow = false;
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenKeys, None),
        KeyBinding::new("cmd-1", ShowWriting, None),
        KeyBinding::new("cmd-2", ShowReading, None),
    ]);
    live::bind_keys(cx);
}

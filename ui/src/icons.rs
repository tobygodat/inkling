//! The icons Inkling bundles (`ui/icons/`, sources in `SOURCES.md`), served beside GPUI Kit's own.
//! Hosts register `Assets` so both sets draw on the Mac and on iPhone.

use gpui_kit::*;
use inkling_core::Provider;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Cog,
    Message,
    Artboard,
    Robot,
    Claude,
    Gemini,
}

impl Icon {
    const ALL: [Icon; 6] = [Icon::Cog, Icon::Message, Icon::Artboard, Icon::Robot, Icon::Claude, Icon::Gemini];

    fn path(self) -> &'static str {
        match self {
            Icon::Cog => "icons/inkling/settings.svg",
            Icon::Message => "icons/inkling/message-circle.svg",
            Icon::Artboard => "icons/inkling/frame.svg",
            Icon::Robot => "icons/inkling/bot.svg",
            Icon::Claude => "icons/inkling/claude.svg",
            Icon::Gemini => "icons/inkling/gemini.svg",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Icon::Cog => include_bytes!("../icons/settings.svg"),
            Icon::Message => include_bytes!("../icons/message-circle.svg"),
            Icon::Artboard => include_bytes!("../icons/frame.svg"),
            Icon::Robot => include_bytes!("../icons/bot.svg"),
            Icon::Claude => include_bytes!("../icons/claude.svg"),
            Icon::Gemini => include_bytes!("../icons/gemini.svg"),
        }
    }

    /// ChatGPT draws as the robot: see `ui/icons/SOURCES.md`.
    pub fn provider(provider: Provider) -> Icon {
        match provider {
            Provider::Claude => Icon::Claude,
            Provider::Gemini => Icon::Gemini,
            Provider::ChatGpt => Icon::Robot,
        }
    }

    pub fn draw(self, size: Pixels, color: Rgba) -> Svg {
        svg().path(self.path()).size(size).flex_none().text_color(color)
    }
}

/// Inkling's icons, then GPUI Kit's.
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Icon::ALL.into_iter().find(|i| i.path() == path) {
            Some(icon) => Ok(Some(Cow::Borrowed(icon.bytes()))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_kit::assets::Assets.list(path)?;
        all.extend(Icon::ALL.into_iter().map(Icon::path).filter(|p| p.starts_with(path)).map(SharedString::from));
        Ok(all)
    }
}

//! Reading's look as Swift sees it: the tone, the face and the sizes kept in `settings.json`
//! beside the GPUI app's, and every colour the reader draws, made by `inkling_core`.

use std::sync::Mutex;

use inkling_core::appearance::{LineSpacing, PageWidth, TextSize};
use inkling_core::{Face, Mode, PageMode, Paths, Rgb, Settings, View};

use crate::InklingError;

/// Light and Dark, the paper tones, and Custom: the writer's own four colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Tone {
    Light,
    Dark,
    Custom,
    Paper,
    Sepia,
    Sage,
    Dusk,
    Night,
}

impl From<Mode> for Tone {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Light => Tone::Light,
            Mode::Dark => Tone::Dark,
            Mode::Custom => Tone::Custom,
            Mode::Paper => Tone::Paper,
            Mode::Sepia => Tone::Sepia,
            Mode::Sage => Tone::Sage,
            Mode::Dusk => Tone::Dusk,
            Mode::Night => Tone::Night,
        }
    }
}

impl From<Tone> for Mode {
    fn from(t: Tone) -> Self {
        match t {
            Tone::Light => Mode::Light,
            Tone::Dark => Mode::Dark,
            Tone::Custom => Mode::Custom,
            Tone::Paper => Mode::Paper,
            Tone::Sepia => Mode::Sepia,
            Tone::Sage => Mode::Sage,
            Tone::Dusk => Mode::Dusk,
            Tone::Night => Mode::Night,
        }
    }
}

/// Tight two-line cards by default, or roomier cards with a larger cover and a one-line excerpt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ListDensity {
    Compact,
    Comfortable,
}

impl From<inkling_core::ListDensity> for ListDensity {
    fn from(d: inkling_core::ListDensity) -> Self {
        match d {
            inkling_core::ListDensity::Compact => ListDensity::Compact,
            inkling_core::ListDensity::Comfortable => ListDensity::Comfortable,
        }
    }
}

impl From<ListDensity> for inkling_core::ListDensity {
    fn from(d: ListDensity) -> Self {
        match d {
            ListDensity::Compact => inkling_core::ListDensity::Compact,
            ListDensity::Comfortable => inkling_core::ListDensity::Comfortable,
        }
    }
}

/// How reading looks. Every number is clamped on the way in, as the panel's sliders are.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Look {
    pub tone: Tone,
    /// The body face's family, such as "Source Serif 4". One Inkling doesn't offer reads as it.
    pub face: String,
    /// The body size in points on the Mac; the iPhone draws it at 19/21 of that.
    pub text_size: u16,
    /// Line height in hundredths of the size: 175 is 1.75.
    pub line_spacing: u16,
    /// The column's widest, in points.
    pub page_width: u16,
    pub paged: bool,
    pub density: ListDensity,
}

/// The colours the app draws, each `0xRRGGBB`. `DESIGN.md` names each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Colors {
    pub ground: u32,
    pub surface: u32,
    pub ink: u32,
    pub muted: u32,
    pub hairline: u32,
    pub segment: u32,
    pub link: u32,
    pub selection: u32,
    pub highlight: u32,
    pub added_text: u32,
    pub added_fill: u32,
    pub added_line: u32,
    pub added_count: u32,
    pub removed_text: u32,
    pub removed_fill: u32,
    pub removed_line: u32,
}

/// The faces Inkling offers for reading and writing, Source Serif 4 first.
#[uniffi::export]
pub fn faces() -> Vec<String> {
    Face::ALL.iter().map(|f| f.family().to_string()).collect()
}

/// `settings.json` in a data folder, the same file the GPUI app keeps.
#[derive(uniffi::Object)]
pub struct Looks {
    paths: Paths,
    settings: Mutex<Settings>,
}

#[uniffi::export]
impl Looks {
    #[uniffi::constructor]
    pub fn open(data: String) -> std::sync::Arc<Looks> {
        let paths = Paths { dir: data.into() };
        let settings = Mutex::new(Settings::load(&paths));
        std::sync::Arc::new(Looks { paths, settings })
    }

    pub fn reading(&self) -> Look {
        let mut settings = self.lock();
        *settings = Settings::load(&self.paths);
        let look = settings.look(View::Reading);
        Look {
            tone: look.mode.into(),
            face: look.face.family().to_string(),
            text_size: look.text_size.get(),
            line_spacing: look.line_spacing.get(),
            page_width: look.page_width.get(),
            paged: settings.reading.pages == PageMode::Paged,
            density: settings.reading.density.into(),
        }
    }

    /// Keeps reading's look. While reading and writing share one look, writing takes it too.
    pub fn set_reading(&self, look: Look) -> Result<(), InklingError> {
        let mut settings = self.lock();
        *settings = Settings::load(&self.paths);
        settings.change_look(View::Reading, |a| {
            a.mode = look.tone.into();
            a.face = Face::ALL.into_iter().find(|f| f.family() == look.face).unwrap_or_default();
            a.text_size = TextSize::new(look.text_size.into());
            a.line_spacing = LineSpacing::new(look.line_spacing.into());
            a.page_width = PageWidth::new(look.page_width.into());
        });
        settings.reading.pages = if look.paged { PageMode::Paged } else { PageMode::Scroll };
        settings.reading.density = look.density.into();
        settings.save(&self.paths)?;
        Ok(())
    }

    /// How writing looks, from `settings.json`.
    pub fn writing(&self) -> Look {
        let mut settings = self.lock();
        *settings = Settings::load(&self.paths);
        let look = settings.look(View::Writing);
        Look {
            tone: look.mode.into(),
            face: look.face.family().to_string(),
            text_size: look.text_size.get(),
            line_spacing: look.line_spacing.get(),
            page_width: look.page_width.get(),
            paged: false,
            density: settings.reading.density.into(),
        }
    }

    /// Keeps writing's look. While reading and writing share one look, reading takes it too.
    pub fn set_writing(&self, look: Look) -> Result<(), InklingError> {
        let mut settings = self.lock();
        *settings = Settings::load(&self.paths);
        settings.change_look(View::Writing, |a| {
            a.mode = look.tone.into();
            a.face = Face::ALL.into_iter().find(|f| f.family() == look.face).unwrap_or_default();
            a.text_size = TextSize::new(look.text_size.into());
            a.line_spacing = LineSpacing::new(look.line_spacing.into());
            a.page_width = PageWidth::new(look.page_width.into());
        });
        settings.save(&self.paths)?;
        Ok(())
    }

    /// The colours of `tone`, with Custom in the writer's own colours.
    pub fn colors(&self, tone: Tone) -> Colors {
        let mut look = self.lock().look(View::Reading).clone();
        look.mode = tone.into();
        let t = look.tokens();
        let c = Rgb::hex;
        Colors {
            ground: c(t.ground),
            surface: c(t.surface),
            ink: c(t.ink),
            muted: c(t.muted),
            hairline: c(t.hairline),
            segment: c(t.segment),
            link: c(t.link),
            selection: c(t.selection),
            highlight: c(t.highlight),
            added_text: c(t.added_text),
            added_fill: c(t.added_fill),
            added_line: c(t.added_line),
            added_count: c(t.added_count),
            removed_text: c(t.removed_text),
            removed_fill: c(t.removed_fill),
            removed_line: c(t.removed_line),
        }
    }
}

impl Looks {
    fn lock(&self) -> std::sync::MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_looks_are_kept_clamped_and_coloured_by_core() {
        let data = tempfile::tempdir().unwrap();
        let data = data.path().to_string_lossy().into_owned();
        let looks = Looks::open(data.clone());
        let default = looks.reading();
        assert_eq!(
            (default.tone, default.face.as_str(), default.text_size, default.line_spacing, default.density),
            (Tone::Light, "Source Serif 4", 21, 175, ListDensity::Compact)
        );
        assert_eq!(looks.colors(Tone::Light).ground, 0xFBFBF9);
        assert_eq!(looks.colors(Tone::Dark).ink, 0xE8E6E1);

        let sepia = Look {
            tone: Tone::Sepia,
            face: "Charter".into(),
            text_size: 99,
            line_spacing: 150,
            page_width: 640,
            paged: true,
            density: ListDensity::Comfortable,
        };
        looks.set_reading(sepia.clone()).unwrap();
        let kept = Looks::open(data).reading();
        assert_eq!(kept, Look { text_size: 32, ..sepia }, "the size is clamped and the rest kept");
        assert_eq!(looks.colors(Tone::Sepia).ground, 0xF4ECD8);
    }
}

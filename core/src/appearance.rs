//! How the page looks: the writer's choices (`Appearance`) and every colour the views draw,
//! derived from them in one place (`Appearance::tokens`). An appearance read from disk is always
//! valid: a field that is missing or can't be read takes its default, and a number out of range is
//! clamped, so a bad value never costs the rest of the settings.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Light,
    Dark,
    Custom,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Light, Mode::Dark, Mode::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Light => "Light",
            Mode::Dark => "Dark",
            Mode::Custom => "Custom",
        }
    }
}

/// A 24-bit colour, `0xRRGGBB`. Written to disk as `"#RRGGBB"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(u32);

impl Rgb {
    pub const fn new(hex: u32) -> Self {
        Rgb(hex & 0xFF_FFFF)
    }

    pub const fn hex(self) -> u32 {
        self.0
    }

    /// `#1F2328`, `1f2328` or `#abc`, with spaces around it allowed.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let long: String = match digits.len() {
            3 => digits.chars().flat_map(|c| [c, c]).collect(),
            6 => digits.to_string(),
            _ => return None,
        };
        u32::from_str_radix(&long, 16).ok().map(Rgb::new)
    }

    fn channels(self) -> [f32; 3] {
        [(self.0 >> 16) & 0xFF, (self.0 >> 8) & 0xFF, self.0 & 0xFF].map(|c| c as f32 / 255.)
    }

    fn from_channels([r, g, b]: [f32; 3]) -> Self {
        let byte = |c: f32| (c.clamp(0., 1.) * 255.).round() as u32;
        Rgb::new(byte(r) << 16 | byte(g) << 8 | byte(b))
    }

    /// Hue in degrees, saturation and lightness from 0 to 1.
    pub fn from_hsl(hue: f32, saturation: f32, lightness: f32) -> Self {
        let (s, l) = (saturation.clamp(0., 1.), lightness.clamp(0., 1.));
        let chroma = (1. - (2. * l - 1.).abs()) * s;
        let h = hue.rem_euclid(360.) / 60.;
        let x = chroma * (1. - (h % 2. - 1.).abs());
        let (r, g, b) = match h as u32 {
            0 => (chroma, x, 0.),
            1 => (x, chroma, 0.),
            2 => (0., chroma, x),
            3 => (0., x, chroma),
            4 => (x, 0., chroma),
            _ => (chroma, 0., x),
        };
        let m = l - chroma / 2.;
        Rgb::from_channels([r + m, g + m, b + m])
    }

    /// The inverse of `from_hsl`. A grey has hue 0.
    pub fn hsl(self) -> (f32, f32, f32) {
        let [r, g, b] = self.channels();
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.;
        let d = max - min;
        if d == 0. {
            return (0., 0., l);
        }
        let s = d / (1. - (2. * l - 1.).abs());
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.)
        } else if max == g {
            (b - r) / d + 2.
        } else {
            (r - g) / d + 4.
        };
        (h * 60., s, l)
    }

    /// `t` of the way from `self` to `other`.
    fn mix(self, other: Rgb, t: f32) -> Rgb {
        let (a, b) = (self.channels(), other.channels());
        Rgb::from_channels([0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t))
    }

    /// WCAG relative luminance.
    fn luminance(self) -> f32 {
        let [r, g, b] = self.channels().map(|c| if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// WCAG contrast ratio, from 1 to 21.
    pub fn contrast(self, other: Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn is_dark(self) -> bool {
        self.luminance() < 0.18
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:06X}", self.0)
    }
}

impl fmt::Debug for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Rgb::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("not a colour: {text:?}")))
    }
}

/// The four colours the writer can set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Background,
    /// The surfaces on the background: bars, fields, chips, the panel.
    Foreground,
    Text,
    Accent,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Background, Part::Foreground, Part::Text, Part::Accent];

    pub fn label(self) -> &'static str {
        match self {
            Part::Background => "Background",
            Part::Foreground => "Foreground",
            Part::Text => "Text",
            Part::Accent => "Accent",
        }
    }
}

/// One colour per `Part`. A palette with a colour that can't be read loads as the default one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub background: Rgb,
    pub foreground: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        LIGHT.palette()
    }
}

impl Palette {
    pub fn get(&self, part: Part) -> Rgb {
        match part {
            Part::Background => self.background,
            Part::Foreground => self.foreground,
            Part::Text => self.text,
            Part::Accent => self.accent,
        }
    }

    fn set(&mut self, part: Part, color: Rgb) {
        match part {
            Part::Background => self.background = color,
            Part::Foreground => self.foreground = color,
            Part::Text => self.text = color,
            Part::Accent => self.accent = color,
        }
    }
}

/// The body faces Inkling offers. Source Serif 4 is bundled; a host lists only the others its
/// text system has, and draws a face it lacks in Source Serif 4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Face {
    #[default]
    SourceSerif,
    Charter,
    Georgia,
    IowanOldStyle,
    Palatino,
    AvenirNext,
}

impl Face {
    pub const ALL: [Face; 6] =
        [Face::SourceSerif, Face::Charter, Face::Georgia, Face::IowanOldStyle, Face::Palatino, Face::AvenirNext];

    pub fn family(self) -> &'static str {
        match self {
            Face::SourceSerif => "Source Serif 4",
            Face::Charter => "Charter",
            Face::Georgia => "Georgia",
            Face::IowanOldStyle => "Iowan Old Style",
            Face::Palatino => "Palatino",
            Face::AvenirNext => "Avenir Next",
        }
    }
}

impl Serialize for Face {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.family())
    }
}

impl<'de> Deserialize<'de> for Face {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let family = String::deserialize(d)?;
        Face::ALL
            .into_iter()
            .find(|f| f.family() == family)
            .ok_or_else(|| serde::de::Error::custom(format!("not a face Inkling offers: {family:?}")))
    }
}

/// A whole number held in `LO..=HI`. Every way in clamps, so a value out of range can't exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Clamped<const LO: u16, const HI: u16, const DEFAULT: u16>(u16);

impl<const LO: u16, const HI: u16, const DEFAULT: u16> Clamped<LO, HI, DEFAULT> {
    pub const MIN: u16 = LO;
    pub const MAX: u16 = HI;

    /// Rounds to the nearest whole number and clamps. NaN gives the default.
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            return Self::default();
        }
        Clamped(value.round().clamp(LO as f64, HI as f64) as u16)
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

impl<const LO: u16, const HI: u16, const DEFAULT: u16> Default for Clamped<LO, HI, DEFAULT> {
    fn default() -> Self {
        Clamped(DEFAULT)
    }
}

impl<const LO: u16, const HI: u16, const DEFAULT: u16> Serialize for Clamped<LO, HI, DEFAULT> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u16(self.0)
    }
}

impl<'de, const LO: u16, const HI: u16, const DEFAULT: u16> Deserialize<'de> for Clamped<LO, HI, DEFAULT> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        f64::deserialize(d).map(Self::new)
    }
}

/// The body text size in points.
pub type TextSize = Clamped<15, 32, 21>;
/// The body line height, in hundredths of the text size.
pub type LineSpacing = Clamped<120, 220, 175>;
/// The widest the text column grows, in points.
pub type PageWidth = Clamped<480, 960, 700>;

/// What the writer chose on the Appearance panel. Kept in `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    #[serde(deserialize_with = "lenient")]
    pub mode: Mode,
    /// The colours for `Mode::Custom`, kept while another mode is on.
    #[serde(deserialize_with = "lenient")]
    pub custom: Palette,
    #[serde(deserialize_with = "lenient")]
    pub face: Face,
    #[serde(deserialize_with = "lenient")]
    pub text_size: TextSize,
    #[serde(deserialize_with = "lenient")]
    pub line_spacing: LineSpacing,
    #[serde(deserialize_with = "lenient")]
    pub page_width: PageWidth,
}

/// Reads `T`, or gives its default when the value is there but isn't a `T`.
pub(crate) fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

impl Appearance {
    /// The four colours in use now: the mode's own, or the custom ones.
    pub fn palette(&self) -> Palette {
        match self.mode {
            Mode::Light => LIGHT.palette(),
            Mode::Dark => DARK.palette(),
            Mode::Custom => self.custom,
        }
    }

    /// Sets one colour and turns Custom on. Leaving Light or Dark starts from that mode's
    /// colours, so changing one colour changes only that one on screen.
    pub fn paint(&mut self, part: Part, color: Rgb) {
        self.custom = self.palette();
        self.custom.set(part, color);
        self.mode = Mode::Custom;
    }

    pub fn line_height(&self) -> f32 {
        self.line_spacing.get() as f32 / 100.
    }

    /// Every colour the views draw.
    pub fn tokens(&self) -> Tokens {
        match self.mode {
            Mode::Light => LIGHT,
            Mode::Dark => DARK,
            Mode::Custom => Tokens::custom(self.custom),
        }
    }
}

/// The colours the views draw, by use. `DESIGN.md` names each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub ground: Rgb,
    pub surface: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub hairline: Rgb,
    pub added_text: Rgb,
    pub added_fill: Rgb,
    pub added_line: Rgb,
    pub added_count: Rgb,
    pub removed_text: Rgb,
    pub removed_fill: Rgb,
    pub removed_line: Rgb,
    pub link: Rgb,
    pub selection: Rgb,
    pub segment: Rgb,
}

pub const LIGHT: Tokens = Tokens {
    ground: Rgb::new(0xFBFBF9),
    surface: Rgb::new(0xFFFFFF),
    ink: Rgb::new(0x1F2328),
    muted: Rgb::new(0x5C6168),
    hairline: Rgb::new(0xDEDFDA),
    added_text: Rgb::new(0x116329),
    added_fill: Rgb::new(0xDAFBE1),
    added_line: Rgb::new(0x8DD6A0),
    added_count: Rgb::new(0x1A7F37),
    removed_text: Rgb::new(0x82071E),
    removed_fill: Rgb::new(0xFFEBE9),
    removed_line: Rgb::new(0xCF222E),
    link: Rgb::new(0x0969DA),
    selection: Rgb::new(0xB6D4FE),
    segment: Rgb::new(0xEEEFEB),
};

pub const DARK: Tokens = Tokens {
    ground: Rgb::new(0x15171A),
    surface: Rgb::new(0x1E2125),
    ink: Rgb::new(0xE8E6E1),
    muted: Rgb::new(0xA3A7AD),
    hairline: Rgb::new(0x34383E),
    added_text: Rgb::new(0x7EE2A0),
    added_fill: Rgb::new(0x12361F),
    added_line: Rgb::new(0x2EA043),
    added_count: Rgb::new(0x56D364),
    removed_text: Rgb::new(0xFFA198),
    removed_fill: Rgb::new(0x3D1418),
    removed_line: Rgb::new(0xF85149),
    link: Rgb::new(0x58A6FF),
    selection: Rgb::new(0x264F78),
    segment: Rgb::new(0x25282D),
};

impl Tokens {
    fn palette(&self) -> Palette {
        Palette { background: self.ground, foreground: self.surface, text: self.ink, accent: self.link }
    }

    /// The writer's four colours, with the rest made from them. Changes take Light's or Dark's
    /// greens and reds, whichever suits the background, and muted text is pulled toward the text
    /// colour until it reads.
    fn custom(p: Palette) -> Tokens {
        let changes = if p.background.is_dark() { DARK } else { LIGHT };
        let mut muted = p.text.mix(p.background, 0.28);
        let mut toward = 0.;
        while toward < 1. && (muted.contrast(p.background) < 4.5 || muted.contrast(p.foreground) < 4.5) {
            toward += 0.1;
            muted = p.text.mix(p.background, 0.28 * (1. - toward));
        }
        Tokens {
            ground: p.background,
            surface: p.foreground,
            ink: p.text,
            muted,
            hairline: p.text.mix(p.background, 0.87),
            link: p.accent,
            selection: p.accent.mix(p.background, 0.7),
            segment: p.text.mix(p.background, 0.94),
            ..changes
        }
    }
}

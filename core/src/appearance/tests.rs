use super::*;
use crate::{Paths, Provider, Settings};
use super::{PageMode, ReadingLook, View};
use proptest::prelude::*;
use serde_json::{Value, json};

/// `DESIGN.md`'s colours before the Appearance panel, in `Tokens` order.
const DESIGN_LIGHT: [u32; 15] = [
    0xFBFBF9, 0xFFFFFF, 0x1F2328, 0x5C6168, 0xDEDFDA, 0x116329, 0xDAFBE1, 0x8DD6A0, 0x1A7F37, 0x82071E, 0xFFEBE9,
    0xCF222E, 0x0969DA, 0xB6D4FE, 0xEEEFEB,
];

fn all(t: &Tokens) -> [u32; 15] {
    [
        t.ground, t.surface, t.ink, t.muted, t.hairline, t.added_text, t.added_fill, t.added_line, t.added_count,
        t.removed_text, t.removed_fill, t.removed_line, t.link, t.selection, t.segment,
    ]
    .map(Rgb::hex)
}

fn temp(case: u64) -> Paths {
    let dir = std::env::temp_dir().join(format!("inkling-appearance-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Paths { dir }
}

/// A field as some other build or a hand edit might have written it: a good value, or junk.
fn field(good: impl Strategy<Value = Value>) -> impl Strategy<Value = Option<Value>> {
    prop_oneof![
        4 => good.prop_map(Some),
        1 => Just(None),
        1 => prop_oneof![Just(json!("Comic Sans")), Just(json!(null)), Just(json!([1, 2])), Just(json!("#12345"))].prop_map(Some),
    ]
}

fn number() -> impl Strategy<Value = Value> {
    prop_oneof![(-1e4f64..1e4).prop_map(|n| json!(n)), (0u16..1000).prop_map(|n| json!(n))]
}

fn color() -> impl Strategy<Value = Value> {
    prop_oneof![any::<u32>().prop_map(|c| json!(Rgb::new(c).to_string())), any::<u32>().prop_map(|c| json!(format!("{:06x}", c & 0xFFFFFF)))]
}

fn expect<T: DeserializeOwned + Default>(value: &Option<Value>) -> T {
    value.clone().and_then(|v| T::deserialize(v).ok()).unwrap_or_default()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn an_appearance_on_disk_loads_valid_survives_a_reload_and_light_never_changes(
        shape in prop::sample::select(vec!["object", "absent", "junk"]),
        mode in field(prop::sample::select(vec![json!("light"), json!("dark"), json!("custom"), json!("sepia"), json!("night")])),
        custom in field(prop::collection::vec(color(), 4).prop_map(|c| json!({"background": c[0], "foreground": c[1], "text": c[2], "accent": c[3]}))),
        face in field(prop::sample::select(Face::ALL.map(|f| json!(f.family())).to_vec())),
        sizes in (field(number()), field(number()), field(number())),
        switches in (field(any::<bool>().prop_map(|b| json!(b))), field(any::<bool>().prop_map(|b| json!(b)))),
        paint in (prop::sample::select(Part::ALL.to_vec()), any::<u32>()),
        case in any::<u64>(),
    ) {
        let (text_size, line_spacing, page_width) = sizes;
        let (markdown, maths) = switches;
        let mut appearance = serde_json::Map::new();
        for (key, value) in [("mode", &mode), ("custom", &custom), ("face", &face), ("text_size", &text_size), ("line_spacing", &line_spacing), ("page_width", &page_width), ("markdown", &markdown), ("maths", &maths)] {
            if let Some(value) = value {
                appearance.insert(key.into(), value.clone());
            }
        }
        let mut file = json!({"provider": "Gemini", "models": {"Gemini": "custom-model"}});
        match shape {
            "object" => file["appearance"] = Value::Object(appearance),
            "junk" => file["appearance"] = json!("dark"),
            _ => {}
        }
        let present = shape == "object";
        let paths = temp(case);
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(paths.settings(), serde_json::to_vec(&file).unwrap()).unwrap();

        let loaded = Settings::load(&paths);
        prop_assert_eq!(loaded.provider, Provider::Gemini, "an appearance never costs the rest of the settings");
        prop_assert_eq!(loaded.model(Provider::Gemini), "custom-model");
        let a = &loaded.appearance;
        if !present {
            prop_assert_eq!(a, &Appearance::default(), "a file from before appearance, or with junk for it, loads the default look");
        }
        let clamp = |v: &Option<Value>, lo: u16, hi: u16, default: u16| match v.as_ref().and_then(Value::as_f64) {
            Some(n) if present => n.round().clamp(lo as f64, hi as f64) as u16,
            _ => default,
        };
        prop_assert_eq!(a.text_size.get(), clamp(&text_size, 15, 32, 21));
        prop_assert_eq!(a.line_spacing.get(), clamp(&line_spacing, 120, 220, 175));
        prop_assert_eq!(a.page_width.get(), clamp(&page_width, 480, 960, 700));
        let switch = |v: &Option<Value>| v.as_ref().filter(|_| present).and_then(Value::as_bool).unwrap_or(true);
        prop_assert_eq!(a.markdown, switch(&markdown), "a file from before the switches, or junk for one, draws Markdown");
        prop_assert_eq!(a.maths, switch(&maths), "and maths");
        if present {
            prop_assert_eq!(a.mode, expect::<Mode>(&mode));
            prop_assert_eq!(a.face, expect::<Face>(&face));
            prop_assert_eq!(a.custom, expect::<Palette>(&custom));
        }

        loaded.save(&paths).unwrap();
        prop_assert_eq!(&Settings::load(&paths), &loaded, "an appearance survives save and load");

        let light = Appearance { mode: Mode::Light, ..a.clone() };
        prop_assert_eq!(all(&light.tokens()), DESIGN_LIGHT, "Light is DESIGN.md's colours whatever else is set");

        let (part, hex) = paint;
        let mut painted = a.clone();
        painted.paint(part, Rgb::new(hex));
        let mut wanted = a.palette();
        wanted.set(part, Rgb::new(hex));
        prop_assert_eq!(painted.mode, Mode::Custom, "changing a colour turns Custom on");
        prop_assert_eq!(painted.palette(), wanted, "and changes only that colour on screen");
        let _ = std::fs::remove_dir_all(&paths.dir);
    }

    #[test]
    fn a_colour_survives_its_hex_and_its_hsl(hex in any::<u32>()) {
        let c = Rgb::new(hex);
        prop_assert_eq!(Rgb::parse(&c.to_string()), Some(c));
        prop_assert_eq!(Rgb::parse(&format!(" {:06x} ", c.hex())), Some(c));
        let (h, s, l) = c.hsl();
        let back = Rgb::from_hsl(h, s, l);
        let near = (0..3).all(|i| ((c.hex() >> (8 * i)) & 0xFF).abs_diff((back.hex() >> (8 * i)) & 0xFF) <= 1);
        prop_assert!(near, "{c} → hsl({h}, {s}, {l}) → {back}");
    }
}

/// One change a writer or reader can make, as the panel makes it.
#[derive(Clone, Debug)]
enum Step {
    Look(View, Mode, u16),
    Switch(bool),
    Sync(bool, View),
}

fn step() -> impl Strategy<Value = Step> {
    let view = prop::sample::select(vec![View::Writing, View::Reading]);
    let mode = prop::sample::select([Mode::ALL.to_vec(), Mode::TONES.to_vec()].concat());
    prop_oneof![
        4 => (view.clone(), mode, 15u16..=32).prop_map(|(v, m, size)| Step::Look(v, m, size)),
        1 => any::<bool>().prop_map(Step::Switch),
        2 => (any::<bool>(), view).prop_map(|(on, v)| Step::Sync(on, v)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn reading_and_writing_share_a_look_only_while_synced(steps in prop::collection::vec(step(), 1..24), case in any::<u64>()) {
        let mut s = Settings::default();
        for step in steps {
            let before = s.clone();
            match step {
                Step::Look(view, mode, size) => {
                    s.change_look(view, |a| {
                        a.mode = mode;
                        a.text_size = super::TextSize::new(size.into());
                    });
                    let other = if view == View::Writing { View::Reading } else { View::Writing };
                    prop_assert_eq!(s.look(view).mode, mode);
                    if !s.reading.synced {
                        prop_assert_eq!(s.look(other), before.look(other), "with sync off a change stays in its own view");
                    }
                }
                Step::Switch(on) => s.change_look(View::Writing, |a| a.markdown = on),
                Step::Sync(on, from) => {
                    s.set_synced(on, from);
                    if on {
                        prop_assert!(s.look(from).same_look(before.look(from)), "turning sync on keeps the look on screen");
                    }
                }
            }
            if s.reading.synced {
                prop_assert!(s.appearance.same_look(&s.reading.look), "while synced the two looks are one");
            }
            let markdown_step = matches!(step, Step::Switch(_));
            if !markdown_step {
                prop_assert_eq!(s.appearance.markdown, before.appearance.markdown, "reading never changes writing's switches");
            }
            prop_assert_eq!(s.reading.pages, before.reading.pages, "page mode is reading's alone");
        }
        let paths = temp(case ^ 0x5eed);
        s.save(&paths).unwrap();
        prop_assert_eq!(&Settings::load(&paths), &s, "both looks and the sync survive save and load");
        let _ = std::fs::remove_dir_all(&paths.dir);
    }

    #[test]
    fn a_reading_look_on_disk_loads_valid_and_an_old_file_reads_with_the_writing_look(
        shape in prop::sample::select(vec!["object", "absent", "junk"]),
        mode in field(prop::sample::select(Mode::TONES.map(|m| json!(m.label().to_lowercase())).to_vec())),
        size in field(number()),
        synced in field(any::<bool>().prop_map(|b| json!(b))),
        pages in field(prop::sample::select(vec![json!("scroll"), json!("paged")])),
        case in any::<u64>(),
    ) {
        let writing = json!({"mode": "dark", "text_size": 24, "markdown": false});
        let mut file = json!({"provider": "Claude", "appearance": writing});
        let mut look = serde_json::Map::new();
        for (key, value) in [("mode", &mode), ("text_size", &size)] {
            if let Some(value) = value {
                look.insert(key.into(), value.clone());
            }
        }
        let mut reading = serde_json::Map::new();
        reading.insert("look".into(), Value::Object(look.clone()));
        for (key, value) in [("synced", &synced), ("pages", &pages)] {
            if let Some(value) = value {
                reading.insert(key.into(), value.clone());
            }
        }
        match shape {
            "object" => file["reading"] = Value::Object(reading),
            "junk" => file["reading"] = json!(["sepia"]),
            _ => {}
        }
        let paths = temp(case ^ 0xbeef);
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(paths.settings(), serde_json::to_vec(&file).unwrap()).unwrap();
        let s = Settings::load(&paths);
        prop_assert_eq!(s.appearance.mode, Mode::Dark, "the reading look never costs the writing one");
        prop_assert!(!s.appearance.markdown);
        let present = shape == "object";
        let synced = !present || synced.as_ref().and_then(Value::as_bool).unwrap_or(true);
        prop_assert_eq!(s.reading.synced, synced, "an old file, or junk for reading, is synced");
        prop_assert_eq!(s.reading.pages, if present { expect::<PageMode>(&pages) } else { PageMode::Scroll });
        if synced {
            prop_assert!(s.reading.look.same_look(&s.appearance), "a synced reading look is the writing look");
        } else {
            let stored: Appearance = serde_json::from_value(Value::Object(look)).unwrap_or_default();
            prop_assert_eq!(&s.reading.look, &stored, "an unsynced reading look loads as stored, field by field");
        }
        let _ = std::fs::remove_dir_all(&paths.dir);
    }
}

#[test]
fn a_settings_file_with_a_reading_look_round_trips() {
    let mut s = Settings::default();
    s.set_synced(false, View::Writing);
    s.change_look(View::Reading, |a| a.mode = Mode::Sepia);
    s.reading.pages = PageMode::Paged;
    let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    assert_eq!(back.reading, ReadingLook { look: Appearance { mode: Mode::Sepia, ..Appearance::default() }, synced: false, pages: PageMode::Paged });
}

#[test]
fn text_and_labels_meet_4_5_to_1_in_every_mode() {
    let tones = Mode::TONES.map(|m| (m.label(), Appearance { mode: m, ..Appearance::default() }.tokens()));
    for (name, t) in [("Light", LIGHT), ("Dark", DARK)].into_iter().chain(tones) {
        let pairs = [
            ("ink on ground", t.ink, t.ground),
            ("ink on surface", t.ink, t.surface),
            ("ink on segment", t.ink, t.segment),
            ("ink on selection", t.ink, t.selection),
            ("muted on ground", t.muted, t.ground),
            ("muted on surface", t.muted, t.surface),
            ("muted on segment", t.muted, t.segment),
            ("surface on ink", t.surface, t.ink),
            ("added text on its fill", t.added_text, t.added_fill),
            ("kept text on ground", t.added_text, t.ground),
            ("removed text on its fill", t.removed_text, t.removed_fill),
            ("added count on surface", t.added_count, t.surface),
            ("removed count on surface", t.removed_line, t.surface),
            ("link on ground", t.link, t.ground),
            ("link on surface", t.link, t.surface),
        ];
        for (what, fg, bg) in pairs {
            let ratio = fg.contrast(bg);
            assert!(ratio >= 4.5, "{name}: {what} is {ratio:.2}:1 ({fg} on {bg})");
        }
    }
}

#[test]
fn a_hex_field_takes_three_or_six_digits() {
    assert_eq!(Rgb::parse("#abc"), Some(Rgb::new(0xAABBCC)));
    for bad in ["", "#", "#12345", "#1234567", "#ggg000", "#12 345"] {
        assert_eq!(Rgb::parse(bad), None, "{bad:?}");
    }
}

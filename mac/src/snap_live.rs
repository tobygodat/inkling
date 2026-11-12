//! Headless proof of the live editor on its own, outside the app's page: the offscreen renderer
//! and input dispatched inside the app. No window is shown and nothing takes focus.

use super::*;
use gpui_kit::test::TestWindowExt as _;
use inkling_core::{Appearance, Face, Mode};
use inkling_ui::live::{LiveEditor, Options, Snapshot};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const SAMPLE: &str = r#"# Ferry notes

The crossing takes **twenty-two minutes** and costs \*almost\* nothing &amp; the light is good. See <https://ferry.example/times>.

- A bullet with `inline code`
- A bullet with a [link](https://example.com)

The first plain paragraph is about ferries.

The second plain paragraph is where the caret starts, in the middle of this sentence.

```rust
fn main() {
    println!("hello");
}
```

Energy is $E = mc^2$, a broken one is $\frac{1}{$ and tickets cost $5 and $10 today.

$$
\sum_{k=1}^{n} k = \frac{n(n+1)}{2}
$$

A gull ![a gull](gull.svg) and a harbour ![the harbour](https://img.example/harbour.png) online.

| Boat | Minutes |
|------|---------|
| Ferry | 22 |
"#;

/// Markdown whose drawn text has to match what `SourceMap` says, byte for byte.
const TRICKY: &[&str] = &[
    "a \\*b\\* \\\\ \\[x\\] \\` c",
    "&amp; &copy; &#35; &#x41; &nbsp;x &NotEqualTilde; &bogus;",
    "<https://a.example/x> and https://b.example, www.c.example and <me@d.example>",
    "line one \nline two\r\nline three  \nhard",
    "> quoted\n> more *quoted*\n>\n> - item",
    "1. one\n   continued\n2. two\n   - nested `code`",
    "| a \\| b | `c` |\n|---|:-:|\n| &amp; | **d** |",
    "``` rust\n  indented\n\ttabbed\n```",
    "    indented code\n    second line",
    "$x$ and $5 and $10 and $ y $ and $$z$$",
    "**bold *nested* bold** ~~gone~~ _it_",
    "[ref][r] and ![img][r]\n\n[r]: /there",
    "- [ ] task\n- [x] done",
    "Setext\n======",
];

struct Live {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    view: Entity<LiveEditor>,
    dir: PathBuf,
    frames: Option<(PathBuf, usize)>,
}

impl Live {
    fn open(text: &str, caret: usize, options: Options, images: &Path, dir: &Path) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(inkling_ui::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let text = text.to_string();
        let images = images.to_path_buf();
        let (window, view) = cx
            .update(|cx| {
                gpui_kit::open_window(window_options(size(px(1280.), px(1400.)), false, false), cx, |window, cx| {
                    cx.new(|cx| {
                        let mut e = LiveEditor::new(&text, Options { markdown: true, maths: true }, FormFactor::Mac, &Appearance::default(), Some(images), window, cx);
                        e.set_options(options, cx);
                        e.set_selection(caret..caret, cx);
                        e
                    })
                })
            })
            .expect("a headless window");
        let mut live = Live { cx, window, view, dir: dir.to_path_buf(), frames: None };
        live.frame();
        live
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx.update_window(self.window, |_, w, cx| f(w, cx)).expect("window is open")
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut LiveEditor, &mut Window, &mut Context<LiveEditor>) -> R) -> R {
        let view = self.view.clone();
        let r = self.with(|w, cx| view.update(cx, |e, cx| f(e, w, cx)));
        self.frame();
        r
    }

    fn frame(&mut self) {
        self.cx.run_until_parked();
        self.with(|w, cx| w.render_frame(cx));
        self.with(|w, cx| w.render_frame(cx));
        if let Some((dir, n)) = self.frames.as_mut() {
            *n += 1;
            let path = dir.join(format!("{n:05}.png"));
            if let Ok(image) = self.cx.capture_screenshot(self.window) {
                image.save(path).ok();
            }
        }
    }

    /// Saves every frame to `INKLING_LIVE_FRAMES`, when set, for the review video.
    fn record(&mut self, on: bool) {
        self.frames = std::env::var_os("INKLING_LIVE_FRAMES").filter(|_| on).map(|d| (PathBuf::from(d), 0));
    }

    fn press(&mut self, key: &str) {
        self.with(|w, cx| w.press(key, cx));
        self.frame();
    }

    fn input(&mut self, text: &str) {
        self.with(|w, cx| w.input(text, cx));
        self.frame();
    }

    fn type_slowly(&mut self, text: &str) {
        for c in text.chars() {
            self.input(&c.to_string());
        }
    }

    fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.with(|w, cx| w.drag(from, to, cx));
        self.frame();
    }

    fn shot(&mut self, name: &str) {
        self.frame();
        let image = self.cx.capture_screenshot(self.window).expect("offscreen renderer");
        let path = self.dir.join(format!("{name}.png"));
        image.save(&path).expect("write png");
        println!("wrote {}", path.display());
    }

    fn snap(&mut self) -> Snapshot {
        self.view.read_with(&self.cx, |e, _| e.snapshot())
    }

    fn value(&mut self) -> String {
        self.view.read_with(&self.cx, |e, _| e.value().to_string())
    }

    fn head(&mut self) -> usize {
        self.view.read_with(&self.cx, |e, _| e.selection().head)
    }

    fn caret_to(&mut self, at: usize) {
        self.update(|e, _, cx| e.set_selection(at..at, cx));
    }

    /// The window point just right of the left edge of source byte `at`, half a line down.
    fn point_of(&mut self, at: usize) -> Point<Pixels> {
        let (p, h) = self.view.read_with(&self.cx, |e, _| e.point_for(at)).unwrap_or_else(|| fail(&format!("byte {at} is drawn")));
        point(p.x + px(1.), p.y + h / 2.)
    }

    /// The window point just right of the left edge of drawn byte `index` in the leaf that draws
    /// `drawn` text, found by the kit's text, not by the source map.
    fn point_of_drawn(&mut self, needle: &str) -> Point<Pixels> {
        let snap = self.snap();
        let (leaf, index) = snap
            .leaves
            .iter()
            .filter(|l| !l.is_source)
            .find_map(|l| l.text.find(needle).map(|i| (l, i)))
            .unwrap_or_else(|| fail(&format!("{needle:?} is drawn")));
        let (key, h) = (leaf.key, leaf.line_height.unwrap_or(px(20.)));
        let p = self.view.read_with(&self.cx, |e, _| e.drawn_point(key, index)).unwrap_or_else(|| fail("the leaf is laid out"));
        point(p.x + px(1.), p.y + h / 2.)
    }

    fn click(&mut self, at: Point<Pixels>) {
        self.drag(at, at);
    }
}

fn fail(what: &str) -> ! {
    println!("FAIL {what}");
    std::process::exit(1)
}

fn check(ok: bool, lane: &str, what: &str) {
    if !ok {
        fail(&format!("{lane} {what}"));
    }
    println!("ok   {lane} {what}");
}

fn find(text: &str, needle: &str) -> usize {
    text.find(needle).unwrap_or_else(|| fail(&format!("the sample holds {needle:?}")))
}

/// Link definitions draw as their source, by design.
fn is_definition(text: &str) -> bool {
    text.starts_with('[') && text.contains("]:")
}

fn block_of(snap: &Snapshot, source_at: usize) -> Option<usize> {
    snap.leaves.iter().find(|l| l.source.contains(&source_at)).map(|l| l.block)
}

/// Writes the local image the sample shows.
fn images() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("inkling-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("image dir");
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="24"><ellipse cx="24" cy="12" rx="22" ry="10" fill="#8DA2B5"/></svg>"##;
    std::fs::write(dir.join("gull.svg"), svg).expect("image");
    dir
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).expect("screenshot dir");
    let images = images();
    let caret = find(SAMPLE, "in the middle") + 3;
    let on = Options { markdown: true, maths: true };
    let mut s = Live::open(SAMPLE, caret, on, &images, dir);
    s.record(true);

    // Lane 1.
    let l = "live-01";
    println!("ok   {l} the editor alone, outside the app; the app's own flow on the live page is page-01 above");
    let snap = s.snap();
    let active = snap.active.unwrap_or_else(|| fail("a block holds the caret"));
    let source_leaves: Vec<_> = snap.leaves.iter().filter(|l| l.is_source).collect();
    let para = find(SAMPLE, "The second plain");
    let para_end = para + SAMPLE[para..].find('\n').unwrap_or(0);
    check(
        source_leaves.len() == 1 && source_leaves[0].block == active && source_leaves[0].text == SAMPLE[para..para_end],
        l,
        "the caret's paragraph draws its source and no other block does",
    );
    check(snap.mismatches.is_empty(), l, &format!("every block's kit text equals its SourceMap {:?}", snap.mismatches));
    let leaf_count = snap.leaves.len();
    check(leaf_count >= 12, l, &format!("{leaf_count} leaves are drawn"));
    s.shot("live-01-open");

    // Lane 2.
    let l = "live-02";
    s.press("alt-down");
    check(s.head() == para_end, l, "Option-Down goes to the end of the block");
    let typed = " **bold** and more";
    s.type_slowly(typed);
    let value = s.value();
    let para_end = para_end + typed.len();
    check(value[..para_end].ends_with(&format!("this sentence.{typed}")), l, "the typed text ends the paragraph's source");
    let snap = s.snap();
    let stars: Vec<usize> = value[para..para_end].match_indices("**").map(|(i, _)| para + i).collect();
    let muted = |at: usize| snap.muted.iter().any(|r| r.start <= at && at + 2 <= r.end);
    check(stars.len() == 2 && stars.iter().all(|&at| muted(at)), l, "both ** markers are drawn muted");
    check(!muted(find(&value, "and more")), l, "the words are not muted");
    s.shot("live-02-typed");

    // Lane 3.
    let l = "live-03";
    let first = find(&value, "is about") + 3;
    s.caret_to(first);
    let start_block = s.snap().active;
    let mut presses = 0;
    while s.snap().active == start_block && presses < 6 {
        s.press("down");
        presses += 1;
    }
    let moved = s.snap().active;
    check(moved != start_block, l, &format!("Down leaves the block after {presses} press"));
    s.press("up");
    check(s.head() == first && s.snap().active == start_block, l, "Up returns to the starting byte");
    s.shot("live-03-arrows");

    // Lane 4.
    let l = "live-04";
    let from = find(&value, "ferries") + 2;
    let to = find(&value, "println") + 3;
    let a = s.point_of(from);
    let b = s.point_of(to);
    s.drag(a, b);
    s.press("cmd-c");
    let copied = s.cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    check(copied.as_deref() == Some(&value[from..to]), l, "the clipboard holds the source from the paragraph into the code block");
    let snap = s.snap();
    let spans = (block_of(&snap, from), block_of(&snap, to));
    check(matches!(spans, (Some(x), Some(y)) if y == x + 2), l, "the selection runs two blocks on");
    s.shot("live-04-select");

    // Lane 5.
    let l = "live-05";
    for (drawn, expected, what) in [
        ("and costs", find(&value, "and costs"), "a word after bold text"),
        ("*almost", find(&value, "\\*almost"), "an escape"),
        ("& the", find(&value, "&amp; the"), "an entity"),
    ] {
        let elsewhere = find(&value, "Ferry |");
        s.caret_to(elsewhere);
        let at = s.point_of_drawn(drawn);
        s.click(at);
        check(s.head() == expected, l, &format!("clicking {what} puts the caret on its source byte ({} = {expected})", s.head()));
    }
    s.shot("live-05-click");
    for (i, md) in TRICKY.iter().enumerate() {
        let mut t = Live::open(md, md.len(), on, &images, dir);
        t.caret_to(0);
        let snap = t.snap();
        let source: Vec<&str> = snap.leaves.iter().filter(|x| x.is_source && Some(x.block) != snap.active && !is_definition(&x.text)).map(|x| x.text.as_str()).collect();
        check(
            snap.mismatches.is_empty() && source.is_empty(),
            l,
            &format!("tricky case {i} is drawn and its text is what its SourceMap says {:?} {source:?}", snap.mismatches),
        );
    }

    // Lane 6.
    let l = "live-06";
    s.caret_to(find(&value, "Ferry |"));
    let heading = |snap: &Snapshot| snap.leaves.iter().find(|l| l.block == 0).and_then(|l| Some((l.line_height?, snap.blocks.first()?.size.height)));
    let out = heading(&s.snap()).unwrap_or_else(|| fail("the heading is laid out"));
    s.caret_to(find(&value, "notes"));
    let snap = s.snap();
    let inside = heading(&snap).unwrap_or_else(|| fail("the heading is laid out"));
    check(snap.active == Some(0), l, "the caret's heading draws its source");
    check(out.0 == inside.0, l, &format!("the heading keeps its size ({:?} line height in both)", out.0));
    let change = (inside.1 - out.1).abs();
    check(change <= px(2.), l, &format!("the heading's height changes by {change:?}"));
    s.shot("live-06-heading");

    // Lane 7.
    let l = "live-07";
    s.caret_to(find(&value, "Ferry |"));
    let snap = s.snap();
    let formulas: Vec<&str> = snap.formulas.iter().map(|f| f.0.as_str()).collect();
    check(
        formulas.len() == 2 && formulas[0] == "E = mc^2" && formulas[1].contains("\\sum"),
        l,
        &format!("the inline and block formulas are images {formulas:?}"),
    );
    let text_leaf = snap.leaves.iter().find(|l| l.text.starts_with("Energy is")).unwrap_or_else(|| fail("the maths paragraph is drawn"));
    check(
        text_leaf.text.contains("$\\frac{1}{$") && text_leaf.text.contains("$5 and $10"),
        l,
        "the broken formula and the prices are text",
    );
    let inline = snap.formulas[0].1;
    let line = text_leaf.line_height.unwrap_or_default();
    let top = s.view.read_with(&s.cx, |e, _| e.drawn_point(text_leaf.key, 0)).unwrap_or_default().y;
    let (ascent, descent) = s.with(|w, _| {
        let font = gpui_kit::font(Face::SourceSerif.family());
        let size = px(21.);
        let shaped = w.text_system().shape_line("Energy".into(), size, &[TextRun { len: 6, font, color: black(), background_color: None, underline: None, strikethrough: None }], None);
        (shaped.ascent, shaped.descent)
    });
    let text_baseline = top + (line - ascent - descent) / 2. + ascent;
    check(
        (inline.bottom() - text_baseline).abs() <= px(1.),
        l,
        &format!("E = mc^2, which has no depth, ends on the text baseline ({:?} vs {text_baseline:?})", inline.bottom()),
    );
    s.shot("live-07-maths");

    // Lane 8.
    let l = "live-08";
    s.caret_to(find(&value, "is about ferries") + "is about ferries".len());
    let mut versions = vec![s.value()];
    for word in [" one", " two", " three"] {
        s.type_slowly(word);
        versions.push(s.value());
    }
    for step in (0..3).rev() {
        s.press("cmd-z");
        check(s.value() == versions[step], l, &format!("undo {} restores the source", 3 - step));
    }
    s.press("cmd-shift-z");
    check(s.value() == versions[1], l, "redo puts back the first edit");
    s.shot("live-08-undo");

    // Lane 9.
    s.record(false);
    let l = "live-09";
    let at = s.head();
    s.update(|e, w, cx| e.replace_and_mark_text_in_range(None, "かな", Some(2..2), w, cx));
    let snap = s.snap();
    let marked = at..at + "かな".len();
    check(snap.marked == Some(marked.clone()) && !snap.underlines.is_empty(), l, "the marked text is underlined");
    s.shot("live-09-ime");
    s.update(|e, w, cx| e.replace_text_in_range(None, "仮名", w, cx));
    let v = s.value();
    check(s.snap().marked.is_none() && v[at..].starts_with("仮名") && !v.contains("かな"), l, "committing replaces the marked text");

    // Lane 10.
    let l = "live-10";
    s.update(|e, _, cx| e.set_options(Options { markdown: false, maths: false }, cx));
    let snap = s.snap();
    let v = s.value();
    let plain = snap.leaves.iter().all(|leaf| leaf.is_source && leaf.text == v[leaf.source.clone()]);
    check(plain && !snap.leaves.is_empty(), l, "every block's leaf text equals its source");
    check(snap.formulas.is_empty() && snap.leaves.iter().all(|leaf| !leaf.text.contains('\u{FFFC}')), l, "no image or formula is drawn");
    s.shot("live-10-plain");
    let dark = Appearance { mode: Mode::Dark, ..Appearance::default() };
    s.update(|e, _, cx| e.set_appearance(&dark, cx));
    let snap = s.snap();
    let ink = dark.tokens().ink.hex();
    let as_hex = |c: Rgba| ((c.r * 255.).round() as u32) << 16 | ((c.g * 255.).round() as u32) << 8 | (c.b * 255.).round() as u32;
    check(as_hex(snap.ink) == ink && as_hex(snap.ground) == dark.tokens().ground.hex(), l, "the dark theme gives the page Dark's ink and ground");
    s.shot("live-10-dark");
    let leaf = snap.leaves.iter().find(|l| l.text.starts_with("The first plain")).and_then(|l| l.bounds).unwrap_or_else(|| fail("a paragraph is laid out"));
    let scale = s.with(|w, _| w.scale_factor());
    let image = s.cx.capture_screenshot(s.window).expect("offscreen renderer");
    let (x0, y0) = ((leaf.left().as_f32() * scale) as u32, (leaf.top().as_f32() * scale) as u32);
    let (x1, y1) = (((leaf.right().as_f32() * scale) as u32).min(image.width()), ((leaf.bottom().as_f32() * scale) as u32).min(image.height()));
    let brightest = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| image.get_pixel(x, y).0)
        .max_by_key(|p| p[0] as u32 + p[1] as u32 + p[2] as u32)
        .map_or(0, |p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32);
    let near = |a: u32, b: u32| (0..3).all(|i| ((a >> (8 * i)) & 0xFF).abs_diff((b >> (8 * i)) & 0xFF) <= 10);
    check(near(brightest, ink), l, &format!("the page's text is drawn in Dark's ink (#{brightest:06X} vs #{ink:06X})"));
    std::fs::remove_dir_all(&images).ok();
}

/// Typed character to painted frame, on a 5,000-word draft, for `page`: the live editor alone,
/// or the whole app's page, which reads the draft from `INKLING_DATA_DIR`.
pub fn bench(page: &str) {
    let mut doc = String::from("# Long draft\n\n");
    let mut i = 0;
    while doc.split_whitespace().count() < 5000 {
        doc.push_str(&format!(
            "Paragraph {i} keeps going with **bold words** and *some italic* and a [link](https://example.com) so the \
             parser has marks to handle. The ferry left at seven forty and the water was flat, the light low and \
             orange, and nobody on deck said a word for the whole crossing, which suited everyone fine.\n\n"
        ));
        i += 1;
    }
    let caret = find(&doc, "Paragraph 3 ") + 20;
    let mut times = vec![];
    let images = images();
    match page {
        "live" => {
            let mut s = Live::open(&doc, caret, Options { markdown: true, maths: true }, &images, &images);
            for n in 0..220 {
                let t = Instant::now();
                s.with(|w, cx| w.input("x", cx));
                s.cx.run_until_parked();
                s.with(|w, cx| w.render_frame(cx));
                if n >= 20 {
                    times.push(t.elapsed());
                }
            }
            check(s.value().len() == doc.len() + 220, "bench", "every character reached the draft");
        }
        _ => {
            if std::env::var_os("INKLING_DATA_DIR").is_none() {
                fail("the app bench needs INKLING_DATA_DIR set to a temp folder");
            }
            inkling_core::Paths::mac().save_draft(&doc).expect("write the bench draft");
            let mut cx = HeadlessAppContext::with_platform(
                gpui_kit::platform::current_platform(true).text_system(),
                Arc::new(inkling_ui::Assets),
                gpui_kit::platform::current_headless_renderer,
            );
            cx.update(init);
            let build = Build::with_records(None, []);
            let (window, view) = cx.update(|cx| open(cx, FormFactor::Mac, false, false, Box::new(Arc::new(inkling_core::MemoryKeys::default())), build)).expect("window");
            let with = |cx: &mut HeadlessAppContext, f: &mut dyn FnMut(&mut Window, &mut App)| cx.update_window(window, |_, w, cx| f(w, cx)).expect("window");
            with(&mut cx, &mut |_, cx| view.update(cx, |v, cx| v.page().set_selection(caret..caret, cx)));
            cx.run_until_parked();
            with(&mut cx, &mut |w, cx| w.render_frame(cx));
            for n in 0..220 {
                let t = Instant::now();
                with(&mut cx, &mut |w, cx| w.input("x", cx));
                cx.run_until_parked();
                with(&mut cx, &mut |w, cx| w.render_frame(cx));
                if n >= 20 {
                    times.push(t.elapsed());
                }
            }
            let len = view.read_with(&cx, |v, cx| v.text(cx).len());
            check(len == doc.len() + 220, "bench", "every character reached the draft");
        }
    }
    times.sort();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    println!(
        "bench {page}: {} words, {} keystrokes, median {:.2} ms, p90 {:.2} ms, worst {:.2} ms",
        doc.split_whitespace().count(),
        times.len(),
        ms(times[times.len() / 2]),
        ms(times[times.len() * 9 / 10]),
        ms(times[times.len() - 1])
    );
    std::fs::remove_dir_all(&images).ok();
}

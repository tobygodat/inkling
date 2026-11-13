//! Headless proof: the offscreen renderer and in-app input dispatch. No window is shown, nothing
//! takes focus, and no event reaches the system event tap.
use super::*;
use gpui_kit::test::TestWindowExt as _;
use inkling_core::feedback::{Delivery, FeedbackId, Outcome, Record, SendError, Status};
use inkling_core::{AiEvent, Appearance, Build, ChangeId, Face, Mode, Part, BuildNumber, MemoryKeys, Paths, Provider, Review, Revision, Secret};
use inkling_ui::live::Snapshot;
use inkling_ui::{Menu, Restart};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub mod a11y;
pub mod books;

const PARAGRAPHS: [&str; 4] = [
    "Every weekday the seven-forty ferry leaves without waiting for anyone, and every weekday i am almost not on it.",
    "I like teh crossing because nothing is asked of me. For twenty-two minutes I am between places.",
    "The light on the water was nice this morning, and i dont think anyone else noticed.",
    "Today a man beside me was teaching his daughter to tie a bowline. She got it wrong alot before the knot held",
];

struct Snap {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    view: Entity<Inkling>,
    dir: PathBuf,
    /// Where every frame goes for the review video, and how many so far.
    frames: Option<(PathBuf, usize)>,
}

impl Snap {
    fn open(dir: &Path) -> Self {
        Self::open_as(dir, FormFactor::Mac)
    }

    fn open_as(dir: &Path, form: FormFactor) -> Self {
        Self::open_with(dir, form, Arc::default(), Build::with_records(None, []))
    }

    /// A Mac window of `window_size` instead of 1280 by 800.
    fn open_sized(dir: &Path, window_size: Size<Pixels>) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(inkling_ui::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let keys: Box<dyn inkling_core::KeyStore> = Box::new(Arc::new(MemoryKeys::default()));
        let (window, view) = cx
            .update(|cx| open_sized(cx, FormFactor::Mac, window_size, false, false, keys, Build::with_records(None, [])))
            .expect("open a headless window");
        let mut snap = Snap { cx, window, view, dir: dir.to_path_buf(), frames: None };
        snap.frame();
        snap
    }

    /// `keys` outlives the view, so a relaunch finds what was saved, as it would in the Keychain.
    fn open_with(dir: &Path, form: FormFactor, keys: Arc<MemoryKeys>, build: Build) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(inkling_ui::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let (window, view) =
            cx.update(|cx| open(cx, form, false, false, Box::new(keys), build)).expect("open a headless window");
        let mut snap = Snap { cx, window, view, dir: dir.to_path_buf(), frames: None };
        snap.frame();
        snap
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx.update_window(self.window, |_, window, cx| f(window, cx)).expect("window is open")
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut Inkling, &mut Window, &mut Context<Inkling>) -> R) -> R {
        let view = self.view.clone();
        self.with(|window, cx| view.update(cx, |v, cx| f(v, window, cx)))
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

    /// Saves every frame to `INKLING_PAGE_FRAMES`, when set, for the review video.
    fn record(&mut self, on: bool) {
        self.frames = std::env::var_os("INKLING_PAGE_FRAMES").filter(|_| on).map(|d| (PathBuf::from(d), 0));
    }

    fn type_slowly(&mut self, text: &str) {
        for c in text.chars() {
            self.input(&c.to_string());
        }
    }

    fn press(&mut self, key: &str) {
        self.with(|w, cx| w.press(key, cx));
        self.frame();
    }

    fn input(&mut self, text: &str) {
        self.with(|w, cx| w.input(text, cx));
        self.frame();
    }

    fn click(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.with(|w, cx| w.click(id, cx));
        self.frame();
    }

    fn bounds(&mut self, id: impl Into<ElementId>) -> Option<Bounds<Pixels>> {
        let id = id.into();
        self.with(|w, _| w.try_find(id).map(|e| e.bounds()))
    }

    fn hover(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.with(|w, cx| w.hover(id, cx));
        self.frame();
    }

    /// Plays an animation out. Animations run on the executor's clock, which is the test clock.
    fn settle(&mut self) {
        self.cx.advance_clock(Duration::from_millis(260));
        self.update(|_, _, cx| cx.notify());
        self.frame();
    }

    /// Drags a panel slider's thumb to `fraction` of its track.
    fn slide(&mut self, id: &'static str, from: f32, to: f32) {
        let track = self.bounds(id).unwrap_or_else(|| fail(&format!("the {id} slider is laid out")));
        let at = |f: f32| point(track.left() + track.size.width * f, track.center().y);
        self.with(|w, cx| w.drag(at(from), at(to), cx));
        self.frame();
    }

    /// The height of the page's first line, which follows the text size.
    fn first_line(&mut self) -> Pixels {
        self.point_for(0).map(|(_, h)| h).unwrap_or_default()
    }

    /// The top of a caret at source byte `at` on the live page, and its height.
    fn point_for(&mut self, at: usize) -> Option<(Point<Pixels>, Pixels)> {
        self.read(|v, cx| v.page().live().read(cx).point_for(at))
    }

    fn live(&mut self) -> Snapshot {
        self.read(|v, cx| v.page().live().read(cx).snapshot())
    }

    /// A window point inside drawn text `needle`, `offset` bytes in, found by the kit's text.
    fn point_of_drawn(&mut self, needle: &str, offset: usize) -> Point<Pixels> {
        let snap = self.live();
        let (key, index, h) = snap
            .leaves
            .iter()
            .filter(|l| !l.is_source)
            .find_map(|l| l.text.find(needle).map(|i| (l.key, i + offset, l.line_height.unwrap_or(px(20.)))))
            .unwrap_or_else(|| fail(&format!("{needle:?} is drawn")));
        let p = self.read(|v, cx| v.page().live().read(cx).drawn_point(key, index)).unwrap_or_else(|| fail("the leaf is laid out"));
        point(p.x + px(1.), p.y + h / 2.)
    }

    fn page_focused(&mut self) -> bool {
        let view = self.view.clone();
        self.with(|w, cx| view.read(cx).page().focus_handle(cx).is_focused(w))
    }

    fn cursor(&mut self) -> usize {
        self.read(|v, cx| v.page().cursor(cx))
    }

    fn selected(&mut self) -> std::ops::Range<usize> {
        self.read(|v, cx| v.page().selected_range(cx))
    }

    fn appearance(&mut self) -> Appearance {
        self.read(|v, _| v.appearance().clone())
    }

    fn menu(&mut self) -> Menu {
        self.read(|v, _| v.menu())
    }

    fn advance(&mut self, ms: u64) {
        self.cx.advance_clock(Duration::from_millis(ms));
        self.frame();
    }

    /// Real time for the fake model's thread, test time for the app's timers.
    fn wait(&mut self, ms: u64, until: impl Fn(&Revision) -> bool) {
        for _ in 0..(ms / 10) {
            if self.view.read_with(&self.cx, |v, _| until(v.revision())) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            self.advance(16);
        }
        self.frame();
    }

    fn shot(&mut self, name: &str) {
        self.frame();
        let image = self.cx.capture_screenshot(self.window).expect("offscreen renderer");
        let path = self.dir.join(format!("{name}.png"));
        image.save(&path).expect("write png");
        println!("wrote {}", path.display());
    }

    fn text(&mut self) -> String {
        self.view.read_with(&self.cx, |v, cx| v.text(cx))
    }

    fn revision(&mut self) -> Revision {
        self.view.read_with(&self.cx, |v, _| v.revision().clone())
    }

    fn review(&mut self) -> Review {
        match self.revision() {
            Revision::Reviewing(review) => review,
            other => fail(&format!("expected a review, got {other:?}")),
        }
    }

    fn read<R>(&mut self, f: impl FnOnce(&Inkling, &App) -> R) -> R {
        self.view.read_with(&self.cx, |v, cx| f(v, cx))
    }

    fn statuses(&mut self) -> Vec<String> {
        self.read(|v, _| {
            v.feedback()
                .items()
                .map(|i| match i.status {
                    Status::Pending => "pending".to_string(),
                    Status::Done { .. } => "done".to_string(),
                    Status::Declined { .. } => "declined".to_string(),
                })
                .collect()
        })
    }

    fn field(&mut self) -> String {
        self.read(|v, cx| v.feedback_field().read(cx).value().to_string())
    }

    fn save_link(&mut self, link: &str) {
        self.press("cmd-,");
        self.click("feedback-link");
        self.input(link);
        self.click("save-link");
    }

    fn select(&mut self, range: std::ops::Range<usize>) {
        self.update(|v, _, cx| v.page().set_selection(range, cx));
        self.frame();
    }
}

fn fail(what: &str) -> ! {
    println!("FAIL {what}");
    std::process::exit(1)
}

fn check(ok: bool, what: &str) {
    if !ok {
        fail(what);
    }
    println!("ok   {what}");
}

fn reviewing(r: &Revision) -> bool {
    matches!(r, Revision::Reviewing(_))
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).expect("screenshot dir");
    if std::env::var_os("INKLING_FAKE_AI").is_none() {
        return missing_key(dir);
    }
    let paths = Paths::mac();
    let mut s = Snap::open(dir);
    let fonts = s.cx.update(|cx| cx.text_system().all_font_names());
    check(
        ["Source Serif 4", "Inter"].iter().all(|f| fonts.iter().any(|n| n == f)),
        "the bundled Source Serif 4 and Inter are loaded",
    );
    s.shot("01-empty");
    cog(&mut s);

    for (i, p) in PARAGRAPHS.iter().enumerate() {
        if i > 0 {
            s.press("enter");
            s.press("enter");
        }
        s.input(p);
    }
    let typed = PARAGRAPHS.join("\n\n");
    check(s.text() == typed, "four paragraphs typed by key events");
    s.shot("02-writing");
    let page_text = s.point_for(0).map(|(p, _)| p);

    s.advance(499);
    check(paths.load_draft().is_empty(), "nothing is saved 499 ms after the last edit");
    s.advance(2);
    check(paths.load_draft() == typed, "autosave writes the draft 500 ms after the last edit");

    s.click("fix-grammar");
    s.wait(3000, |r| matches!(r, Revision::Asking { answer, .. } if answer.split_whitespace().count() >= 12));
    check(s.revision().is_busy(), "Fix grammar asks for a new version");
    check(s.text() == typed, "the page is unchanged while asking");
    s.shot("03-asking");

    s.wait(10_000, reviewing);
    let review = s.review();
    let stats = review.stats();
    check(stats.total == 5 && stats.pending == 5 && stats.accepted == 0, "the version has five pending changes");
    check(stats.words_added == 6 && stats.words_removed == 5, "the version bar counts +6 and −5 words");
    s.shot("04-review");
    let page = s.bounds("review").unwrap_or_else(|| fail("the review page is laid out"));
    let review_text = page.origin + point(px(24.), px(21.));
    check(
        page_text == Some(review_text),
        &format!("the review's text starts exactly where the page's text does ({page_text:?} vs {review_text:?})"),
    );
    for i in 0..stats.total {
        let accept = s.bounds(("accept", i)).unwrap_or_else(|| {
            let paths = s.with(|w, _| gpui_kit::base::test_support::registered_paths(w));
            fail(&format!("change {i} has an accept button. Registered: {paths}"))
        });
        let reject = s.bounds(("reject", i)).unwrap_or_else(|| fail(&format!("change {i} has a reject button")));
        let inline = page.contains(&accept.center()) && accept.top() == reject.top() && accept.right() <= reject.left();
        check(inline, &format!("change {i}'s controls sit side by side inside the page"));
    }

    s.click(("accept", 0usize));
    check(s.review().stats().accepted == 1, "the inline accept button accepts its change");
    s.shot("05-accepted-one");
    check(s.bounds(("undo", 0usize)).is_some(), "an accepted change offers Undo");

    s.click(("reject", 1usize));
    check(s.review().stats().rejected == 1, "the inline reject button rejects its change");
    s.click(("undo", 1usize));
    check(s.review().stats().pending == 4, "Undo makes a rejected change pending again");
    s.click(("reject", 1usize));
    s.shot("06-rejected-one");
    check(paths.load_draft() == typed && s.text() == typed, "the draft file and the page don't change during review");

    let expected = s.review().result();
    s.press("escape");
    check(s.revision() == Revision::Idle && s.text() == expected, "Esc goes back to the draft with only the accepted change");

    let draft = s.text();
    let nice = draft.find("nice").expect("the word");
    s.select(nice..nice + 4);
    s.click("find-word");
    s.wait(5000, reviewing);
    let change = s.review().changes().next().map(|c| (c.more, c.removed.to_string()));
    check(change == Some((3, "nice".into())), "Find a word offers a wording with 3 more");
    s.click(("more", 0usize));
    s.shot("07a-wordings-open");
    s.click(("wording", 2usize));
    let chosen = s.review().changes().next().map(|c| c.added.to_string());
    check(chosen.as_deref() == Some("still"), "choosing from \"3 more\" uses that wording");
    s.shot("07-other-wording");
    s.press("tab");
    check(s.revision() == Revision::Idle && s.text() == draft.replacen("nice", "still", 1), "Tab accepts the last change and ends the review");

    let before = s.text();
    s.click("fix-grammar");
    s.wait(10_000, reviewing);
    let mut all = s.review();
    all.accept_rest();
    let edits = all.edits().len();
    let expected = all.result();
    s.click("accept-rest");
    check(s.revision() == Revision::Idle && s.text() == expected, "Accept the rest puts Review::result() in the page");
    s.shot("08-accept-the-rest");
    for _ in 0..edits {
        s.press("cmd-z");
    }
    check(s.text() == before, &format!("Cmd-Z undoes the {edits} accepted changes one by one"));
    for _ in 0..edits {
        s.press("cmd-shift-z");
    }
    check(s.text() == expected, "Shift-Cmd-Z redoes them");
    s.advance(501);
    check(paths.load_draft() == expected, "the result is autosaved");

    s.click("fix-grammar");
    s.wait(10_000, |r| !r.is_busy());
    let notice = s.view.read_with(&s.cx, |v, _| v.notice().map(str::to_string));
    check(s.revision() == Revision::Idle && notice.is_some(), "an empty version says there was nothing to change");
    s.shot("09-nothing-to-change");

    s.click("next-line");
    check(s.revision().is_busy(), "Next line asks");
    s.press("escape");
    check(s.revision() == Revision::Idle && s.text() == expected, "Esc cancels a request and leaves the page alone");

    let end = expected.len();
    s.select(end..end);
    s.click("find-word");
    check(
        s.revision() == Revision::Failed("Select a word or phrase first.".into()),
        "Find a word with nothing selected asks for a selection",
    );
    s.shot("10a-find-a-word-needs-a-selection");
    s.press("escape");

    s.click("next-line");
    let busy = "Claude is busy right now. Try again in a moment.";
    s.update(|v, w, cx| v.feed(AiEvent::Failed(busy.into()), w, cx));
    s.wait(400, |_| false);
    check(s.revision() == Revision::Failed(busy.into()), "a provider failure shows its message and later events are dropped");
    s.shot("10-failure");
    s.press("escape");

    s.press("cmd-,");
    check(s.view.read_with(&s.cx, |v, _| v.key_screen_provider()) == Some(Provider::Claude), "Cmd-Comma opens the key screen");
    let key = "sk-ant-snap-test-not-a-real-key";
    s.input(key);
    s.click("save-key");
    let settings = std::fs::read_to_string(paths.settings()).unwrap_or_default();
    check(!settings.is_empty() && !settings.contains(key), "Save writes settings.json without the key");
    check(s.view.read_with(&s.cx, |v, _| v.key_saved()), "the key screen says Saved from KeyStore::has");
    // A caption under the title or under Save key would push what follows down by a line.
    let below_back = s.bounds("back-to-page").zip(s.bounds("key")).map(|(b, k)| k.top() - b.bottom());
    let below_save = s.bounds("save-key").zip(s.bounds("feedback-link")).map(|(b, l)| l.top() - b.bottom());
    check(
        below_back.is_some_and(|g| g < px(228.)) && below_save.is_some_and(|g| g < px(96.)),
        "the key screen has no caption under its title or under Save key",
    );
    s.shot("11-keys");
    s.press("escape");
    check(s.view.read_with(&s.cx, |v, _| v.key_screen_provider()).is_none(), "Escape closes the key screen");
    check(s.bounds("open-keys").is_none(), "the Mac footer has no provider button");
    s.click("cog");
    s.click("menu-provider");
    check(s.view.read_with(&s.cx, |v, _| v.key_screen_provider()) == Some(Provider::Claude), "Provider in the cog's menu opens the key screen");
    check(s.menu() != Menu::Open { pinned: true }, "choosing Provider closes the menu");
    check(s.bounds("cog").is_some(), "the cog is on the key screen too");
    s.press("escape");

    drop(s);
    let mut relaunch = Snap::open(dir);
    check(relaunch.text() == expected, "a relaunch shows the saved text");
    drop(relaunch);
    phone(dir, &expected);
    feedback(dir);
    appearance(dir);
    live_page(dir);
}

/// The cog at the top left: hover peeks at the menu, a click pins it, and it never covers the text.
fn cog(s: &mut Snap) {
    let cog = s.bounds("cog").unwrap_or_else(|| fail("the window has a cog"));
    check(cog.left() < px(24.) && cog.top() < px(24.), "the cog sits in the top left corner");
    // The headless pointer starts at the window's origin, over the cog.
    s.hover("column");
    s.advance(200);
    check(s.bounds("menu").is_none() && s.menu() == Menu::Closed, "the cog's menu starts closed");
    s.hover("cog");
    s.settle();
    check(s.menu() == Menu::Open { pinned: false }, "hovering the cog opens its menu");
    let menu = s.bounds("menu").unwrap_or_else(|| fail("the open menu is drawn"));
    let column = s.bounds("column").unwrap_or_else(|| fail("the text column is laid out"));
    check(menu.right() < column.left(), "the menu never reaches the text column");
    let rows = s.bounds("menu-appearance").zip(s.bounds("menu-provider"));
    check(rows.is_some_and(|(a, p)| a.bottom() <= p.top()), "the menu lists Appearance, then Provider");
    check(menu.size.width < px(180.), "the menu is only as wide as its rows need");
    s.shot("22-menu-open");
    s.hover("column");
    check(s.menu() == Menu::Closing, "leaving the cog closes an unpinned menu");
    s.advance(200);
    check(s.menu() == Menu::Closed && s.bounds("menu").is_none(), "and it is gone once it has faded");
    s.click("cog");
    s.hover("column");
    check(s.menu() == Menu::Open { pinned: true }, "a click pins the menu open");
    s.click("cog");
    s.advance(200);
    check(s.menu() == Menu::Closed, "a second click closes it");
    s.click("cog");
    s.press("escape");
    s.advance(200);
    check(s.menu() == Menu::Closed && s.text().is_empty(), "Escape closes it and leaves the page alone");
    s.shot("23-menu-closed");
}

/// The Appearance panel beside the page: every change shows on the page at once, is saved, and
/// comes back after a relaunch.
fn appearance(dir: &Path) {
    let paths = Paths::mac();
    let mut s = Snap::open(dir);
    s.hover("column");
    s.advance(200);
    let line = s.first_line();
    let narrow = s.bounds("column").unwrap_or_else(|| fail("the column is laid out"));
    s.click("cog");
    s.click("menu-appearance");
    s.advance(200);
    check(s.read(|v, _| v.appearance_open()), "Appearance in the cog's menu opens the panel");
    let panel = s.bounds("appearance").unwrap_or_else(|| fail("the panel is drawn"));
    let column = s.bounds("column").unwrap_or_else(|| fail("the page stays on screen"));
    check(panel.left() == px(0.) && panel.size.width == px(300.), "the panel is 300 pt wide on the left");
    check(column.left() >= panel.right(), "the page sits beside the panel, not under it");
    check(s.bounds(("face", 0usize)).is_some() && s.bounds(("face", 1usize)).is_some(), "the font list offers Source Serif 4 and this Mac's faces");
    s.shot("24-appearance-open");

    let face = s.read(|v, _| v.faces().get(1).copied()).unwrap_or_else(|| fail("this Mac has a second face"));
    s.click(("face", 1usize));
    check(s.appearance().face == face, &format!("choosing {} sets the body face", face.family()));
    s.slide("text-size", 6. / 17., 15. / 17.);
    let size = s.appearance().text_size.get();
    check(size > 27 && s.first_line() > line, &format!("a bigger text size ({size} pt) takes effect on the page"));
    s.slide("page-width", 220. / 480., 1.);
    let wide = s.bounds("column").unwrap_or_else(|| fail("the column is laid out"));
    check(s.appearance().page_width.get() == 960 && wide.size.width > column.size.width, "a wider page width widens the column");
    s.slide("line-spacing", 0.55, 0.2);
    check(s.appearance().line_spacing.get() < 175, "the line spacing slider sets the spacing");
    let saved = std::fs::read_to_string(paths.settings()).unwrap_or_default();
    check(saved.contains(&format!("\"text_size\": {size}")) && saved.contains(face.family()), "every change is saved to settings.json");
    s.shot("25-appearance-changed");

    s.click(("mode", 1usize));
    check(s.appearance().mode == Mode::Dark && s.appearance().tokens() == inkling_core::appearance::DARK, "Dark switches every colour to the dark set");
    s.shot("26-dark");
    let dark = s.appearance().palette();
    s.with(|w, cx| w.scroll("appearance-body", ScrollDelta::Pixels(point(px(0.), px(-1000.))), cx));
    s.frame();
    s.click(("color", 3usize));
    check(s.read(|v, _| v.picker_part()) == Some(Part::Accent), "a colour row opens the picker under it");
    check(s.bounds(("swatch", 9usize)).is_some() && s.bounds("wheel").is_none(), "the picker shows ten swatches first");
    s.with(|w, cx| w.scroll("appearance-body", ScrollDelta::Pixels(point(px(0.), px(-1000.))), cx));
    s.frame();
    s.click("custom");
    s.with(|w, cx| w.scroll("appearance-body", ScrollDelta::Pixels(point(px(0.), px(-1000.))), cx));
    s.frame();
    let wheel = s.bounds("wheel").unwrap_or_else(|| fail("Custom shows the colour wheel"));
    let r = wheel.size.width / 2.;
    let (angle, reach) = (30f32.to_radians(), 0.9);
    let to = point(wheel.center().x + r * angle.cos() * reach, wheel.center().y - r * angle.sin() * reach);
    s.with(|w, cx| w.drag(wheel.center(), to, cx));
    s.frame();
    let a = s.appearance();
    let (hue, strength, _) = a.custom.accent.hsl();
    check(a.mode == Mode::Custom, "picking on the wheel switches to Custom");
    check((hue - 30.).abs() < 4. && (strength - 0.9).abs() < 0.05, &format!("dragging on the wheel sets hue and strength ({hue:.0}°, {strength:.2})"));
    check(Part::ALL.iter().all(|&p| p == Part::Accent || a.palette().get(p) == dark.get(p)), "the other colours stay as Dark had them");
    let hex = s.read(|v, cx| v.hex_field(cx));
    check(hex.as_deref() == Some(a.custom.accent.to_string().as_str()), "the hex field shows the colour picked");
    s.shot("27-custom-accent");
    let before = s.appearance();
    s.click("appearance-done");
    check(!s.read(|v, _| v.appearance_open()), "Done closes the panel");
    drop(s);

    let mut s = Snap::open(dir);
    check(s.appearance() == before, "a relaunch restores the appearance");
    s.shot("28-appearance-relaunched");
    s.click("cog");
    s.click("menu-appearance");
    s.advance(200);
    s.with(|w, cx| w.scroll("appearance-body", ScrollDelta::Pixels(point(px(0.), px(-1000.))), cx));
    s.frame();
    s.click("appearance-reset");
    check(s.appearance() == Appearance::default(), "Reset returns to the default look");
    let column = s.bounds("column").map(|b| b.size.width);
    check(s.first_line() == line && column == Some(narrow.size.width.min(px(700.))), "and the page is as it was");
    check(Face::default() == s.appearance().face, "in Source Serif 4");
}

/// The same views laid out for iPhone: no inline controls, a tap selects, a sheet acts.
fn phone(dir: &Path, draft: &str) {
    let mut s = Snap::open_as(dir, FormFactor::Phone);
    check(s.text() == draft, "the iPhone layout opens the same draft");
    let halves = s.bounds("show-writing").zip(s.bounds("show-reading"));
    check(
        halves.is_some_and(|(w, r)| w.size.height >= px(44.) && w.size.width >= px(44.) && r.size.width >= px(44.) && w.top() == px(0.)),
        "on iPhone the Write and Read toggle has 44 pt halves in the status row",
    );
    s.shot("13a-iphone-toggle");
    let still = draft.find("still").expect("the word");
    s.select(still..still + 5);
    s.click("find-word");
    s.wait(5000, reviewing);
    check(s.bounds("open-feedback").is_none(), "with no feedback link the iPhone offers no feedback");
    check(s.bounds(("accept", 0usize)).is_none(), "iPhone draws no inline controls");
    check(s.bounds("views").is_none(), "and the toggle steps aside for the version bar");
    check(s.bounds("keep").is_some() && s.bounds("use").is_some(), "the sheet offers Keep and Use for the change");
    s.shot("13-iphone-review");
    s.click("use");
    check(s.revision() == Revision::Idle && s.text() != draft, "Use accepts the change and ends the review");
}

fn missing_key(dir: &Path) {
    let mut s = Snap::open(dir);
    s.input("A draft with no key yet.");
    s.click("next-line");
    let provider = s.view.read_with(&s.cx, |v, _| v.key_screen_provider());
    check(provider == Some(Provider::Claude), "a missing key opens the key screen on Claude");
    s.shot("12-missing-key");
}

const GOOD_LINK: &str = "https://hooks.example/inkling#k=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// `.invalid` makes the fake transport fail as offline.
const FAILING_LINK: &str = "https://hooks.invalid/inkling#k=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Feedback, from a fresh setup to the next build's statuses and Restart.
fn feedback(dir: &Path) {
    let paths = Paths::mac();
    let keys = Arc::new(MemoryKeys::default());
    let mut s = Snap::open_with(dir, FormFactor::Mac, keys.clone(), Build::with_records(Some(BuildNumber(41)), []));
    check(s.bounds("open-feedback").is_none(), "with no feedback link nothing offers feedback");

    s.save_link("not a link");
    check(
        matches!(s.read(|v, _| v.link_status().cloned()), Some(Err(_))) && !keys.has(Secret::FeedbackLink),
        "a bad link is refused and nothing is saved",
    );
    s.click("feedback-link");
    s.input(GOOD_LINK);
    s.click("save-link");
    check(keys.has(Secret::FeedbackLink) && s.read(|v, _| v.feedback_on()), "a saved link turns feedback on");
    let mut files = vec![paths.dir.clone()];
    let mut leaked = vec![];
    while let Some(path) = files.pop() {
        match std::fs::read_dir(&path) {
            Ok(entries) => files.extend(entries.flatten().map(|e| e.path())),
            Err(_) => {
                let bytes = std::fs::read(&path).unwrap_or_default();
                if String::from_utf8_lossy(&bytes).contains("#k=") || String::from_utf8_lossy(&bytes).contains("hooks.example") {
                    leaked.push(path);
                }
            }
        }
    }
    check(leaked.is_empty(), &format!("the link is in no file in the data folder {leaked:?}"));
    s.shot("14-feedback-link");
    s.press("escape");
    let feedback = s.bounds("open-feedback");
    let window = s.with(|w, _| w.viewport_size());
    check(
        feedback.is_some_and(|b| b.size.width == b.size.height && b.size.width >= px(40.) && b.right() > window.width - px(24.) && b.bottom() > window.height - px(24.)),
        "a round feedback button sits in the bottom right corner",
    );
    s.input("i like teh ferry. ");
    s.click("fix-grammar");
    s.wait(5000, reviewing);
    check(reviewing(&s.revision()) && s.bounds("open-feedback").is_some(), "the feedback button stays during a review");
    s.shot("21-feedback-button-in-review");
    s.press("escape");

    s.click("open-feedback");
    check(s.read(|v, _| v.feedback_open()), "Feedback opens the feedback screen");
    check(s.statuses().is_empty() && s.bounds(("feedback-item", 0usize)).is_none(), "an empty list says nothing was sent");
    s.shot("15-feedback-empty");
    s.press("cmd-enter");
    check(s.bounds("feedback-message").is_some() && s.statuses().is_empty(), "Send with nothing written says to write first");
    s.input("M");
    check(s.bounds("feedback-message").is_none(), "typing clears \"Write what should change first.\"");
    s.press("backspace");
    s.input("Make the margins wider on the Mac.");
    s.click("send-feedback");
    check(s.statuses() == ["pending"] && s.field().is_empty(), "a send lands in the list as waiting for a build");
    let below_send = s.bounds("send-feedback").zip(s.bounds(("feedback-item", 0usize))).map(|(b, i)| i.top() - b.bottom());
    check(below_send.is_some_and(|g| g < px(64.)), "nothing sits between Send and the list");
    let log = std::fs::read_to_string(paths.sent_feedback()).unwrap_or_default();
    let draft_in_log = PARAGRAPHS.iter().any(|p| log.contains(&p[..24]));
    check(log.contains("Make the margins wider on the Mac.") && !draft_in_log, "the sent log holds the text and not the draft");
    s.shot("16-feedback-sent");

    s.save_link(FAILING_LINK);
    s.press("escape");
    s.click("open-feedback");
    s.input("Bigger title.");
    s.click("send-feedback");
    let delivery = s.read(|v, _| v.feedback().delivery().clone());
    check(
        delivery == Delivery::Failed(SendError::Offline) && s.field() == "Bigger title." && s.statuses().len() == 1,
        "a failed send says so and keeps the text",
    );
    check(s.bounds("feedback-message").is_some(), "the failure is shown under the field, with Try again on the button");
    s.shot("17-feedback-failed");
    s.save_link(GOOD_LINK);
    s.press("escape");
    s.click("open-feedback");
    check(s.field() == "Bigger title.", "the text survives leaving the screen");
    s.click("send-feedback");
    check(s.statuses().len() == 2 && s.field().is_empty(), "Try again sends the same text");

    s.input("A darker caret.");
    s.click("send-feedback");
    let ids: Vec<FeedbackId> = s.read(|v, _| v.feedback().items().map(|i| i.sent.id).collect());
    drop(s);

    let next = || {
        Build::with_records(
            Some(BuildNumber(42)),
            [
                Record { id: ids[2], outcome: Outcome::Done { summary: "The page margins are 32 pt wider on the Mac.".into() }, seconds_to_ship: 412 },
                Record {
                    id: ids[1],
                    outcome: Outcome::Declined { reason: "The title is already 36 pt, the largest DESIGN.md allows.".into() },
                    seconds_to_ship: 95,
                },
            ],
        )
    };
    let mut s = Snap::open_with(dir, FormFactor::Mac, keys.clone(), next());
    s.click("open-feedback");
    check(s.statuses() == ["pending", "declined", "done"], "the next build shows done, declined and waiting");
    s.shot("18-feedback-statuses");

    let mut phone = Snap::open_with(dir, FormFactor::Phone, keys.clone(), next());
    phone.click("open-feedback");
    check(phone.statuses() == ["pending", "declined", "done"], "the phone layout lists the same statuses");
    let rows: Vec<_> = (0..3usize).map(|i| phone.bounds(("feedback-item", i))).collect();
    check(rows.iter().all(|r| r.is_some_and(|b| b.size.height >= px(44.))), "on iPhone the rows are at least 44 pt");
    phone.shot("19-iphone-feedback");
    drop(phone);

    s.press("escape");
    let restarts = Rc::new(Cell::new(0));
    let seen = restarts.clone();
    let view = s.view.clone();
    let _sub = s.cx.update(|cx| cx.subscribe(&view, move |_, _: &Restart, _| seen.set(seen.get() + 1)));
    s.update(|v, _, cx| v.set_installed_build(Some(BuildNumber(42)), cx));
    s.frame();
    check(s.bounds("restart").is_none(), "the running build on disk offers no restart");
    s.update(|v, _, cx| v.set_installed_build(Some(BuildNumber(43)), cx));
    s.frame();
    check(s.bounds("restart").is_some_and(|b| b.size.height >= px(24.)), "a newer build on disk offers a Restart button");
    s.shot("20-restart");
    s.input("Restart now. ");
    s.click("restart");
    check(paths.load_draft() == s.text() && s.text().contains("Restart now."), "Restart saves the draft first");
    check(restarts.get() == 1, "Restart asks the host once");
    drop(s);

    let mut dev = Snap::open_with(dir, FormFactor::Mac, keys, Build::with_records(None, []));
    dev.update(|v, _, cx| v.set_installed_build(Some(BuildNumber(43)), cx));
    dev.frame();
    check(dev.bounds("restart").is_none(), "a development build never offers a restart");
}

const PAGE: &str = "# Ferry notes\n\n- The crossing takes **twenty-two** minutes\n- The fare is cheap\n\nEnergy is $E = mc^2$ on the water.\n\nThe caret starts in this last paragraph.";
const GRAMMAR: &str = "i like the **teh** ferry, and $E = mc^2$ is still true.\n\nWe watched the **cold** grey water and dont talk.";

fn lane(ok: bool, lane: &str, what: &str) {
    check(ok, &format!("{lane} {what}"));
}

/// The heading's line is taller than the body's.
fn heading_large(snap: &Snapshot) -> bool {
    let line = |f: &dyn Fn(&inkling_ui::live::SnapLeaf) -> bool| snap.leaves.iter().find(|l| f(l)).and_then(|l| l.line_height);
    match (line(&|l| l.block == 0), line(&|l| l.text.contains("on the water"))) {
        (Some(heading), Some(body)) => heading > body,
        _ => false,
    }
}

fn formula_drawn(snap: &Snapshot) -> bool {
    snap.formulas.iter().any(|f| f.0 == "E = mc^2")
}

/// The Mac page is the live editor: its two switches, the review over its source, Find a word,
/// autosave and focus, each checked on the whole app.
fn live_page(dir: &Path) {
    let paths = Paths::mac();
    let l = "page-01";
    println!("ok   {l} every step above, the write, ask, review, accept and finish flow, passed on the live page");
    paths.save_draft(PAGE).expect("write the draft");
    let mut s = Snap::open(dir);
    s.shot("page-01-flow");

    let l = "page-02";
    lane(s.text() == PAGE && s.appearance().markdown && s.appearance().maths, l, "the draft opens with both switches on");
    let end = PAGE.len();
    s.select(end..end);
    let snap = s.live();
    lane(heading_large(&snap), l, "the heading is drawn larger than body text");
    lane(formula_drawn(&snap), l, &format!("the formula is drawn as an image {:?}", snap.formulas));
    lane(snap.mismatches.is_empty(), l, "every block's drawn text matches its source map");
    s.shot("page-02-on");

    let l = "page-03";
    s.click("cog");
    s.click("menu-appearance");
    s.advance(200);
    s.click("markdown");
    lane(!s.appearance().markdown, l, "the Markdown switch turns Markdown off");
    lane(s.page_focused() && s.cursor() == end, l, "and the page keeps focus and its caret");
    s.click("maths");
    lane(s.appearance().maths, l, "the maths switch is disabled while Markdown is off and keeps its value");
    let snap = s.live();
    let v = s.text();
    let plain = snap.leaves.iter().all(|leaf| leaf.is_source && leaf.text == v[leaf.source.clone()]);
    let drawn: String = snap.leaves.iter().map(|leaf| leaf.text.as_str()).collect::<String>().split_whitespace().collect();
    let source: String = v.split_whitespace().collect();
    lane(plain && drawn == source && snap.formulas.is_empty(), l, "the page's drawn text is the draft's source");
    s.shot("page-03-off");

    let l = "page-04";
    s.click("markdown");
    s.click("maths");
    lane(s.appearance().markdown && !s.appearance().maths, l, "Markdown on, maths off");
    let snap = s.live();
    let maths_leaf = snap.leaves.iter().find(|leaf| leaf.text.contains("on the water")).map(|leaf| leaf.text.clone());
    lane(snap.formulas.is_empty() && maths_leaf.is_some_and(|t| t.contains("$E = mc^2$")), l, "the formula is its source");
    lane(heading_large(&snap), l, "the heading is still large");
    s.shot("page-04-md-only");
    let saved = std::fs::read_to_string(paths.settings()).unwrap_or_default();
    lane(saved.contains("\"markdown\": true") && saved.contains("\"maths\": false"), l, "both switches are saved to settings.json");

    let l = "page-05";
    drop(s);
    let mut s = Snap::open(dir);
    s.select(end..end);
    let a = s.appearance();
    let snap = s.live();
    lane(a.markdown && !a.maths, l, "a relaunch on the same data folder keeps both switches");
    lane(snap.formulas.is_empty() && heading_large(&snap), l, "and the page draws as it did before the relaunch");
    s.shot("page-05-saved");
    s.update(|v, w, cx| v.set_appearance(|a| a.maths = true, w, cx));
    s.frame();
    lane(formula_drawn(&s.live()), l, "turning maths back on draws the formula at once");
    drop(s);

    let l = "page-06";
    paths.save_draft(GRAMMAR).expect("write the draft");
    let mut s = Snap::open(dir);
    s.click("fix-grammar");
    s.wait(10_000, reviewing);
    let review = s.review();
    lane(review.draft() == GRAMMAR, l, "Fix grammar asks about the Markdown source, markers included");
    let changes: Vec<_> = review.changes().map(|c| (c.range.clone(), c.removed.to_string(), c.added.to_string())).collect();
    lane(
        changes.len() >= 2 && changes.iter().all(|(range, removed, _)| &GRAMMAR[range.clone()] == removed),
        l,
        &format!("each change's marked range is its range in the source {changes:?}"),
    );
    let drawn: String = review
        .segments()
        .iter()
        .map(|seg| match seg {
            inkling_core::Segment::Unchanged(t) => t.to_string(),
            inkling_core::Segment::Change(c) => c.removed.to_string(),
        })
        .collect();
    lane(drawn == GRAMMAR, l, "the review draws the source text, with ** and $ as written");
    s.shot("page-06-review");

    let l = "page-07";
    let mut two = review.clone();
    let ids: Vec<ChangeId> = two.changes().map(|c| c.id).take(2).collect();
    for &id in &ids {
        two.accept(id);
    }
    let expected = two.result();
    s.click(("accept", 0usize));
    s.click(("accept", 1usize));
    s.press("escape");
    lane(s.revision() == Revision::Idle && s.text() == expected, l, "accepting two changes and finishing puts them in the source");
    s.shot("page-07-applied");
    s.press("cmd-z");
    s.press("cmd-z");
    lane(s.text() == GRAMMAR, l, "two undo steps restore the source");
    s.press("cmd-shift-z");
    s.press("cmd-shift-z");
    lane(s.text() == expected, l, "and two redo steps put them back");

    let l = "page-08";
    let from = s.point_of_drawn("the cold", 0);
    let to = s.point_of_drawn("grey", 4);
    s.with(|w, cx| w.drag(from, to, cx));
    s.frame();
    let sel = s.selected();
    lane(&expected[sel.clone()] == "the **cold** grey", l, &format!("dragging over drawn bold text selects its source {:?}", &expected[sel.clone()]));
    s.shot("page-08-find-word");
    s.click("find-word");
    let asked = match s.revision() {
        Revision::Asking { request, .. } => Some(request.ask().clone()),
        _ => None,
    };
    lane(asked == Some(inkling_core::Ask::FindWord(sel.clone())), l, &format!("Find a word asks about the selection's source range {asked:?}"));
    s.press("escape");
    drop(s);

    let l = "page-09";
    paths.save_draft("").expect("clear the draft");
    let mut s = Snap::open(dir);
    s.hover("column");
    s.advance(200);
    s.record(true);
    for (i, line) in ["# Crossing", "- The ferry leaves at **7:40**", "- It takes 22 minutes", "Its energy is $E = mc^2$, give or take."].iter().enumerate() {
        if i > 0 {
            s.press("enter");
            if i != 2 {
                s.press("enter");
            }
        }
        s.type_slowly(line);
    }
    for _ in 0..8 {
        s.frame();
    }
    s.record(false);
    let typed = s.text();
    lane(typed.starts_with("# Crossing\n\n- The ferry") && typed.ends_with("give or take."), l, "a heading, a list and a formula typed by key events");
    s.advance(501);
    lane(std::fs::read(paths.draft()).ok() == Some(typed.clone().into_bytes()), l, "500 ms later draft.md is the page's source byte for byte");
    let snap = s.live();
    lane(snap.leaves.iter().any(|leaf| leaf.block == 0 && !leaf.is_source), l, "the heading is drawn once the caret leaves it");
    s.shot("page-09-autosave");

    let l = "page-10";
    let at = typed.find("22").expect("the number") + 1;
    s.select(at..at);
    s.press("cmd-,");
    lane(s.read(|v, _| v.key_screen_provider()).is_some(), l, "Cmd-Comma opens the key screen");
    s.press("escape");
    lane(s.page_focused(), l, "closing the key screen gives the page focus again");
    lane(s.cursor() == at, l, "and the caret is where it was");
    s.shot("page-10-focus");
}

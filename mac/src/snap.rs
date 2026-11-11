//! Headless proof: the offscreen renderer and in-app input dispatch. No window is shown, nothing
//! takes focus, and no event reaches the system event tap.
use super::*;
use gpui_kit::test::TestWindowExt as _;
use inkling_core::{AiEvent, Paths, Provider, Review, Revision};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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
}

impl Snap {
    fn open(dir: &Path) -> Self {
        Self::open_as(dir, FormFactor::Mac)
    }

    fn open_as(dir: &Path, form: FormFactor) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(gpui_kit::assets::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let (window, view) = cx.update(|cx| open(cx, form, false, false)).expect("open a headless window");
        let mut snap = Snap { cx, window, view, dir: dir.to_path_buf() };
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

    fn select(&mut self, range: std::ops::Range<usize>) {
        self.update(|v, _, cx| v.page().update(cx, |p, cx| p.set_selected_range(range, cx)));
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
    let page_text = s.view.read_with(&s.cx, |v, cx| v.page().read(cx).range_to_bounds(&(0..0)).map(|b| b.origin));

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
    check(
        page_text == Some(page.origin + point(px(10.), px(8.))),
        "the review's text starts exactly where the page's text does",
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
    s.shot("11-keys");
    s.press("escape");

    drop(s);
    let mut relaunch = Snap::open(dir);
    check(relaunch.text() == expected, "a relaunch shows the saved text");
    drop(relaunch);
    phone(dir, &expected);
}

/// The same views laid out for iPhone: no inline controls, a tap selects, a sheet acts.
fn phone(dir: &Path, draft: &str) {
    let mut s = Snap::open_as(dir, FormFactor::Phone);
    check(s.text() == draft, "the iPhone layout opens the same draft");
    let still = draft.find("still").expect("the word");
    s.select(still..still + 5);
    s.click("find-word");
    s.wait(5000, reviewing);
    check(s.bounds(("accept", 0usize)).is_none(), "iPhone draws no inline controls");
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

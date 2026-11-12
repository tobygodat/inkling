//! Headless proof: the offscreen renderer and in-app input dispatch. No window is shown, nothing
//! takes focus, and no event reaches the system event tap.
use super::*;
use gpui_kit::test::TestWindowExt as _;
use inkling_core::feedback::{Delivery, FeedbackId, Outcome, Record, SendError, Status};
use inkling_core::{AiEvent, Build, BuildNumber, MemoryKeys, Paths, Provider, Review, Revision, Secret};
use inkling_ui::Restart;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
        Self::open_with(dir, form, Arc::default(), Build::with_records(None, []))
    }

    /// `keys` outlives the view, so a relaunch finds what was saved, as it would in the Keychain.
    fn open_with(dir: &Path, form: FormFactor, keys: Arc<MemoryKeys>, build: Build) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(gpui_kit::assets::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let (window, view) =
            cx.update(|cx| open(cx, form, false, false, Box::new(keys), build)).expect("open a headless window");
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
    s.click("open-keys");
    check(s.view.read_with(&s.cx, |v, _| v.key_screen_provider()) == Some(Provider::Claude), "clicking the provider in the footer opens the key screen");
    s.press("escape");

    drop(s);
    let mut relaunch = Snap::open(dir);
    check(relaunch.text() == expected, "a relaunch shows the saved text");
    drop(relaunch);
    phone(dir, &expected);
    feedback(dir);
}

/// The same views laid out for iPhone: no inline controls, a tap selects, a sheet acts.
fn phone(dir: &Path, draft: &str) {
    let mut s = Snap::open_as(dir, FormFactor::Phone);
    check(s.text() == draft, "the iPhone layout opens the same draft");
    let still = draft.find("still").expect("the word");
    s.select(still..still + 5);
    s.click("find-word");
    s.wait(5000, reviewing);
    check(s.bounds("open-feedback").is_none(), "with no feedback link the iPhone offers no feedback");
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
    check(feedback.is_some_and(|b| b.size.height >= px(24.)), "the footer offers Feedback as a button, not bare text");

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

//! Headless proof: GPUI Kit's offscreen Metal renderer and its in-app input dispatch.
//! No window is shown, nothing takes focus, and no event reaches the system event tap.
use super::*;
use gpui_kit::test::TestWindowExt as _;
use std::path::Path;
use std::sync::Arc;

const PARAGRAPHS: [&str; 3] = [
    "The house was quiet in the way it only is before anyone else wakes.",
    "She carried her tea to the window and watched the street come slowly into focus: the bakery lights, a cyclist leaning into the hill, the neighbour's cat picking its way along the top of the fence as if the whole morning had been arranged for its inspection.",
    "Somewhere upstairs a floorboard settled.",
];

struct Snap {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    view: Entity<Inkling>,
    dir: std::path::PathBuf,
}

impl Snap {
    fn open(dir: &Path) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(gpui_kit::assets::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let (window, view) = cx
            .update(|cx| {
                gpui_kit::open_window(window_options(false, false), cx, |window, cx| cx.new(|cx| Inkling::new(window, cx)))
            })
            .expect("open a headless window");
        let mut snap = Snap { cx, window, view, dir: dir.to_path_buf() };
        snap.frame();
        snap
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx.update_window(self.window, |_, window, cx| f(window, cx)).expect("window is open")
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

    /// Real time for the fake model's thread, test time for the app's timers.
    fn wait(&mut self, ms: u64, until: impl Fn(&Inkling) -> bool) {
        for _ in 0..(ms / 10) {
            let done = self.view.read_with(&self.cx, |v, _| until(v));
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            self.cx.advance_clock(Duration::from_millis(16));
            self.frame();
        }
        self.frame();
    }

    fn shot(&mut self, name: &str) -> Vec<u8> {
        self.frame();
        let image = self.cx.capture_screenshot(self.window).expect("offscreen renderer");
        let path = self.dir.join(format!("{name}.png"));
        image.save(&path).expect("write png");
        println!("wrote {}", path.display());
        image.into_raw()
    }

    fn text(&mut self) -> String {
        self.view.read_with(&self.cx, |v, cx| v.text(cx))
    }

    fn assist(&mut self) -> Assist {
        self.view.read_with(&self.cx, |v, _| v.assist.clone())
    }
}

fn check(ok: bool, what: &str) {
    println!("{} {what}", if ok { "ok  " } else { "FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).expect("screenshot dir");
    if std::env::var_os("INKLING_FAKE_AI").is_none() {
        return missing_key(dir);
    }
    let mut s = Snap::open(dir);
    s.shot("01-empty");

    for (i, p) in PARAGRAPHS.iter().enumerate() {
        if i > 0 {
            s.press("enter");
            s.press("enter");
        }
        s.input(p);
    }
    let typed = PARAGRAPHS.join("\n\n");
    check(s.text() == typed, "three paragraphs typed by key events");
    let before_ghost = s.shot("02-typed");

    s.cx.advance_clock(Duration::from_millis(600));
    s.frame();
    check(Paths::mac().load_draft() == typed, "autosave wrote the draft 500 ms after the last edit");

    s.press("cmd-j");
    s.wait(3000, |v| v.assist.proposal().is_some_and(|p| p.text.split_whitespace().count() >= 7));
    check(matches!(s.assist(), Assist::Streaming(_)), "Cmd-J streams a proposal");
    check(s.text() == typed, "the document is unchanged while streaming");
    let streaming = s.shot("03-continue-streaming");
    let cursor_top = s.view.read_with(&s.cx, |v, cx| {
        let state = v.page.read(cx);
        let at = state.cursor();
        state.range_to_bounds(&(at..at)).map(|b| b.origin.y).unwrap_or_default()
    });
    let scale = s.with(|w, _| w.scale_factor());
    let rows = (f32::from(cursor_top) * scale) as usize;
    let width = (900. * scale) as usize * 4;
    let same = before_ghost[..rows * width] == streaming[..rows * width];
    check(rows > 0 && same, &format!("the overlay matches the editor pixel for pixel above the cursor line ({rows} device rows)"));

    s.wait(5000, |v| matches!(v.assist, Assist::Ready(_)));
    s.press("tab");
    let accepted = format!("{typed}{}", inkling_core::store::FAKE_TEXT);
    check(s.text() == accepted, "Tab accepts the proposal into the document");
    s.shot("04-continue-accepted");

    s.press("cmd-z");
    check(s.text() == typed, "Cmd-Z undoes the accepted proposal");
    s.press("cmd-shift-z");
    check(s.text() == accepted, "Shift-Cmd-Z redoes it");

    s.press("cmd-j");
    s.wait(400, |v| v.assist.proposal().is_some_and(|p| !p.text.is_empty()));
    s.input("x");
    check(s.assist() == Assist::Idle, "typing dismisses the ghost text");
    s.press("backspace");
    check(s.text() == accepted, "Backspace removes the typed character");

    let start = accepted.find("watched the street").expect("phrase");
    let end = start + "watched the street come slowly into focus".len();
    s.view.update(&mut s.cx, |v, cx| v.page.update(cx, |p, cx| p.set_selected_range(start..end, cx)));
    s.frame();
    s.press("cmd-c");
    let copied = s.cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    check(copied.as_deref() == Some(&accepted[start..end]), "Cmd-C copies the selection");

    s.press("cmd-k");
    s.input("make it slower");
    s.shot("05a-command-bar");
    s.press("enter");
    s.wait(5000, |v| matches!(v.assist, Assist::Ready(_)));
    check(matches!(s.assist(), Assist::Ready(ref p) if p.replace == (start..end)), "Return in the bar rewrites the selection");
    s.shot("05-rewrite-proposal");
    s.press("escape");
    check(s.assist() == Assist::Idle && s.text() == accepted, "Esc dismisses the rewrite and keeps the text");

    s.press("cmd-,");
    s.input("not-a-real-key");
    s.shot("06-settings");
    s.press("escape");

    let page = s.view.read_with(&s.cx, |v, _| v.page.clone());
    let end = accepted.len();
    page.update(&mut s.cx, |p, cx| p.set_selected_range(end..end, cx));
    let marked = s.with(|window, cx| {
        let bounds = page.read(cx).text_bounds().expect("laid out");
        let mut ime = gpui_kit::test::TestInput::new(ElementInputHandler::new(bounds, page.clone()), window);
        ime.compose_text(None, "か", None, cx);
        let marked = ime.marked_text_range(cx);
        ime.commit_text("火", cx);
        marked
    });
    s.frame();
    check(marked.is_some() && s.text() == format!("{accepted}火"), "marked text from an input method composes, then commits");
    s.press("backspace");
    s.cx.advance_clock(Duration::from_millis(600));
    s.frame();

    drop(s);
    let mut relaunch = Snap::open(dir);
    check(relaunch.text() == accepted, "a relaunch shows the saved draft");
}

fn missing_key(dir: &Path) {
    let mut s = Snap::open(dir);
    s.input("A draft with no key yet.");
    s.press("cmd-j");
    let on_claude = s.view.read_with(&s.cx, |v, _| matches!(&v.panel, Panel::Settings(f) if f.provider == Provider::Claude));
    check(on_claude, "a missing key opens settings on that provider");
    s.shot("07-missing-key");
}

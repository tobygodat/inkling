//! Headless proof: drives the real view with in-app key and mouse events on GPUI's test
//! platform, renders each state with Metal offscreen, and checks what the slice promises.
//! No window reaches the window server, so nothing takes focus. Keys stay in memory.

use anyhow::{Result, ensure};
use gpui::{
    AnyWindowHandle, AppContext, EntityInputHandler, HeadlessAppContext, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, WindowHandle, point,
    px, size,
};
use inkling_core::{Assist, MemoryKeys, Paths, Provider, store::FAKE_TEXT};
use inkling_gpui_fast::app::{Inkling, init};
use std::{path::PathBuf, sync::Arc, time::Duration};

const PARAGRAPHS: &str = "The morning came in grey and slow, the way it does at the end of October when the light seems to forget what it is for. I sat at the kitchen table with a cup of tea going cold and tried to remember the last thing I had written that I liked.\n\nNothing came.\n\nSo I wrote about the kettle instead. ";

struct Run {
    cx: HeadlessAppContext,
    win: WindowHandle<Inkling>,
    out: PathBuf,
}

impl Run {
    fn any(&self) -> AnyWindowHandle {
        self.win.into()
    }

    fn draw(&mut self) {
        self.cx.run_until_parked();
        let w = self.any();
        self.cx.update_window(w, |_, window, cx| window.draw(cx).clear(cx)).unwrap();
    }

    fn key(&mut self, k: &'static str) {
        let ks = Keystroke::parse(k).unwrap();
        self.press(ks);
    }

    fn press(&mut self, ks: Keystroke) {
        let w = self.any();
        self.cx.update_window(w, |_, window, cx| window.dispatch_keystroke(ks, cx)).unwrap();
        self.draw();
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\n' => self.key("enter"),
                ' ' => self.key("space"),
                c => {
                    let mut ks = Keystroke::parse("a").unwrap();
                    ks.key = c.to_lowercase().to_string();
                    ks.key_char = Some(c.to_string());
                    self.press(ks);
                }
            }
        }
    }

    fn event(&mut self, e: PlatformInput) {
        let w = self.any();
        self.cx.update_window(w, |_, window, cx| window.dispatch_event(e, cx)).unwrap();
        self.draw();
    }

    fn shot(&mut self, name: &str) -> Result<()> {
        self.draw();
        let image = self.cx.capture_screenshot(self.any())?;
        image.save(self.out.join(name))?;
        println!("shot {name} {}x{}", image.width(), image.height());
        Ok(())
    }

    fn read<R>(&mut self, f: impl FnOnce(&Inkling, &gpui::App) -> R) -> R {
        let win = self.win;
        self.cx.update(|cx| {
            let view = win.entity(cx).unwrap();
            f(view.read(cx), cx)
        })
    }

    fn doc_text(&mut self) -> String {
        self.read(|v, cx| v.doc.read(cx).text.clone())
    }

    fn with<R>(&mut self, f: impl FnOnce(&mut Inkling, &mut gpui::Window, &mut gpui::Context<Inkling>) -> R) -> R {
        let win = self.win;
        win.update(&mut self.cx, f).unwrap()
    }

    /// Lets the fake model's thread run in real time, then hands its events to the view.
    fn wait_ai(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
        self.cx.advance_clock(Duration::from_millis(16));
        self.draw();
    }

    fn settle_ai(&mut self) -> Result<()> {
        for _ in 0..100 {
            self.wait_ai(50);
            if !self.read(|v, _| v.assist.is_busy()) {
                return Ok(());
            }
        }
        anyhow::bail!("the fake model never finished")
    }
}

fn check(ok: bool, what: &str) -> Result<()> {
    ensure!(ok, "FAILED: {what}");
    println!("ok   {what}");
    Ok(())
}

fn open(cx: &mut HeadlessAppContext, paths: &Paths) -> Result<WindowHandle<Inkling>> {
    cx.open_window(size(px(900.), px(720.)), |window, cx| {
        cx.new(|cx| Inkling::new(paths.clone(), Box::new(MemoryKeys::default()), window, cx))
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: snap <out dir> <ai|missing-key>"));
    let mode = args.next().unwrap_or_else(|| "ai".into());
    ensure!(std::env::var_os("INKLING_DATA_DIR").is_some(), "set INKLING_DATA_DIR to a temp folder");
    std::fs::create_dir_all(&out)?;

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), gpui_platform::current_headless_renderer);
    cx.update(init);
    let paths = Paths::mac();
    let win = open(&mut cx, &paths)?;
    let mut run = Run { cx, win, out };
    run.draw();

    if mode == "missing-key" {
        ensure!(std::env::var_os("INKLING_FAKE_AI").is_none(), "run missing-key without INKLING_FAKE_AI");
        run.type_text("A line to continue from.");
        run.key("cmd-j");
        let form = run.read(|v, _| v.form.as_ref().map(|f| (f.provider, f.missing)));
        check(form == Some((Provider::Claude, true)), "Cmd-J with no key opens settings on Claude")?;
        return run.shot("07-missing-key.png");
    }
    ensure!(std::env::var_os("INKLING_FAKE_AI").is_some(), "run ai mode with INKLING_FAKE_AI=1");

    run.shot("01-empty.png")?;

    run.type_text(PARAGRAPHS);
    check(run.doc_text() == PARAGRAPHS, "typed keystrokes land in the document")?;
    run.shot("02-typed.png")?;

    run.cx.advance_clock(Duration::from_millis(400));
    run.draw();
    check(paths.load_draft().is_empty(), "nothing is saved 400 ms after the last edit")?;
    run.cx.advance_clock(Duration::from_millis(200));
    run.draw();
    check(paths.load_draft() == PARAGRAPHS, "the draft is saved 500 ms after the last edit")?;

    run.key("cmd-j");
    run.wait_ai(330);
    let streaming = run.read(|v, _| match &v.assist {
        Assist::Streaming(p) => Some(p.text.clone()),
        _ => None,
    });
    check(streaming.as_ref().is_some_and(|t| !t.is_empty() && t.len() < FAKE_TEXT.len()), "Continue is mid-stream")?;
    check(run.doc_text() == PARAGRAPHS, "ghost text leaves the document unchanged")?;
    run.shot("03-continue-streaming.png")?;

    run.settle_ai()?;
    run.key("tab");
    check(run.doc_text() == format!("{PARAGRAPHS}{FAKE_TEXT}"), "Tab accepts the continuation into the document")?;
    run.shot("04-continue-accepted.png")?;

    run.key("cmd-z");
    check(run.doc_text() == PARAGRAPHS, "Cmd-Z undoes the accepted continuation")?;
    run.key("cmd-shift-z");
    check(run.doc_text() == format!("{PARAGRAPHS}{FAKE_TEXT}"), "Shift-Cmd-Z redoes it")?;

    run.key("cmd-j");
    run.wait_ai(200);
    run.key("escape");
    check(run.read(|v, _| v.assist == Assist::Idle), "Esc dismisses a streaming Continue")?;
    run.wait_ai(300);
    check(run.read(|v, _| v.assist == Assist::Idle), "a dismissed request sends nothing more")?;

    run.key("cmd-j");
    run.wait_ai(200);
    run.type_text("!");
    check(run.read(|v, _| v.assist == Assist::Idle), "typing dismisses ghost text")?;
    run.key("backspace");

    let target = "Nothing came.";
    let start = run.doc_text().find(target).unwrap();
    run.with(|v, _, cx| v.doc.update(cx, |d, cx| d.select(start..start + target.len(), cx)));
    run.key("cmd-k");
    check(run.read(|v, _| v.bar_open), "Cmd-K with a selection opens the command bar")?;
    run.type_text("make it gentler");
    run.key("enter");
    run.settle_ai()?;
    check(run.read(|v, _| matches!(&v.assist, Assist::Ready(p) if p.replace == (start..start + target.len()))), "Rewrite proposes a replacement for the selection")?;
    run.shot("05-rewrite-proposal.png")?;
    run.key("enter");
    let after = run.doc_text();
    check(after.contains(FAKE_TEXT) && !after.contains(target), "Return accepts the rewrite")?;

    run.key("cmd-,");
    check(run.read(|v, _| v.form.is_some()), "Cmd-Comma opens settings")?;
    run.with(|v, _, cx| v.choose_provider(Provider::Gemini, cx));
    run.type_text("not-a-real-key");
    run.shot("06-settings.png")?;
    run.key("enter");
    check(run.read(|v, _| v.form.is_none() && v.settings.provider == Provider::Gemini), "Save applies the provider")?;
    let saved = std::fs::read_to_string(paths.settings())?;
    check(saved.contains("Gemini") && !saved.contains("not-a-real-key"), "settings.json has the provider and no key")?;

    run.key("cmd-a");
    run.key("cmd-c");
    run.key("right");
    run.key("enter");
    run.key("cmd-v");
    let doubled = run.doc_text();
    check(doubled == format!("{after}\n{after}"), "Cmd-A, Cmd-C, Cmd-V round-trips through the clipboard")?;
    run.key("cmd-z");
    run.key("cmd-z");
    check(run.doc_text() == after, "undo removes the paste and the newline")?;
    for _ in 0..7 {
        run.key("shift-left");
    }
    run.key("cmd-x");
    check(run.doc_text() == after[..after.len() - 7], "Cmd-X cuts the selection")?;
    run.key("cmd-v");
    check(run.doc_text() == after, "Cmd-V puts it back")?;

    let y = px(72. + 14.);
    let down = MouseDownEvent { button: MouseButton::Left, position: point(px(115.), y), click_count: 1, ..Default::default() };
    run.event(PlatformInput::MouseDown(down));
    let drag = MouseMoveEvent { position: point(px(300.), y), pressed_button: Some(MouseButton::Left), modifiers: Modifiers::default() };
    run.event(PlatformInput::MouseMove(drag));
    run.event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: point(px(300.), y), click_count: 1, ..Default::default() }));
    let selected = run.read(|v, cx| {
        let d = v.doc.read(cx);
        d.text[d.selected.clone()].to_string()
    });
    check(selected.starts_with("The morning") && selected.len() > 15, &format!("a mouse drag selects {selected:?}"))?;

    run.key("cmd-right");
    let end = run.doc_text().len();
    run.with(|v, _, cx| v.doc.update(cx, |d, cx| d.select(end..end, cx)));
    run.with(|v, window, cx| {
        v.doc.update(cx, |d, cx| {
            d.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            d.replace_and_mark_text_in_range(None, "nih", None, window, cx);
            d.replace_text_in_range(None, "你好", window, cx);
        })
    });
    check(run.doc_text().ends_with("你好") && !run.doc_text().contains("nih"), "marked text is replaced by the committed IME text")?;

    run.cx.advance_clock(Duration::from_millis(600));
    run.draw();
    let last = run.doc_text();
    let again = open(&mut run.cx, &paths)?;
    let reloaded = run.cx.update(|cx| again.entity(cx).unwrap().read(cx).doc.read(cx).text.clone());
    check(reloaded == last, "a relaunch shows the same text")?;
    Ok(())
}

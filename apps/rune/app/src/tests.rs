use super::*;
use inkling_core::store::FAKE_TEXT;
use rune::{Env, Ime, Mods, Runtime};

const DRAFT: &str = "The kettle sang.";

fn app() -> Runtime<State> {
    // SAFETY: every test sets the same value, before any thread reads it.
    unsafe { std::env::set_var("INKLING_FAKE_AI", "1") };
    let dir = std::env::temp_dir().join(format!("inkling-rune-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    let paths = Paths { dir };
    paths.save_draft(DRAFT).unwrap();
    let mut rt = Runtime::new(state(paths), view::view, (900.0, 680.0), Env { system_dark: false, scale: 2.0 });
    rt.frame(0.0);
    rt
}

fn key(rt: &mut Runtime<State>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(1.0 / 60.0);
}

fn cmd(c: char) -> (Key, Mods) {
    (Key::Command(c), Mods { cmd: true, ..Mods::default() })
}

fn wait_ready(rt: &mut Runtime<State>) {
    for _ in 0..400 {
        drain(&mut rt.state);
        rt.frame(1.0 / 60.0);
        if matches!(rt.state.assist, Assist::Ready(_)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no proposal arrived: {:?}", rt.state.assist);
}

#[test]
fn marked_text_shows_without_entering_the_draft_until_committed() {
    let mut rt = app();
    rt.ime(Ime::Preedit("か".into(), Some((0, 3))));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.doc.text, DRAFT);
    assert!(rt.state.doc.preedit.is_some());
    rt.ime(Ime::Commit("課".into()));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.doc.text, format!("{DRAFT}課"));
}

#[test]
fn esc_drops_the_suggestion_and_any_late_words() {
    let mut rt = app();
    let (k, m) = cmd('j');
    key(&mut rt, k, m);
    assert!(rt.state.assist.is_busy());
    std::thread::sleep(Duration::from_millis(100));
    key(&mut rt, Key::Escape, Mods::default());
    std::thread::sleep(Duration::from_millis(200));
    drain(&mut rt.state);
    assert_eq!(rt.state.assist, Assist::Idle);
    assert_eq!(rt.state.doc.text, DRAFT);
}

#[test]
fn typing_dismisses_the_suggestion_and_still_types() {
    let mut rt = app();
    let (k, m) = cmd('j');
    key(&mut rt, k, m);
    wait_ready(&mut rt);
    key(&mut rt, Key::Text("!".into()), Mods::default());
    assert_eq!(rt.state.assist, Assist::Idle);
    assert_eq!(rt.state.doc.text, format!("{DRAFT}!"));
}

#[test]
fn tab_accepts_and_one_undo_takes_it_back() {
    let mut rt = app();
    let (k, m) = cmd('j');
    key(&mut rt, k, m);
    wait_ready(&mut rt);
    key(&mut rt, Key::Tab, Mods::default());
    assert_eq!(rt.state.doc.text, format!("{DRAFT}{FAKE_TEXT}"));
    let (k, m) = cmd('z');
    key(&mut rt, k, m);
    assert_eq!(rt.state.doc.text, DRAFT);
}

#[test]
fn rewrite_replaces_the_selection_on_return() {
    let mut rt = app();
    rt.state.doc.set_selection(4, 10);
    let (k, m) = cmd('k');
    key(&mut rt, k, m);
    assert!(rt.state.bar.is_some());
    for c in "tighten".chars() {
        key(&mut rt, Key::Text(c.to_string()), Mods::default());
    }
    key(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.bar.is_none());
    wait_ready(&mut rt);
    assert_eq!(rt.state.assist.proposal().unwrap().replace, 4..10);
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.doc.text, format!("The {FAKE_TEXT} sang."));
}

#[test]
fn cmd_k_without_a_selection_continues() {
    let mut rt = app();
    let (k, m) = cmd('k');
    key(&mut rt, k, m);
    assert!(rt.state.bar.is_none());
    assert_eq!(rt.state.assist.proposal().unwrap().replace, DRAFT.len()..DRAFT.len());
}

#[test]
fn copy_and_cut_write_the_clipboard_and_a_paste_lands_at_the_caret() {
    let mut rt = app();
    let board = rune::clipboard::memory();
    rt.use_clipboard(&board);
    for c in ['a', 'c'] {
        let (k, m) = cmd(c);
        key(&mut rt, k, m);
    }
    assert_eq!(board.text().as_deref().map(String::as_str), Some(DRAFT));
    rt.state.doc.set_selection(0, 4);
    let (k, m) = cmd('x');
    key(&mut rt, k, m);
    assert_eq!(board.text().as_deref().map(String::as_str), Some("The "));
    assert_eq!(rt.state.doc.text, "kettle sang.");
    key(&mut rt, Key::End, Mods::default());
    rt.paste(" The");
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.doc.text, "kettle sang. The");
}

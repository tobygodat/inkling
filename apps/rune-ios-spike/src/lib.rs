//! Throwaway spike: one Rune `text_input` on a `CAMetalLayer`, fed by UIKit's keyboard.
//! The C ABI is `ios/Spike/Spike.h`. Every call runs on the main thread.
//!
//! UIKit speaks UTF-16 offsets into a document that includes the marked (preedit) text. Rune
//! keeps the preedit beside `TextEdit::text`, drawn at the caret. The bridge converts both
//! ways. Each edit is queued on Rune's own input path (`Runtime::key`, `Runtime::ime`) and
//! applied at once with `Runtime::frame(0.0)`, because UIKit reads the text back straight
//! after every call it makes.

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use rune::embed::LayerHost;
use rune::{AppSpec, Color, Cx, El, Ime, Key, KeyEvent, Mods, ScrollSpec, TextEdit, TextStyle, TouchPhase, col, text_input};

const PAPER: Color = Color { r: 0.98, g: 0.97, b: 0.95, a: 1.0 };
const INK: Color = Color { r: 0.13, g: 0.12, b: 0.11, a: 1.0 };
const MUTED: Color = Color { r: 0.6, g: 0.58, b: 0.55, a: 1.0 };
const ACCENT: Color = Color { r: 0.85, g: 0.42, b: 0.18, a: 1.0 };

pub struct State {
    doc: TextEdit,
    insets: [f32; 4],
    viewport_h: f32,
}

fn init() -> State {
    State { doc: TextEdit::new(""), insets: [0.0; 4], viewport_h: 800.0 }
}

fn view(s: &State, _: &mut Cx<State>) -> El {
    let [top, right, bottom, left] = s.insets;
    let field = text_input(&s.doc, TextStyle::new(19.0, 400.0).lh(28.0))
        .key("doc")
        .on_edit(|s: &mut State| &mut s.doc)
        .on_submit(|s: &mut State, _| s.doc.insert("\n"))
        .placeholder("Start writing…", MUTED)
        .caret(ACCENT, ACCENT.alpha(0.25))
        .color(INK)
        .w_full()
        .min_h((s.viewport_h - top - bottom - 48.0).max(28.0));
    col().w_full().h_full().fill(PAPER).child(
        col()
            .key("page")
            .scroll(ScrollSpec { stick_bottom: false })
            .scroll_margin(24.0)
            .grow(1.0)
            .min_h(0.0)
            .pt(top + 24.0)
            .pb(bottom + 24.0)
            .pl(left + 22.0)
            .pr(right + 22.0)
            .child(field),
    )
}

pub struct SpikeHost {
    view: LayerHost<State>,
}

type WakeFn = extern "C" fn(*mut c_void);

struct WakeTarget(WakeFn, *mut c_void);
unsafe impl Send for WakeTarget {}
unsafe impl Sync for WakeTarget {}

fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn with_host<T>(host: *mut SpikeHost, fallback: T, f: impl FnOnce(&mut SpikeHost) -> T) -> T {
    match unsafe { host.as_mut() } {
        Some(host) => guarded(fallback, || f(host)),
        None => fallback,
    }
}

fn str_arg<'a>(s: *const c_char) -> &'a str {
    if s.is_null() { "" } else { unsafe { CStr::from_ptr(s) }.to_str().unwrap_or("") }
}

fn u16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The byte offset in `s` at UTF-16 offset `n`, clamped to a char boundary.
fn byte_at(s: &str, n: usize) -> usize {
    let mut units = 0;
    for (b, c) in s.char_indices() {
        if units >= n {
            return b;
        }
        units += c.len_utf16();
    }
    s.len()
}

fn units_at(s: &str, byte: usize) -> usize {
    u16_len(&s[..s.floor_char_boundary(byte.min(s.len()))])
}

/// What UIKit sees: the text with the preedit spliced in at the caret, the marked range, and
/// the selection, all in UTF-16 offsets of that document.
struct DocView {
    text: String,
    marked: Option<(usize, usize)>,
    selection: (usize, usize),
}

fn doc_view(edit: &TextEdit) -> DocView {
    let (a, b) = edit.range();
    match &edit.preedit {
        Some((p, cursor)) if !p.is_empty() => {
            let at = edit.focus.min(edit.text.len());
            let text = format!("{}{}{}", &edit.text[..at], p, &edit.text[at..]);
            let start = units_at(&edit.text, at);
            let end = start + u16_len(p);
            let (ca, cb) = cursor.map_or((p.len(), p.len()), |c| c);
            DocView { text, marked: Some((start, end)), selection: (start + units_at(p, ca), start + units_at(p, cb)) }
        }
        _ => DocView { text: edit.text.clone(), marked: None, selection: (units_at(&edit.text, a), units_at(&edit.text, b)) },
    }
}

impl SpikeHost {
    fn edit(&self) -> &TextEdit {
        &self.view.runtime().state.doc
    }

    fn key(&mut self, key: Key) {
        let rt = self.view.runtime_mut();
        rt.key(KeyEvent { key, mods: Mods::default() });
        rt.frame(0.0);
    }

    fn ime(&mut self, ev: Ime) {
        let rt = self.view.runtime_mut();
        rt.ime(ev);
        rt.frame(0.0);
    }

    fn composing(&self) -> bool {
        self.edit().preedit.as_ref().is_some_and(|(p, _)| !p.is_empty())
    }

    fn type_text(&mut self, s: &str) {
        if self.composing() {
            self.ime(Ime::Commit(s.to_owned()));
            return;
        }
        // UIKit sends Return as "\n" and a pasted or dictated run as one string.
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                self.key(Key::Enter);
            }
            if !line.is_empty() {
                self.key(Key::Text(line.to_owned()));
            }
        }
    }
}

/// # Safety
/// `layer` is a `CAMetalLayer *` that outlives the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_new(
    layer: *mut c_void,
    width: f32,
    height: f32,
    scale: f32,
    wake: Option<WakeFn>,
    wake_ctx: *mut c_void,
) -> *mut SpikeHost {
    guarded(std::ptr::null_mut(), || {
        let target = Arc::new(wake.map(|f| WakeTarget(f, wake_ctx)));
        let waker = Arc::new(move || {
            if let Some(WakeTarget(f, ctx)) = target.as_ref() {
                f(*ctx);
            }
        });
        let spec = AppSpec::new("Spike", (width, height), init, view);
        let state = init();
        match unsafe { LayerHost::from_core_animation_layer(layer, &spec, state, (width, height), scale, false, waker) } {
            Ok(view) => Box::into_raw(Box::new(SpikeHost { view })),
            Err(e) => {
                eprintln!("spike_new: {e}");
                std::ptr::null_mut()
            }
        }
    })
}

/// # Safety
/// `host` came from `spike_new` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_free(host: *mut SpikeHost) {
    if !host.is_null() {
        guarded((), || drop(unsafe { Box::from_raw(host) }));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_resize(host: *mut SpikeHost, width: f32, height: f32, scale: f32) {
    with_host(host, (), |h| {
        h.view.resize((width, height), scale);
        h.view.runtime_mut().update(|s, _| s.viewport_h = height);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_set_insets(host: *mut SpikeHost, top: f32, right: f32, bottom: f32, left: f32) {
    with_host(host, (), |h| h.view.runtime_mut().update(|s, _| s.insets = [top, right, bottom, left]));
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_frame(host: *mut SpikeHost, wake_after: *mut f64) -> bool {
    with_host(host, false, |h| {
        let out = h.view.frame();
        if !wake_after.is_null() {
            unsafe { *wake_after = out.wake_after_secs.unwrap_or(-1.0) };
        }
        out.animating
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_touch(host: *mut SpikeHost, id: u64, phase: u32, x: f32, y: f32, time: f64) {
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Cancelled,
    };
    with_host(host, (), |h| {
        let rt = h.view.runtime_mut();
        rt.touch(id, phase, x, y, time);
        rt.frame(0.0);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_focused(host: *mut SpikeHost) -> bool {
    with_host(host, false, |h| h.edit().focused)
}

// Tier 1: UIKeyInput.

/// # Safety
/// `text` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_insert_text(host: *mut SpikeHost, text: *const c_char) {
    let text = str_arg(text);
    with_host(host, (), |h| h.type_text(text));
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_delete_backward(host: *mut SpikeHost) {
    with_host(host, (), |h| h.key(Key::Backspace));
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_has_text(host: *mut SpikeHost) -> bool {
    with_host(host, false, |h| !h.edit().text.is_empty())
}

// Tier 2: UITextInput. Offsets are UTF-16 units of the document UIKit sees.

/// The document as a new C string; free it with `spike_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn spike_text(host: *mut SpikeHost) -> *mut c_char {
    with_host(host, std::ptr::null_mut(), |h| CString::new(doc_view(h.edit()).text.replace('\0', "")).map_or(std::ptr::null_mut(), CString::into_raw))
}

/// # Safety
/// `s` came from `spike_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// A counter-free change stamp: UIKit's input delegate is told when it moves without UIKit
/// having asked (a tap moved the caret, Rune handled a key itself).
#[unsafe(no_mangle)]
pub extern "C" fn spike_revision(host: *mut SpikeHost) -> u64 {
    use std::hash::{Hash, Hasher};
    with_host(host, 0, |h| {
        let e = h.edit();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&e.text, e.anchor, e.focus, &e.preedit).hash(&mut hasher);
        hasher.finish()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_selection(host: *mut SpikeHost, start: *mut usize, end: *mut usize) {
    with_host(host, (), |h| {
        let (a, b) = doc_view(h.edit()).selection;
        unsafe {
            *start = a;
            *end = b;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_set_selection(host: *mut SpikeHost, start: usize, end: usize) {
    with_host(host, (), |h| {
        if h.composing() {
            return;
        }
        h.view.runtime_mut().update(|s, _| {
            let (a, b) = (byte_at(&s.doc.text, start), byte_at(&s.doc.text, end));
            s.doc.set_selection(a, b);
        });
        h.view.runtime_mut().frame(0.0);
    });
}

/// Returns false when nothing is marked.
#[unsafe(no_mangle)]
pub extern "C" fn spike_marked_range(host: *mut SpikeHost, start: *mut usize, end: *mut usize) -> bool {
    with_host(host, false, |h| match doc_view(h.edit()).marked {
        Some((a, b)) => {
            unsafe {
                *start = a;
                *end = b;
            }
            true
        }
        None => false,
    })
}

/// Replaces document range `start..end` with `text`, as autocorrect and predictive text do.
///
/// # Safety
/// `text` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_replace(host: *mut SpikeHost, start: usize, end: usize, text: *const c_char) {
    let text = str_arg(text);
    with_host(host, (), |h| {
        if h.composing() {
            h.ime(Ime::Commit(text.to_owned()));
            return;
        }
        h.view.runtime_mut().update(|s, _| {
            let (a, b) = (byte_at(&s.doc.text, start), byte_at(&s.doc.text, end));
            s.doc.set_selection(a, b);
        });
        if text.is_empty() {
            if start != end {
                h.key(Key::Backspace);
            }
        } else {
            h.type_text(text);
        }
    });
}

/// `sel_start..sel_end` is the cursor inside `text`, in its UTF-16 units. Empty `text` ends
/// the composition.
///
/// # Safety
/// `text` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spike_set_marked(host: *mut SpikeHost, text: *const c_char, sel_start: usize, sel_end: usize) {
    let text = str_arg(text);
    with_host(host, (), |h| {
        if !h.composing() && !text.is_empty() {
            let (a, b) = h.edit().range();
            if a != b {
                h.key(Key::Backspace);
            }
        }
        let cursor = (byte_at(text, sel_start), byte_at(text, sel_end));
        h.ime(Ime::Preedit(text.to_owned(), Some(cursor)));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn spike_unmark(host: *mut SpikeHost) {
    with_host(host, (), |h| {
        if let Some((p, _)) = h.edit().preedit.clone().filter(|(p, _)| !p.is_empty()) {
            h.ime(Ime::Commit(p));
        }
    });
}

/// The live caret's rect in view points (`[x, y, w, h]`), as Rune last drew it. Rune exposes
/// no geometry for any other text position.
#[unsafe(no_mangle)]
pub extern "C" fn spike_caret_rect(host: *mut SpikeHost, out: *mut f32) -> bool {
    with_host(host, false, |h| match h.view.runtime().ime_area {
        Some(r) => {
            unsafe { std::ptr::copy_nonoverlapping(r.as_ptr(), out, 4) };
            true
        }
        None => false,
    })
}

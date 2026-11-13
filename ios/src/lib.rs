//! Inkling's iPhone app: the shared views (`inkling-ui`) laid out for the phone, in the Swift
//! host under `app/`. The host owns the window, the safe areas and the keyboard; this crate
//! owns everything drawn inside the GPUI view.

extern crate gpui_mobile;

use futures_channel::oneshot;
use gpui_kit::*;
use inkling_core::{Build, Paths};
use inkling_ui::{BookHost, FormFactor, Inkling};
use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Registers the app with gpui-mobile. The host calls it once, before `gpui_ios_run_demo`.
///
/// # Safety
/// `data_dir` is a NUL-terminated UTF-8 path that outlives the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inkling_ios_register(data_dir: *const c_char) {
    // The typing probe's times are logged at Debug, by gpui-mobile.
    let level = if std::env::var_os("INKLING_PROBE_TYPING").is_some() { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    let _ = log::set_logger(&NsLog).map(|()| log::set_max_level(level));
    std::panic::set_hook(Box::new(|info| nslog(&format!("INKLING PANIC: {info}"))));
    let dir = PathBuf::from(unsafe { CStr::from_ptr(data_dir) }.to_string_lossy().into_owned());
    gpui_mobile::ios::ffi::set_asset_source(inkling_ui::Assets);
    gpui_mobile::ios::ffi::set_app_callback(Box::new(move |cx: &mut App| {
        gpui_kit::init(cx);
        inkling_ui::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let keys = inkling_core::default_keys();
            let app = cx.new(|cx| {
                let mut inkling = Inkling::new(FormFactor::Phone, Paths { dir }, keys, Build::running(), window, cx);
                inkling.set_book_host(book_host(), cx);
                inkling
            });
            let page = app.read(cx).page().focus_handle(cx);
            window.on_focus_in(&page, cx, |_, _| system_selection_ui(true)).detach();
            window.on_focus_out(&page, cx, |_, _, _| system_selection_ui(false)).detach();
            app
        })
        .expect("open the Inkling window");
    }));
}

// The Swift host's side of the library (`app/Inkling/Books.swift`). Each runs on the main thread.
unsafe extern "C" {
    /// Shows the document picker for EPUBs and PDFs and answers `inkling_ios_picked(token, ..)`.
    fn inkling_host_pick_books(token: u64);
    /// Shows the folder picker, remembers the folder as a bookmark and answers
    /// `inkling_ios_picked(token, ..)` with it.
    fn inkling_host_pick_folder(token: u64);
    /// The folders to search, one path per line, in a `malloc`ed string: the app's Documents and
    /// the remembered folders, which stay open until the next call.
    fn inkling_host_book_folders() -> *mut c_char;
    fn free(ptr: *mut c_void);
}

/// Pickers still open, by the token their answer carries.
static PICKING: Mutex<Vec<(u64, oneshot::Sender<Vec<PathBuf>>)>> = Mutex::new(Vec::new());
static NEXT_PICK: AtomicU64 = AtomicU64::new(1);

/// The library's host on iPhone: the system document picker for Add, and for Find books the app's
/// Documents plus the folders the reader allowed.
fn book_host() -> BookHost {
    BookHost {
        folders: Box::new(|_| {
            let list = unsafe { inkling_host_book_folders() };
            if list.is_null() {
                return Vec::new();
            }
            let folders = lines(unsafe { CStr::from_ptr(list) });
            unsafe { free(list.cast()) };
            folders
        }),
        pick: Box::new(|cx| ask_host(cx, |token| unsafe { inkling_host_pick_books(token) })),
        add_folder: Some(Box::new(|cx| ask_host(cx, |token| unsafe { inkling_host_pick_folder(token) }))),
    }
}

/// Asks the host for paths and resolves when it answers through `inkling_ios_picked`.
fn ask_host(cx: &mut App, ask: impl FnOnce(u64)) -> Task<Vec<PathBuf>> {
    let (tx, rx) = oneshot::channel();
    let token = NEXT_PICK.fetch_add(1, Ordering::Relaxed);
    PICKING.lock().unwrap_or_else(|e| e.into_inner()).push((token, tx));
    ask(token);
    cx.spawn(async move |_| rx.await.unwrap_or_default())
}

fn lines(text: &CStr) -> Vec<PathBuf> {
    text.to_string_lossy().lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

/// The host's answer to a picker: the paths chosen, one per line, or null when cancelled.
///
/// # Safety
/// `paths` is null or a NUL-terminated UTF-8 string that outlives the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inkling_ios_picked(token: u64, paths: *const c_char) {
    let chosen = if paths.is_null() { Vec::new() } else { lines(unsafe { CStr::from_ptr(paths) }) };
    let mut picking = PICKING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = picking.iter().position(|(t, _)| *t == token) {
        let (_, tx) = picking.remove(at);
        let _ = tx.send(chosen);
    }
}

/// UIKit draws the caret, the handles and the edit menu for the page. The kit's own inputs
/// draw theirs, so the page turns UIKit's on only while it has focus.
fn system_selection_ui(enabled: bool) {
    use gpui_mobile::ios::ffi::{gpui_ios_get_window, gpui_ios_set_system_selection_ui};
    unsafe { gpui_ios_set_system_selection_ui(gpui_ios_get_window(), enabled) };
}

struct NsLog;

impl log::Log for NsLog {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        nslog(&format!("[{}] {}: {}", record.level(), record.target(), record.args()));
    }
    fn flush(&self) {}
}

fn nslog(msg: &str) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe extern "C" {
        fn NSLog(fmt: *mut AnyObject, ...);
    }
    let msg = std::ffi::CString::new(msg.replace('\0', "")).unwrap_or_default();
    unsafe {
        let ns: *mut AnyObject = msg_send![class!(NSString), alloc];
        let ns: *mut AnyObject = msg_send![ns, initWithUTF8String: msg.as_ptr()];
        let fmt: *mut AnyObject = msg_send![class!(NSString), alloc];
        let fmt: *mut AnyObject = msg_send![fmt, initWithUTF8String: c"%@".as_ptr()];
        NSLog(fmt, ns);
    }
}

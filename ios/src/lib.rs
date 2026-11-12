//! Inkling's iPhone app: the shared views (`inkling-ui`) laid out for the phone, in the Swift
//! host under `app/`. The host owns the window, the safe areas and the keyboard; this crate
//! owns everything drawn inside the GPUI view.

extern crate gpui_mobile;

use gpui_kit::*;
use inkling_core::{Build, Paths};
use inkling_ui::{FormFactor, Inkling};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

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
            let app = cx.new(|cx| Inkling::new(FormFactor::Phone, Paths { dir }, keys, Build::running(), window, cx));
            let page = app.read(cx).page().focus_handle(cx);
            window.on_focus_in(&page, cx, |_, _| system_selection_ui(true)).detach();
            window.on_focus_out(&page, cx, |_, _, _| system_selection_ui(false)).detach();
            app
        })
        .expect("open the Inkling window");
    }));
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

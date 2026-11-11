//! Inkling's iPhone app: the shared views (`inkling-ui`) laid out for the phone, in the Swift
//! host under `app/`. The host owns the window, the safe areas and the keyboard; this crate
//! owns everything drawn inside the GPUI view.

extern crate gpui_mobile;

use gpui_kit::*;
use inkling_core::Paths;
use inkling_ui::{FormFactor, Inkling};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;

/// Registers the app with gpui-mobile. The host calls it once, before `gpui_ios_run_demo`.
///
/// # Safety
/// `data_dir` is a NUL-terminated UTF-8 path that outlives the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inkling_ios_register(data_dir: *const c_char) {
    let _ = log::set_logger(&NsLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    std::panic::set_hook(Box::new(|info| nslog(&format!("INKLING PANIC: {info}"))));
    let dir = PathBuf::from(unsafe { CStr::from_ptr(data_dir) }.to_string_lossy().into_owned());
    gpui_mobile::ios::ffi::set_asset_source(gpui_kit::assets::Assets);
    gpui_mobile::ios::ffi::set_app_callback(Box::new(move |cx: &mut App| {
        gpui_kit::init(cx);
        inkling_ui::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| Inkling::new(FormFactor::Phone, Paths { dir }, inkling_core::default_keys(), window, cx))
        })
        .expect("open the Inkling window");
    }));
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

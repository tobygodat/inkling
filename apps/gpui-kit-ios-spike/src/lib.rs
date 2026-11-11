//! GPUI Kit on iPhone spike (TOB-428). One screen: a toggle for native touch selection, a
//! read-only review paragraph with one tappable inline change, and a full-height kit `Textarea`.
//! The Swift host in `ios/` embeds this view through gpui-mobile's child view controller.

extern crate gpui_mobile;

use gpui::{
    App, Context, FontWeight, HighlightStyle, IntoElement, ParentElement, Render, StrikethroughStyle,
    Styled, StyledText, Window, WindowOptions, div, prelude::*, px, relative, rgb, rgba,
};
use gpui_kit::component::input::{Textarea, TextareaState, set_native_touch_selection};
use gpui_kit::component::{Root, Theme, ThemeMode};

const GROUND: u32 = 0xF4F5F1;
const INK: u32 = 0x1C211E;
const MUTED: u32 = 0x7C857E;
const ACCENT: u32 = 0x2F6B4F;
const HAIRLINE: u32 = 0xDDE0DA;
const SERIF: &str = "Iowan Old Style";

const BEFORE: &str = "The morning was ";
const DELETED: &str = "very cold";
const INSERTED: &str = "bitter";
const AFTER: &str = ", and the lake held still under the fog.";

#[derive(Clone, Copy, PartialEq)]
enum Change {
    Pending,
    Accepted,
}

struct Spike {
    page: gpui::Entity<TextareaState>,
    native_selection: bool,
    change: Change,
}

impl Spike {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page = cx.new(|cx| TextareaState::new(window, cx).soft_wrap(true).placeholder("Start writing"));
        Spike { page, native_selection: false, change: Change::Pending }
    }

    fn review(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, highlights, change_range) = match self.change {
            Change::Pending => {
                let del = BEFORE.len()..BEFORE.len() + DELETED.len();
                let ins = del.end..del.end + INSERTED.len();
                let text = format!("{BEFORE}{DELETED}{INSERTED}{AFTER}");
                let highlights = vec![
                    (
                        del.clone(),
                        HighlightStyle {
                            color: Some(rgb(MUTED).into()),
                            strikethrough: Some(StrikethroughStyle { thickness: px(1.5), color: Some(rgb(MUTED).into()) }),
                            ..Default::default()
                        },
                    ),
                    (
                        ins.clone(),
                        HighlightStyle {
                            color: Some(rgb(ACCENT).into()),
                            background_color: Some(rgba(0x2F6B4F26).into()),
                            font_weight: Some(FontWeight::SEMIBOLD),
                            ..Default::default()
                        },
                    ),
                ];
                (text, highlights, del.start..ins.end)
            }
            Change::Accepted => {
                let ins = BEFORE.len()..BEFORE.len() + INSERTED.len();
                let text = format!("{BEFORE}{INSERTED}{AFTER}");
                let highlights = vec![(
                    ins.clone(),
                    HighlightStyle {
                        underline: Some(gpui::UnderlineStyle { thickness: px(1.), color: Some(rgb(ACCENT).into()), wavy: false }),
                        ..Default::default()
                    },
                )];
                (text, highlights, ins)
            }
        };
        let label = match self.change {
            Change::Pending => "Review · tap the change to accept",
            Change::Accepted => "Review · accepted, tap to undo",
        };
        // InteractiveText::on_click never fires on touch: GPUI dispatches a tap's mouse down and up
        // in one call, and InteractiveText only listens for the up on the paint after the down.
        // So the paragraph takes an ordinary click and hit-tests the change's range itself.
        let styled = StyledText::new(text).with_highlights(highlights);
        let layout = styled.layout().clone();
        div()
            .w_full()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(HAIRLINE))
            .child(div().text_size(px(12.)).text_color(rgb(MUTED)).child(label))
            .child(
                div()
                    .id("review")
                    .pt_1()
                    .font_family(SERIF)
                    .text_size(px(18.))
                    .line_height(relative(1.6))
                    .text_color(rgb(INK))
                    .child(styled)
                    .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                        let Ok(ix) = layout.index_for_position(event.position()) else { return };
                        if !change_range.contains(&ix) {
                            return;
                        }
                        this.change = match this.change {
                            Change::Pending => Change::Accepted,
                            Change::Accepted => Change::Pending,
                        };
                        log::info!("spike: review change is now {}", if this.change == Change::Accepted { "accepted" } else { "pending" });
                        cx.notify();
                    })),
            )
    }
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let native = self.native_selection;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(GROUND))
            .text_color(rgb(INK))
            .child(
                div()
                    .id("native-toggle")
                    .w_full()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(HAIRLINE))
                    .text_size(px(13.))
                    .text_color(rgb(MUTED))
                    .child(format!("Native touch selection: {}  (tap to switch)", if native { "ON" } else { "OFF" }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.native_selection = !this.native_selection;
                        set_native_touch_selection(this.native_selection, cx);
                        log::info!("spike: native touch selection {}", this.native_selection);
                        cx.notify();
                    })),
            )
            .child(self.review(cx))
            .child(
                div().flex_1().min_h_0().w_full().px_2().child(
                    Textarea::new(&self.page)
                        .appearance(false)
                        .h_full()
                        .font_family(SERIF)
                        .text_size(px(18.))
                        .line_height(relative(1.6))
                        .text_color(rgb(INK)),
                ),
            )
    }
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

#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_register_app() {
    let _ = log::set_logger(&NsLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    std::panic::set_hook(Box::new(|info| nslog(&format!("GPUI PANIC: {info}"))));
    gpui_mobile::ios::ffi::set_app_callback(Box::new(|cx: &mut App| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Light, None, cx);
        cx.open_window(WindowOptions::default(), |window, cx| {
            let spike = cx.new(|cx| Spike::new(window, cx));
            cx.new(|cx| Root::new(spike, window, cx))
        })
        .expect("open the spike view");
    }));
}

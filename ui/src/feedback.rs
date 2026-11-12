//! The feedback screen: "What should change?", Send, and "Feedback & fixes", the list of what this
//! device sent with each item's status in the running build. It renders `inkling_core::Desk` and
//! keeps no copy of it. Nothing here is reachable until a feedback link is saved.

use crate::app::{Inkling, footer_button, ghost_button, primary_button};
use crate::FormFactor;
use gpui_kit::component::input::Textarea;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::BuildNumber;
use inkling_core::feedback::{Delivery, SendError, SentFrom, Status};

/// Open while `Inkling::feedback_screen` is `Some`.
pub(crate) struct FeedbackScreen {
    /// "page" or "review": the screen open when Feedback was pressed, for the body's `screen`.
    pub from: &'static str,
}

/// Emitted when the writer presses Restart. The Mac host relaunches the bundle on disk and quits.
pub struct Restart;

impl EventEmitter<Restart> for Inkling {}

impl Inkling {
    pub fn open_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.feedback_on {
            return;
        }
        let from = if self.revision.review().is_some() { "review" } else { "page" };
        self.key_screen = None;
        self.feedback_screen = Some(FeedbackScreen { from });
        if self.form == FormFactor::Mac {
            self.feedback_field.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// The field keeps its text, so text typed and not sent, or sent and failed, is there when
    /// the screen opens again.
    pub fn close_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.feedback_screen = None;
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        if self.revision.review().is_none() {
            self.focus_page(window, cx);
        }
        cx.notify();
    }

    /// Send, or Try again after a failure. Cmd-Return in the field does the same.
    pub fn send_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = &self.feedback_screen else { return };
        let text = self.feedback_field.read(cx).value().to_string();
        let size = window.viewport_size();
        let from = SentFrom {
            form_factor: self.form.id(),
            window: (f32::from(size.width) as u32, f32::from(size.height) as u32),
            screen: screen.from,
        };
        let Some(post) = self.feedback.send(&text, self.keys.as_ref(), from) else { return cx.notify() };
        let run = cx.background_spawn(async move { post.run() });
        self.sending = cx.spawn_in(window, async move |this, cx| {
            let result = run.await;
            let _ = this.update_in(cx, |this, window, cx| this.feedback_settled(result, window, cx));
        });
        cx.notify();
    }

    fn feedback_settled(&mut self, result: Result<(), SendError>, window: &mut Window, cx: &mut Context<Self>) {
        let sent = result.is_ok();
        if let Err(e) = self.feedback.settle(result) {
            self.notice = Some(format!("Sent, but couldn't save the list ({e}).").into());
        }
        if sent {
            self.feedback_field.update(cx, |field, cx| field.set_value("", window, cx));
        }
        if self.form == FormFactor::Mac && self.feedback_screen.is_some() {
            self.feedback_field.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// The host reports the build on disk (the Mac, every few seconds). Restart is offered only
    /// while it is newer than the running build. The iPhone never calls this.
    pub fn set_installed_build(&mut self, installed: Option<BuildNumber>, cx: &mut Context<Self>) {
        if self.installed != installed {
            self.installed = installed;
            cx.notify();
        }
    }

    /// The build Restart would start. `None` on a development build, which has no number.
    pub(crate) fn update_ready(&self) -> Option<BuildNumber> {
        let running = self.feedback.build().number?;
        self.installed.filter(|installed| *installed > running)
    }

    /// Saves the draft now, since the autosave may be pending, then asks the host to restart. If
    /// the save fails, the footer says so and nothing restarts.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        match self.paths.save_draft(&self.text(cx)) {
            Ok(()) => cx.emit(Restart),
            Err(e) => {
                self.saved = crate::Saved::Failed(e.to_string());
                cx.notify();
            }
        }
    }

    pub fn feedback(&self) -> &inkling_core::Desk {
        &self.feedback
    }

    pub fn feedback_field(&self) -> &Entity<gpui_kit::component::input::TextareaState> {
        &self.feedback_field
    }

    /// "Build 43 is installed. Restart", in the Mac footer and at the top of the feedback screen.
    pub(crate) fn render_restart(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let c = self.theme().c;
        let build = self.update_ready()?;
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(c.muted)
                .child(format!("Build {} is installed.", build.0))
                .child(
                    footer_button(&c, "restart", "Restart")
                        .on_click(cx.listener(|this, _, _, cx| this.restart(cx)))
                        .test_support(),
                ),
        )
    }

    pub(crate) fn render_feedback(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let phone = self.form == FormFactor::Phone;
        let delivery = self.feedback.delivery();
        let message = match delivery {
            Delivery::Idle => None,
            Delivery::Sending => Some("Sending…".to_string()),
            Delivery::Stopped(why) => Some(why.to_string()),
            Delivery::Failed(why) => Some(why.to_string()),
        };
        let button = match delivery {
            Delivery::Sending => "Sending…",
            Delivery::Failed(_) => "Try again",
            _ => "Send",
        };
        let typing = self.feedback_field.read(cx).focus_handle(cx).is_focused(window);
        let label = |text: &'static str| div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(c.muted).child(text);
        let items: Vec<AnyElement> = self
            .feedback
            .items()
            .enumerate()
            .map(|(i, item)| {
                let (state, detail) = match item.status {
                    Status::Pending => ("Waiting for a build".to_string(), None),
                    Status::Done { summary, seconds_to_ship } => (shipped_in(seconds_to_ship), Some(summary.to_string())),
                    Status::Declined { reason } => ("Declined".to_string(), Some(reason.to_string())),
                };
                let pending = matches!(item.status, Status::Pending);
                let line = match &detail {
                    Some(detail) => format!("{state} · {detail}"),
                    None => state.clone(),
                };
                let emphasis = HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    color: Some(c.ink.into()),
                    ..Default::default()
                };
                let highlights = if pending { vec![] } else { vec![(0..state.len(), emphasis)] };
                div()
                    .id(("feedback-item", i))
                    .min_h(m.control.max(px(44.)))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .border_t_1()
                    .border_color(c.hairline)
                    .child(div().text_size(px(15.)).line_height(relative(1.4)).line_clamp(3).child(item.sent.text.clone()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(relative(1.4))
                            .line_clamp(2)
                            .text_color(c.muted)
                            .child(StyledText::new(line).with_highlights(highlights)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .collect();
        let empty = items.is_empty();
        div().id("feedback").size_full().flex().flex_col().items_center().overflow_y_scroll().child(
            div()
                .w_full()
                .max_w(px(560.))
                .px(m.gutter)
                .pt_6()
                .pb_8()
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            ghost_button(&c, "feedback-back", "‹ Back to the page")
                                .h(m.control.max(px(32.)))
                                .on_click(cx.listener(|this, _, window, cx| this.close_feedback(window, cx)))
                                .test_support(),
                        )
                        .when(phone && typing, |row| {
                            row.child(
                                ghost_button(&c, "done", "Done")
                                    .h(m.control)
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.ink)
                                    .on_click(cx.listener(|this, _, window, cx| this.done_typing(window, cx)))
                                    .test_support(),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .font_family(self.theme().body.face)
                                .text_size(px(30.))
                                .line_height(relative(1.2))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("What should change?"),
                        )
                        .children(self.render_restart(cx).map(|r| div().text_size(px(13.)).child(r))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .id("feedback-field")
                                .min_h(px(96.))
                                .px(px(14.))
                                .py(px(10.))
                                .bg(c.surface)
                                .border_1()
                                .border_color(c.hairline)
                                .rounded(px(12.))
                                .text_size(px(15.))
                                .line_height(relative(1.5))
                                .child(Textarea::new(&self.feedback_field).appearance(false))
                                .test_support(),
                        )
                        .children(message.map(|m| div().id("feedback-message").text_size(px(13.)).text_color(c.muted).child(m).test_support()))
                        .child(
                            primary_button(&c, "send-feedback", button, m)
                                .h(if phone { px(52.) } else { px(48.) })
                                .justify_center()
                                .text_size(px(15.))
                                .when(*delivery == Delivery::Sending, |b| b.opacity(0.6))
                                .on_click(cx.listener(|this, _, window, cx| this.send_feedback(window, cx)))
                                .test_support(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(label("Feedback & fixes"))
                        .when(empty, |list| {
                            list.child(div().py(px(12.)).text_size(px(15.)).text_color(c.muted).child("Nothing sent yet."))
                        })
                        .child(div().flex().flex_col().children(items)),
                ),
        )
    }
}

/// "Done in 40 s", "Done in 7 min", "Done in 3 h".
pub(crate) fn shipped_in(seconds: u64) -> String {
    match seconds {
        0..60 => format!("Done in {seconds} s"),
        60..3_600 => format!("Done in {} min", (seconds + 30) / 60),
        _ => format!("Done in {} h", (seconds + 1_800) / 3_600),
    }
}

//! SKETCH: ui/src/feedback.rs (new)
//!
//! The feedback screen: "What should change?", Send, and "Feedback & fixes", the list of what
//! this device sent with each item's status in the running build. It renders
//! `inkling_core::Desk` and keeps no copy of it (docs/CONVENTIONS.md). Built like the key screen
//! (ui/src/keys.rs): a full-window screen with "‹ Back to the page", opened from the footer on
//! Mac and the status row on iPhone, closed with Esc or Back.
//!
//! Hidden until set up (decision 5): nothing here is reachable while `Inkling::feedback_on` is
//! false. The only feedback UI shown then is the link section of the key screen.

use crate::app::{Inkling, ghost_button, primary_button};
use crate::{FormFactor, SERIF, color};
use gpui_kit::*;
use inkling_core::feedback::{Delivery, Item, SentFrom, Status};

/// Open while `Some` on `Inkling::feedback_screen`. Holds only what the screen itself needs to
/// remember: where the writer came from, for the body's `screen`.
pub(crate) struct FeedbackScreen {
    /// "page", "review" or "keys" (the screen open when Feedback was pressed).
    pub from: &'static str,
}

/// Emitted when the writer presses Restart. The Mac host subscribes, relaunches the bundle on
/// disk and quits. `ui/` never reads the platform, so it can't do either itself.
pub struct Restart;
impl EventEmitter<Restart> for Inkling {}

impl Inkling {
    /// Footer "Feedback" (Mac) or status-row "Feedback" (iPhone).
    pub fn open_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // TODO: if !self.feedback_on { return }
        // from = if self.revision.review().is_some() { "review" } else { "page" };
        // self.feedback_screen = Some(FeedbackScreen { from });
        // focus self.feedback_field (Mac); on iPhone wait for a tap, like the page.
        todo!()
    }

    pub fn close_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Same shape as close_keys: drop the screen, put the keyboard away, page focus on Mac.
        // The field keeps its text (it lives on `Inkling`, not on the screen), so text typed
        // and not sent, or sent and failed, is there when the screen opens again.
        todo!()
    }

    /// Send / Try again. Return in the field does the same on Mac (cmd-enter, since the field
    /// is multi-line).
    pub fn send_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // TODO:
        // let Some(screen) = &self.feedback_screen else { return };
        // let text = self.feedback_field.read(cx).value().to_string();
        // let size = window.viewport_size();
        // let from = SentFrom { form_factor: self.form.id(), window: (w as u32, h as u32), screen: screen.from };
        // let Some(post) = self.feedback.send(&text, self.keys.as_ref(), from) else { return cx.notify() };
        // let run = cx.background_spawn(async move { post.run() });
        // self.sending = cx.spawn_in(window, async move |this, cx| {
        //     let result = run.await;
        //     let _ = this.update_in(cx, |this, window, cx| this.feedback_settled(result, window, cx));
        // });
        // cx.notify();
        todo!()
    }

    /// Back from the background task. Success empties the field; failure leaves it as typed.
    fn feedback_settled(&mut self, result: Result<(), inkling_core::feedback::SendError>, window: &mut Window, cx: &mut Context<Self>) {
        // TODO:
        // let sent = result.is_ok();
        // if let Err(e) = self.feedback.settle(result) { self.notice = Some(format!("Sent, but couldn't save the list ({e}).").into()) }
        // if sent { self.feedback_field.update(cx, |f, cx| f.set_value("", window, cx)) }
        // cx.notify();
        todo!()
    }

    /// The host tells the view what build is on disk (Mac: every few seconds). The view offers
    /// Restart only while that is newer than the running build. iPhone never calls this.
    pub fn set_installed_build(&mut self, installed: Option<inkling_core::BuildNumber>, cx: &mut Context<Self>) {
        if self.installed != installed {
            self.installed = installed;
            cx.notify();
        }
    }

    /// The build Restart would start, if any. `None` on a development build (no number).
    pub(crate) fn update_ready(&self) -> Option<inkling_core::BuildNumber> {
        let running = self.feedback.build().number?;
        self.installed.filter(|installed| *installed > running)
    }

    /// Saves the draft now (the 500 ms autosave may be pending), then asks the host to restart.
    /// If the save fails, the footer shows it and nothing restarts.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        // TODO:
        // match self.paths.save_draft(&self.text(cx)) {
        //     Ok(()) => cx.emit(Restart),
        //     Err(e) => { self.saved = Saved::Failed(e.to_string()); cx.notify() }
        // }
        todo!()
    }

    /// For the snap: the desk the screen renders.
    pub fn feedback(&self) -> &inkling_core::Desk {
        &self.feedback
    }

    /// Mac: "Build 313 is installed. [Restart]". Used in the footer and at the top of this
    /// screen; one function, so the two never disagree. Ids: "restart".
    pub(crate) fn render_restart(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let build = self.update_ready()?;
        // TODO: muted 12-13 pt text + ghost_button("restart", "Restart") calling this.restart(cx)
        todo!()
    }

    pub(crate) fn render_feedback(&self, screen: &FeedbackScreen, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // Layout, one centred column at most 560 pt, the key screen's gutters and spacing:
        //
        //   ‹ Back to the page                                   (id "feedback-back")
        //   What should change?                                  (serif 30 pt semibold)
        //   Build 313 is installed. Restart                      (render_restart, Mac only)
        //   ┌──────────────────────────────────────────┐
        //   │ Textarea, 3 to 6 lines, 15 pt             │        (id "feedback-field")
        //   └──────────────────────────────────────────┘
        //   <delivery line, muted 13 pt>                         (id "feedback-message")
        //   [Send | Sending… | Try again]                        (id "send-feedback", 48/52 pt)
        //   Goes to the agent on your Mac. Never your writing or your keys.
        //
        //   Feedback & fixes                                     (13 pt medium, muted label)
        //   one row per Item, newest first, ids ("feedback-item", n):
        //     <text, 15 pt ink, at most 3 lines>
        //     <status line, 13 pt>  Waiting for a build · 22:15
        //                           Done in 7 min · <summary>
        //                           Declined · <reason>
        //   or "Nothing sent yet." when empty.
        //
        // Status is in words, not colour: DESIGN.md keeps green and red for changes only, and
        // PRODUCT.md wants states that differ by more than colour. Rows are at least
        // `m.control` tall, so 44 pt on iPhone. The list scrolls with the screen.
        //
        // delivery line: Idle → none; Sending → "Sending…"; Stopped(c) → c.to_string();
        // Failed(e) → e.to_string().
        todo!()
    }
}

/// "Done in 7 min", "Done in 40 s", "Done in 3 h". Pure; the list's only arithmetic.
pub(crate) fn shipped_in(seconds: u64) -> String {
    todo!()
}

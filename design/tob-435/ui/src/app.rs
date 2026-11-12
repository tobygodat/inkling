//! SKETCH: changes to ui/src/app.rs. Only what changes is shown.

use crate::feedback::FeedbackScreen;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;
use inkling_core::{Build, BuildNumber, Desk, KeyStore, Paths};

pub struct Inkling {
    // … every existing field stays …
    /// All feedback state: the sent log, the running build's records, the send in flight.
    pub(crate) feedback: Desk,
    /// Whether a feedback link is saved. Asked of the key store at launch and after a link save,
    /// never on a frame, like `key_saved`. False hides every feedback entry point.
    pub(crate) feedback_on: bool,
    /// The feedback text. Lives here, not on the screen, so it survives closing the screen and
    /// a failed send.
    pub(crate) feedback_field: Entity<TextareaState>,
    pub(crate) feedback_screen: Option<FeedbackScreen>,
    /// The send running on the background executor. Replaced, never cancelled: a POST that has
    /// left can't be called back, and T3 dedupes on the id.
    pub(crate) sending: Task<()>,
    /// The build on disk, as the host last reported it (Mac only).
    pub(crate) installed: Option<BuildNumber>,
}

impl Inkling {
    /// One new parameter, `build`: what the host is running. Mac and iPhone pass
    /// `Build::running()`; the Mac snap passes `Build::with_records(…)` to relaunch "as the next
    /// build".
    pub fn new(
        form: crate::FormFactor,
        paths: Paths,
        keys: Box<dyn KeyStore>,
        build: Build,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // TODO, beside the existing setup:
        // feedback: Desk::open(paths.clone(), build),
        // feedback_on: Desk::has_link(keys.as_ref()),
        // feedback_field: Textarea with placeholder "What should change?", soft wrap, autocorrect
        //   on, auto-capitalise sentences (it's prose, like the page);
        // sending: Task::ready(()), installed: None, feedback_screen: None.
        todo!()
    }

    // escape(): `if self.feedback_screen.is_some() { self.close_feedback(…) }` goes first in the
    // chain, beside the key screen's.

    // Render for Inkling: after the key screen check,
    //   if let Some(screen) = &self.feedback_screen { return root.child(self.render_feedback(screen, cx)) }
    // The two screens are exclusive: opening one closes the other.

    /// Mac footer, left to right: Saved · Feedback · <restart> · provider. "Feedback" (id
    /// "open-feedback") is a 12 pt ghost text button, present only while `feedback_on`. The
    /// restart notice (render_restart) appears only while a newer build is on disk.
    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        todo!();
        #[allow(unreachable_code)]
        div()
    }

    // iPhone `render_status_row`: between the saved text and the provider button, a 44 pt
    // ghost "Feedback" (id "open-feedback") while `feedback_on`. No restart on iPhone: its host
    // never calls `set_installed_build`.
}

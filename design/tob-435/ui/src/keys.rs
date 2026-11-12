//! SKETCH: changes to ui/src/keys.rs. The key screen is "the place to paste the link" (decision 5):
//! the one piece of feedback UI shown before a link is saved, so the writer who never pastes one
//! never sees feedback anywhere else.

use gpui_kit::component::input::InputState;
use gpui_kit::*;

pub(crate) struct KeyScreen {
    // … existing fields …
    /// Masked, like the key field. Its placeholder says whether a link is saved, never what it is.
    pub link: Entity<InputState>,
    /// What the last Save link did.
    pub link_status: Option<Result<(), String>>,
}

impl crate::app::Inkling {
    /// Save link (id "save-link"), or Return in the link field. An empty field keeps the link
    /// already saved, like the key field.
    pub fn save_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // TODO:
        // let pasted = screen.link.read(cx).value().trim();
        // if pasted.is_empty() { return }
        // status = Desk::save_link(self.keys.as_ref(), pasted)
        // self.feedback_on = Desk::has_link(self.keys.as_ref());
        // clear the field (placeholder "Saved in the Keychain. Paste a new link to replace it.")
        todo!()
    }
}

// In `render_keys`, after the Save key block and its note, a hairline then:
//
//   Feedback link                                   Saved. Feedback is on.   ← status
//   ┌──────────────────────────────────────────┐
//   │ ••••••••••••••••••••••              (eye) │    id "feedback-link", masked, mask_toggle
//   └──────────────────────────────────────────┘
//   [Save link]                                       id "save-link", ghost on Mac, 44 pt on iPhone
//   Paste the link from T3 Code to send what should change from this {device}.
//
// Status copy: Err(message) from BadLink or the Keychain; Ok → "Saved. Feedback is on.";
// none + has link → "Saved". Never the link itself.

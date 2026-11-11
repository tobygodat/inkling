//! The three live providers. Not built yet: `stream` reports that, so the UIs run on
//! `Backend::Fake` until it is.

use crate::ai::{Cancel, Prompt, Provider};

pub fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "claude-sonnet-5-5",
        Provider::Gemini => "gemini-flash-latest",
        Provider::ChatGpt => "gpt-latest",
    }
}

/// Blocks until the answer is complete, calling `on_text` with each piece.
/// Returns early with `Ok` when `cancel` is set.
pub fn stream(
    provider: Provider,
    _model: &str,
    _key: &str,
    _prompt: &Prompt,
    _cancel: &Cancel,
    _on_text: &mut dyn FnMut(String),
) -> Result<(), String> {
    Err(format!("{} isn't connected yet.", provider.label()))
}

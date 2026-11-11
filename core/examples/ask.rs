//! Try a real key against one provider:
//!
//! ```sh
//! INKLING_KEY=... cargo run --example ask -- claude "Make it warmer" "The meeting is at noon."
//! ```
//!
//! The key is read from `INKLING_KEY` only, never from an argument, so it stays out of shell
//! history. `INKLING_MODEL` overrides the provider's default model.

use inkling_core::providers::stream;
use inkling_core::{Action, AiRequest, Cancel, Provider};
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [provider, instruction, selection] = args.as_slice() else {
        eprintln!("usage: ask <claude|gemini|chatgpt> \"<instruction>\" \"<selected text>\"");
        return ExitCode::from(2);
    };
    let Some(provider) = Provider::ALL.into_iter().find(|p| p.id() == provider) else {
        eprintln!("unknown provider \"{provider}\": use claude, gemini or chatgpt");
        return ExitCode::from(2);
    };
    let Ok(key) = std::env::var("INKLING_KEY") else {
        eprintln!("set INKLING_KEY to your {} API key", provider.label());
        return ExitCode::from(2);
    };
    let model = std::env::var("INKLING_MODEL").unwrap_or_else(|_| provider.default_model().into());
    let request = AiRequest {
        action: Action::Rewrite(instruction.clone()),
        before: String::new(),
        selection: selection.clone(),
        after: String::new(),
    };
    eprintln!("{} · {model}", provider.label());
    let mut stdout = std::io::stdout();
    let result = stream(provider, &model, &key, &request.prompt(), &Cancel::default(), &mut |text| {
        let _ = stdout.write_all(text.as_bytes()).and_then(|_| stdout.flush());
    });
    println!();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

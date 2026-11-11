//! Try a real key against one provider:
//!
//! ```sh
//! INKLING_KEY=... cargo run -p inkling-core --example ask -- claude "Make it warmer" "The meeting is at noon."
//! ```
//!
//! It sends the text as a draft with the instruction as a free-form ask, streams the revised
//! draft, then prints the review it becomes. The key is read from `INKLING_KEY` only, never from
//! an argument, so it stays out of shell history. `INKLING_MODEL` overrides the default model.

use inkling_core::providers::stream;
use inkling_core::{Ask, Cancel, Decision, Provider, Request};
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [provider, instruction, draft] = args.as_slice() else {
        eprintln!("usage: ask <claude|gemini|chatgpt> \"<instruction>\" \"<draft>\"");
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
    let request = match Request::new(draft.clone(), Ask::Freeform(instruction.clone())) {
        Ok(request) => request,
        Err(bad) => {
            eprintln!("{bad}");
            return ExitCode::from(2);
        }
    };
    let model = std::env::var("INKLING_MODEL").unwrap_or_else(|_| provider.default_model().into());
    eprintln!("{} · {model}", provider.label());
    let mut answer = String::new();
    let mut stdout = std::io::stdout();
    let result = stream(provider, &model, &key, &request.prompt(), &Cancel::default(), &mut |text| {
        let _ = stdout.write_all(text.as_bytes()).and_then(|_| stdout.flush());
        answer.push_str(&text);
    });
    println!();
    if let Err(message) = result {
        eprintln!("{message}");
        return ExitCode::FAILURE;
    }
    match request.review(&answer) {
        Ok(review) => {
            for change in review.changes().filter(|c| c.decision == Decision::Pending) {
                eprintln!("- {:?} + {:?}", change.removed, change.added);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

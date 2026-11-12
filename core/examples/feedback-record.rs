//! Writes one feedback record for `feedback/ship.sh`, so bash never builds JSON:
//!
//! ```sh
//! cargo run -q -p inkling-core --example feedback-record -- <done|declined> <id> "<one line>"
//! ```
//!
//! Writes `feedback/<id>.json` with `seconds_to_ship` from the id's send time to now, and prints
//! its path. Exits 2 on a bad id or a line the app wouldn't show.

use inkling_core::feedback::{FeedbackId, Outcome, write_record};
use std::path::Path;
use std::process::ExitCode;
use std::time::SystemTime;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [outcome, id, line] = args.as_slice() else {
        eprintln!("usage: feedback-record <done|declined> <id> \"<one line>\"");
        return ExitCode::from(2);
    };
    let outcome = match outcome.as_str() {
        "done" => Outcome::Done { summary: line.clone() },
        "declined" => Outcome::Declined { reason: line.clone() },
        other => {
            eprintln!("\"{other}\" is not done or declined");
            return ExitCode::from(2);
        }
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../feedback");
    match FeedbackId::parse(id).and_then(|id| write_record(&dir, id, outcome, SystemTime::now())) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

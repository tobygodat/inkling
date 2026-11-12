//! SKETCH: core/examples/feedback-record.rs (new)
//!
//! Writes one record file, for `feedback/ship.sh`. Bash never builds JSON: this calls the same
//! `record::to_json` / `record::parse` the app and the build script use.
//!
//! ```text
//! cargo run -q -p inkling-core --example feedback-record -- <done|declined> <id> "<one line>"
//! ```
//! Prints the path it wrote. Exit 2 with a message on a bad id, an empty or multi-line text, or
//! a line over MAX_LINE chars.
//!
//! Idempotent: writes `feedback/<id>.json` through a temp file and a rename, so a crash leaves
//! either the old file or the new one. Rerunning rewrites it with a fresh `seconds_to_ship`;
//! `ship.sh` never calls it for a record that is already committed.

fn main() {
    // TODO: parse args into (Outcome, FeedbackId::parse(id)); on Err print it and exit(2).
    // match inkling_core::feedback::write_record(Path::new("feedback"), id, outcome, SystemTime::now()) {
    //     Ok(path) => println!("{}", path.display()),
    //     Err(e) => { eprintln!("{e}"); std::process::exit(2) }
    // }
    todo!()
}

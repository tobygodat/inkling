//! SKETCH: core/build.rs (new)
//!
//! Compiles the agent's records into inkling-core, so every app that links core (the Mac
//! workspace and the separate `ios/` workspace, which depends on `../core` by path) gets the same
//! records with no step of its own. Paths are from CARGO_MANIFEST_DIR, so it doesn't matter which
//! workspace is building.
//!
//! [build-dependencies] serde (derive) and serde_json, the versions core already uses.

#[path = "src/feedback/record.rs"]
#[allow(dead_code)]
mod record;

fn main() {
    // TODO:
    // let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../feedback");
    // println!("cargo:rerun-if-changed={}", dir.display());   // a directory: cargo rescans it
    // println!("cargo:rerun-if-env-changed=INKLING_BUILD");
    //
    // Records:
    //   for each `<stem>.json` directly in `dir` (a missing dir is no records; README.md and
    //   ship.sh are skipped by extension), sorted by name:
    //     record::parse(stem, &bytes) or
    //       panic!("feedback/{stem}.json: {error}. Fix the file or rerun feedback/ship.sh.")
    //   write `$OUT_DIR/records.json`: one JSON array of record::to_json outputs.
    //   A bad record fails `cargo test` in ship.sh, before anything is committed.
    //
    // Build number:
    //   INKLING_BUILD unset → nothing (Build::number is None).
    //   set → must parse as u32, else panic!("INKLING_BUILD must be a whole number, got {v:?}");
    //   println!("cargo:rustc-env=INKLING_BUILD_NUMBER={n}").
    //   Set by mac/install.sh (the same $build it writes to CFBundleVersion) and by
    //   ios/cargo-ios.sh (`git rev-list --count HEAD`). Debug builds for tests and snaps leave it
    //   unset, so the release build that install.sh makes is the only one that changes per commit.
    todo!()
}

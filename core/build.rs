//! Compiles the agent's records into inkling-core, so every app that links core gets the same
//! records with no step of its own. That includes the separate `ios/` workspace: `../feedback`
//! is resolved from this crate's own folder, whichever workspace builds it.

#[path = "src/feedback/record.rs"]
#[allow(dead_code)]
mod record;

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../feedback");
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rerun-if-env-changed=INKLING_BUILD");

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "json"));
    files.sort();
    let records: Vec<String> = files
        .iter()
        .map(|path| {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("feedback/{stem}.json: {e}"));
            let record = record::parse(stem, &bytes)
                .unwrap_or_else(|e| panic!("feedback/{stem}.json: {e}. Fix the file or rerun feedback/ship.sh."));
            record::to_json(&record)
        })
        .collect();
    let out = Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("records.json");
    std::fs::write(out, format!("[{}]", records.join(","))).expect("write records.json");

    if let Ok(build) = std::env::var("INKLING_BUILD") {
        let n: u32 = build.parse().unwrap_or_else(|_| panic!("INKLING_BUILD must be a whole number, got {build:?}"));
        println!("cargo:rustc-env=INKLING_BUILD_NUMBER={n}");
    }
}

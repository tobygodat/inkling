//! SKETCH: changes to ios/src/lib.rs and ios/cargo-ios.sh.
//!
//! ios/src/lib.rs: `Inkling::new(FormFactor::Phone, Paths { dir }, default_keys(), Build::running(), …)`.
//! Nothing else: the records come in through inkling-core's build script, which runs in this
//! workspace too (it resolves `../feedback` from core's own manifest dir). The iPhone never calls
//! `set_installed_build` and never subscribes to `Restart`, so it never shows a restart notice.
//!
//! ios/cargo-ios.sh: `export INKLING_BUILD="$(git -C .. rev-list --count HEAD)"` before
//! `cargo build`, so the iPhone's body names the commit count it was built from, like the Mac's.
//! (TestFlight's CFBundleVersion stays testflight.sh's UTC stamp; that's Apple's number, not ours.)
//!
//! ios/snap.sh: three new steps after `back-with-key`, 12 → 15. New points in `point()`:
//! `feedback-link`, `save-link`, `open-feedback`, `feedback-field`, `send-feedback`.
//!
//!   13 link-saved     tap add-key; swipe up to the link section; tap feedback-link; clipboard
//!                     write the fake link; press label="Paste"; tap save-link; shot.
//!                     check: settings.json does not contain "#k=".
//!   14 feedback-open  tap back; tap open-feedback; shot (empty list).
//!   15 feedback-sent  tap feedback-field; keys.sh "bigger margins"; tap header-done;
//!                     tap send-feedback; wait for sent-feedback.json; check it holds
//!                     "Bigger margins" and not "the light held"; shot (one row, waiting).
//!
//! Done, declined, a failed send and Restart are proved by the Mac snap, which also renders the
//! phone layout of the list (18-iphone-feedback): rebuilding the iPhone app mid-run with new
//! records would cost a minute for nothing the shared view doesn't already show.

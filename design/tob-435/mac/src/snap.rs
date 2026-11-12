//! SKETCH: additions to mac/src/snap.rs. One new section, `feedback(dir)`, run after `phone()`
//! in the fake-AI pass. Offscreen, input dispatched in the app, fake transport, keys in memory.
//!
//! Snap gets `Snap::open_with(dir, form, keys: Arc<MemoryKeys>, build: Build)`; `open_as` becomes
//! `open_with(dir, form, Arc::default(), Build::with_records(None, []))`, so every existing step
//! is unchanged. A link saved before a relaunch is still there after it, as in the Keychain.
//!
//! Fake links: `https://hooks.example/inkling#k=<32 x 'a'>` delivers; the same with host
//! `hooks.invalid` fails as Offline (see `Post::run`).
//!
//! Each line below is one `check(…)`; NN-name is a screenshot in docs/screenshots/mac/.

pub fn feedback(dir: &std::path::Path) {
    // keys = Arc<MemoryKeys>; s = open_with(dir, Mac, keys, Build::with_records(Some(BuildNumber(41)), []))
    //
    // Hidden until set up
    //   ok  "with no link, nothing offers feedback"            bounds("open-feedback").is_none()
    //   cmd-, ; the key screen has "feedback-link"
    //   input "not a link"; click save-link
    //   ok  "a bad link is refused and nothing is saved"        status Err, !keys.has(FeedbackLink)
    //   input the good link; click save-link
    //   ok  "a saved link turns feedback on"                    keys.has(FeedbackLink)
    //   ok  "the link is in no file in the data folder"         grep every file under INKLING_DATA_DIR
    //   shot 14-feedback-link
    //   escape
    //   ok  "the footer offers Feedback"                        bounds("open-feedback").is_some()
    //
    // Sending
    //   click open-feedback
    //   ok  "an empty list says nothing was sent"
    //   shot 15-feedback-empty
    //   input "Make the margins wider on the Mac."; click send-feedback; run_until_parked
    //   ok  "a send lands in the list as waiting for a build"   items()[0] Pending, field empty
    //   ok  "the sent log holds the text and not the draft"     read sent-feedback.json: has the
    //                                                           text, has no paragraph of PARAGRAPHS
    //
    // A failed send keeps the text
    //   save the hooks.invalid link; reopen feedback; input "Bigger title."; click send-feedback
    //   ok  "a failed send says so and keeps the text"          Delivery::Failed(Offline), field text,
    //                                                           items().count() == 1
    //   ok  "the button offers Try again"
    //   shot 16-feedback-failed
    //   save the good link; reopen; click send-feedback
    //   ok  "Try again sends the same text"                     items().count() == 2, field empty
    //
    // Statuses come from the build
    //   ids = the two items' ids; send a third ("A darker caret.")
    //   drop(s); s = open_with(dir, Mac, same keys, Build::with_records(Some(BuildNumber(42)),
    //       [done(ids[0], "Margins are 32 pt wider on the Mac.", 412), declined(ids[1], "The title is already 36 pt, the largest DESIGN.md allows.", 95)]))
    //   click open-feedback
    //   ok  "the next build shows done, declined and waiting"   statuses in order Pending, Declined, Done
    //   shot 17-feedback-statuses
    //   phone: same keys and build, FormFactor::Phone; tap open-feedback from the status row
    //   ok  "on iPhone the rows are at least 44 pt"             every ("feedback-item", n) height >= 44
    //   shot 18-iphone-feedback
    //
    // Restart (Mac)
    //   back on the Mac view; subscribe to Restart, counting events
    //   set_installed_build(Some(42))   ok "the running build on disk offers no restart"   no "restart"
    //   set_installed_build(Some(43))   ok "a newer build on disk offers Restart"         "restart" present
    //   shot 19-restart
    //   input a word on the page (autosave pending); click restart
    //   ok  "Restart saves the draft and asks the host"          draft file == page text, 1 event
    //   Build number None + installed 43: ok "a development build never offers a restart"
    todo!()
}

# Architecture

## Crates
One Cargo workspace at the root holds three crates. `ios/` is a separate workspace for the iPhone
app.

- `core/` is `inkling-core`. It has no UI dependency. It holds the requests (`Ask`, `Request`), the
  three providers, the review model (`Review`, `Change`, `Decision`), `Revision`, feedback
  (`feedback::Desk`, `Build`), settings and key storage. Its build script compiles the feedback
  records in `feedback/` into the crate.
- `ui/` is `inkling-ui`: every view, on GPUI Kit, with no Mac-only or iPhone-only code. The host
  passes a `FormFactor`, the data folder (`Paths`) and the key store. It bundles Source Serif 4 and
  Inter (SIL OFL, licences in `ui/fonts/`).
- `mac/` is the Mac binary. It turns on GPUI Kit's `gpui-fast` feature and opens one window.
- `ios/` is `inkling-ios`, a static library the Swift host in `ios/app/` links. It runs on
  `gpui-pre` through a patched `gpui-mobile`, passes the sandbox's Application Support folder as
  `Paths`, and uses the Keychain through `default_keys()`. The host keeps the view between the top
  safe area and the keyboard.

## Data flow
A quick action or the ask field builds a `Request` from the page, picks a `Backend` with
`backend_for_request`, and calls `start`. Events arrive on a background thread, go through a channel,
and the view feeds each one to `Revision`, which is the only AI state. When the answer is complete,
`Revision` holds a `Review`. The view draws its segments and sends each decision to it. Finishing
the review applies `Revision::finish()`'s edits to the page through the editor, one undo step each.
The draft is never touched before that.

## Feedback
The writer says what should change from inside the app, an agent on the Mac ships it, and the app
shows what happened. `core::feedback::Desk` owns all of it, the way `Revision` owns AI state.

- A feedback link (`https://<webhook>#k=<secret>`) is pasted on the key screen and kept in the
  Keychain (`Secret::FeedbackLink`). Until one is saved, no other feedback UI shows.
- Send builds a `Post` on the UI thread: a JSON body with the text, the id and where it was sent
  from, never the draft or a key, signed with HMAC-SHA256 in `x-inkling-signature`. The secret
  rides in the URL fragment, which is never sent. `Post::run` goes out on the background executor
  over HTTPS, without following redirects, and `Desk::settle` takes the result. A headless run
  (`store::headless()`) uses a fake transport that fails only `.invalid` hosts.
- The id is the send time plus 8 random base32 characters. A retry of the same text keeps it, so
  the webhook can drop a duplicate.
- Each success is appended to `sent-feedback.json` in the data folder. Status is never stored: an
  item is done or declined when the running build holds a record with its id, else it is waiting.
- `feedback/ship.sh` writes one record per feedback (`feedback/<id>.json`), commits it with the
  change, installs the app and pushes. `core/build.rs` validates every record with the same parser
  the app uses and compiles them in, so both Cargo workspaces get them.
- The build number is the commit count. `mac/install.sh` compiles it in through `INKLING_BUILD`
  and writes it to CFBundleVersion. The Mac host reads the installed bundle's number every 5 s;
  the view offers Restart while it is newer than its own. Restart saves the draft and asks the
  host to relaunch the bundle once this process has quit.

## The review page
The page during review is one `StyledText` inside `ui/src/inline.rs`'s `InlineText` element. On Mac
each change is followed by a run of no-break spaces wide enough for its controls. After the text is
laid out, each control is laid out on its own and placed at its run, centred on the line. The
controls are ordinary elements with click handlers, so they work by mouse and by touch. On iPhone
there are no inline controls: a tap is hit-tested against the change ranges, because GPUI's
`InteractiveText` clicks never fire on touch, and a sheet acts on the selected change. After Keep or
Use the sheet moves to the next pending change.

## State
- The draft and settings are files under the app's data folder (`Paths`). The page autosaves 500 ms
  after the last edit.
- API keys and the feedback link are in the Apple Keychain (`KeychainKeys`, keyed by `Secret`),
  never in a file. The key screen shows "Saved" from `KeyStore::has`, which never reads them.
- What this device sent as feedback is `sent-feedback.json` in the data folder. What happened to
  it is compiled into the build.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.
Feedback goes to the T3 webhook in the writer's feedback link, and only the feedback text and
where it was sent from.

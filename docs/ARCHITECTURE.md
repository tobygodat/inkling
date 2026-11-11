# Architecture

## Crates
One Cargo workspace at the root holds three crates. `apps/gpui-kit-ios-spike/` is a separate
workspace kept as the seed for the iPhone host.

- `core/` is `inkling-core`. It has no UI dependency. It holds the requests (`Ask`, `Request`), the
  three providers, the review model (`Review`, `Change`, `Decision`), `Revision`, settings and key
  storage.
- `ui/` is `inkling-ui`: every view, on GPUI Kit, with no Mac-only or iPhone-only code. The host
  passes a `FormFactor`, the data folder (`Paths`) and the key store. It bundles Source Serif 4 and
  Inter (SIL OFL, licences in `ui/fonts/`).
- `mac/` is the Mac binary. It turns on GPUI Kit's `gpui-fast` feature and opens one window.

## Data flow
A quick action or the ask field builds a `Request` from the page, picks a `Backend` with
`backend_for_request`, and calls `start`. Events arrive on a background thread, go through a channel,
and the view feeds each one to `Revision`, which is the only AI state. When the answer is complete,
`Revision` holds a `Review`. The view draws its segments and sends each decision to it. Finishing
the review applies `Revision::finish()`'s edits to the page through the editor, one undo step each.
The draft is never touched before that.

## The review page
The page during review is one `StyledText` inside `ui/src/inline.rs`'s `InlineText` element. On Mac
each change is followed by a run of no-break spaces wide enough for its controls. After the text is
laid out, each control is laid out on its own and placed at its run, centred on the line. The
controls are ordinary elements with click handlers, so they work by mouse and by touch. On iPhone
there are no inline controls: a tap is hit-tested against the change ranges and a sheet acts on the
selected change.

## State
- The draft and settings are files under the app's data folder (`Paths`). The page autosaves 500 ms
  after the last edit.
- API keys are in the Apple Keychain (`KeychainKeys`), never in a file. The key screen shows "Saved"
  from `KeyStore::has`, which never reads the key.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.

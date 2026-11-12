# Architecture

## Crates
One Cargo workspace at the root holds three crates. `ios/` is a separate workspace for the iPhone
app.

- `core/` is `inkling-core`. It has no UI dependency. It holds the requests (`Ask`, `Request`), the
  three providers, the review model (`Review`, `Change`, `Decision`), `Revision`, feedback
  (`feedback::Desk`, `Build`), settings with the writer's `Appearance`, and key storage. Its build script compiles the feedback
  records in `feedback/` into the crate.
- `ui/` is `inkling-ui`: every view, on GPUI Kit, with no Mac-only or iPhone-only code. The host
  passes a `FormFactor`, the data folder (`Paths`) and the key store. It bundles Source Serif 4 and
  Inter (SIL OFL, licences in `ui/fonts/`) and six one-colour icons (`ui/icons/`, sources and terms
  in `SOURCES.md`), which it serves with GPUI Kit's own through `inkling_ui::Assets`. Both hosts
  register that asset source.
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

## Appearance
`core::appearance::Appearance` is what the writer chose: Light, Dark or Custom, the four custom
colours, the body face, text size, line spacing, page width, and the Markdown and maths switches. It lives in `settings.json` and is
read leniently: a missing field or one that can't be read takes its default, and every number is
clamped, so neither an old file nor a hand edit can load an invalid look or cost the other
settings. `Appearance::tokens` derives every colour the views draw; Light is exactly the table in
`DESIGN.md`.

`ui::theme::Theme::new` turns the appearance into GPUI colours and the body's face and sizes, and
is the one place views read them. The Mac uses the writer's appearance in a face its text system
has (asked once at launch); the iPhone always gets the default look at its own sizes.
`Inkling::set_appearance` is the one way to change it: it saves the file, recolours GPUI Kit's
theme and redraws the window.

## The page
`ui::page::Page` is the writing page: the live editor in `ui/src/live/`, on the Mac and on
iPhone. Asking, Find a word, Next line, autosave and finishing a review use only `Page`'s `value`,
`selected_range`, `cursor` and `replace`, so they see the Markdown source and its byte ranges.

The live editor keeps the source in `core::draft::Draft`, which owns the selection, the top-level
blocks and the undo history, and changes only through `Draft::replace`. Each block's
`SourceMap` maps the text GPUI Kit draws back to source bytes, so a click or a drag on drawn text
lands on the right byte. Every block is drawn by GPUI Kit's Markdown renderer except the one that
holds the caret, which shows its source with the syntax muted. RaTeX sets each formula, and the
editor caches the image per formula, size, colour and scale. A remote image is drawn as its alt text and never
fetched.

On iPhone the keyboard reads and edits the source through the editor's `EntityInputHandler`, in
UTF-16 code units that `Draft` converts to and from bytes. UIKit draws the caret, the selection,
its handles and the edit menu over the page while the page has focus (`FormFactor::system_selection`),
from the rectangles the editor reports for a source range. The editor keeps the taps: a tap lands on
the tapped byte, a double tap selects the word. A drag on a handle and a long press are UIKit's, so
the editor claims those touches to keep the page from scrolling under them. While the page has no
focus it has no block of source: every block is drawn. `ios/README.md` has the bridge's side.

Assistive technology reads the page as one multi-line text input, as it read the text area the
live editor replaced. Its value is the source, with the source split into text runs so the
selection can be reported as a place in the text. Each formula reads "Formula" and its TeX, and
each image its alt text. GPUI builds that tree only while a screen reader asks for it, and
`ui/src/live/a11y.rs` reads the editor only then. On iPhone gpui-mobile passes UIKit no tree, so
VoiceOver reads the page only through the text-input bridge, once the page has focus.

`Appearance::markdown` and `Appearance::maths` are the page's two switches.
`Inkling::set_appearance` passes them, with the theme, to the editor, which redraws at once. With
Markdown off, every block is its source and maths is off too, because `$` is Markdown here.

The review keeps drawing the source text, syntax included, so each change sits on the bytes it
replaces. On the Mac its text starts where the live page's does, so nothing moves when a review
opens.

## What is open
`Inkling::screen` is one `Screen`: the page, the key screen or the feedback screen. The page
carries `Option<AppearancePanel>`, and the panel carries `Option<Picker>`, whose `Option<Custom>`
holds the wheel. So the panel exists only beside the page, a picker only inside the panel, and
opening a screen drops whatever the one before held. The cog's `Menu` (`Closed`, `Open { pinned }`,
`Closing` while it fades) is a separate field, because it sits over every screen.

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
  after the last edit. Settings hold the provider, the models and the appearance.
- API keys and the feedback link are in the Apple Keychain (`KeychainKeys`, keyed by `Secret`),
  never in a file. The key screen shows "Saved" from `KeyStore::has`, which never reads them.
- What this device sent as feedback is `sent-feedback.json` in the data folder. What happened to
  it is compiled into the build.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.
Feedback goes to the T3 webhook in the writer's feedback link, and only the feedback text and
where it was sent from.

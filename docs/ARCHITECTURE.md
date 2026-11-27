# Architecture

## Crates
One Cargo workspace at the root holds seven crates. `ios/` is a separate workspace for the iPhone
app.

- `core/` is `inkling-core`. It has no UI dependency. It holds the requests (`Ask`, `Request`), the
  three providers, the review model (`Review`, `Change`, `Decision`), `Revision`, feedback
  (`feedback::Desk`, `Build`), settings with the writer's `Appearance`, and key storage. Its build script compiles the feedback
  records in `feedback/` into the crate.
- `books/` is `inkling-books`, ported from the reader's `reader-core` (ADR 0003). It has no UI
  dependency. It parses an EPUB (chapters from the navigation document, the cover it names) or a
  PDF with a text layer (pages in sections of ten, running headers and footers dropped) into one
  `Book` whose chapters are Markdown (ADR 0006). A web page saved by its address becomes a `Book`
  too (`article`, ADR 0007): Readability (`dom_smoothie`) finds the article, the EPUB walker writes
  it as one chapter, and `Book::source` keeps its address, site, date and excerpt. `Library` keeps `library.json`, each parsed book
  as `books/<sha-256>.json`, the file it came from in `originals/`, its images in `images/<id>/`,
  covers in `covers/` and highlights in `highlights/<id>.json`, writing each file atomically, with each book's reading position.
  `scan` finds books in folders that the library doesn't have yet. The Library and Reading
  screens in `ui/` use it.
- `mcp/` is `inkling-mcp`, the MCP server of ADR 0004. It has no UI dependency. `Server` answers
  one JSON-RPC message at a time, whatever carries it. Its seven tools (`list_books`, `read_book`,
  `search_library`, `read_draft`, `list_documents`, `read_document`, `list_highlights`) only read,
  through a `Store`, and are defined once in `mcp/tools.json`, which the Worker serves too, all but
  `list_highlights` until highlights sync. `read_draft` is the
  document open last. `Folder` is the store over the data folder, read afresh on every call so a
  book the app just added shows at once. The binary, `inkling-mcp --stdio`, is what Claude Desktop
  and Claude Code start on the Mac. It writes nothing in the data folder.
- `sync/` is `inkling-sync`, the devices' side of the server (ADR 0004, phase 3). It has no UI
  dependency. `account` is the OAuth client: `begin` registers the device and builds the sign-in
  page's address with PKCE, `Begun::finish` trades the code that comes back for a refresh token,
  and `Loopback` is the Mac's one-time listener on 127.0.0.1. `run` is one sync: `documents::sync`
  and `library::sync` talk to the server off the UI thread and return what to change, which the
  app records on the UI thread (below). Everything goes through a `Transport`: `Net` (HTTPS) in
  the app, `Fake` (the server in memory, with the Worker's rules and no Google) in tests and in
  every headless run, kept in `fake-server.json` in the data folder.
- `ffi/` is `inkling-ffi`, the boundary of ADR 0007's native app. It has no UI dependency. Its
  `Library` object wraps `inkling_books::Library` behind a mutex: `items`, `book` (which upgrades
  an old parse, as opening a book does), `add_file`, `save_article` (with a `Fetcher` that Swift
  implements for the pictures), `set_position`, `opened` and the highlights. Records and
  `InklingError` are plain values, so UniFFI stays in this crate. `apple/core.sh` builds it into
  `apple/InklingCore`: an XCFramework of the static library for the Mac, the iPhone and the
  Simulator, the Swift that `uniffi-bindgen` writes from the Mac library, and the package's own
  Swift (`PictureFetcher`, over URLSession).
- `apple/InklingUI` is the native app's screens, in SwiftUI. `Shelf` is the library as the screens
  see it, calling `Library` off the main thread. `Blocks` splits a chapter's Markdown into the
  blocks the reader draws, each knowing its byte offset, and `Inline` styles a block's words. On
  the Mac, `RootView` is a three-column split view (sidebar, rows, reader); on iPhone, a
  navigation stack. The reader keeps the place as a chapter and a fraction, as the GPUI reader
  does. The Mac app reads the same `~/Library/Application Support/Inkling` as the GPUI app, so
  the two should not both be open. It bundles a copy of `ui/fonts/` until `ui/` goes.
- `ui/` is `inkling-ui`: every view, on GPUI Kit, with no Mac-only or iPhone-only code. The host
  passes a `FormFactor`, the data folder (`Paths`) and the key store, and can lend the library a
  `BookHost`. It bundles Source Serif 4 and Inter (SIL OFL, licences in `ui/fonts/`) and nine
  one-colour icons (`ui/icons/`, sources and terms
  in `SOURCES.md`), which it serves with GPUI Kit's own through `inkling_ui::Assets`. Both hosts
  register that asset source.
- `mac/` is the Mac binary. It turns on GPUI Kit's `gpui-fast` feature and opens one window. It lends
  the library the open panel and the Desktop, Documents and Downloads folders.
- `ios/` is `inkling-ios`, a static library the Swift host in `ios/app/` links. It runs on
  `gpui-pre` through a patched `gpui-mobile`, passes the sandbox's Application Support folder as
  `Paths`, and uses the Keychain through `default_keys()`. The host keeps the view between the top
  safe area and the keyboard. It lends the library a `BookHost` over the C ABI: the Swift host
  (`ios/app/Inkling/Books.swift`) shows the document pickers and keeps the folder bookmarks.

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
`core::appearance::Appearance` is one look: Light, Dark, Custom or one of the paper tones (Paper,
Sepia, Sage, Dusk, Night), the four custom colours, the body face, text size, line spacing, page width, and the Markdown and maths switches. It lives in `settings.json` and is
read leniently: a missing field or one that can't be read takes its default, and every number is
clamped, so neither an old file nor a hand edit can load an invalid look or cost the other
settings. `Appearance::tokens` derives every colour the views draw; Light is exactly the table in
`DESIGN.md`.

`Settings` holds two: `appearance` for writing and `reading.look` for the library and books, with
`reading.synced` and `reading.pages` (Scroll or Paged) beside it (ADR 0005). `Settings::change_look`
is the one way to change a look: while synced it gives the other view the same colours, face, sizes
and width, and never touches writing's Markdown and maths switches. A file without `reading`, or
with junk for it, loads with reading as a copy of writing, synced.

`ui::theme::Theme::new` turns the appearance into GPUI colours and the body's face and sizes, and
is the one place views read them. `Inkling::theme` is the look of the view on screen
(`Inkling::view`: reading while the library or a book is open). The Mac uses it in a face its text
system has (asked once at launch). The iPhone draws the same look at its own sizes: text and line
height scaled by 19/21 and 1.65/1.75, so the default stays `DESIGN.md`'s, and the screen's width
for the column. `Inkling` emits a `Ground` (the background colour and whether it is dark) when the
look on screen changes, and the iPhone host colours its safe areas and status bar with it.
`Inkling::set_appearance` changes the view on screen's look through `change_look`: it saves the
file, recolours GPUI Kit's theme and redraws the window.

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

## Books
The library is `<data>/library/`, an `inkling_books::Library` that `Inkling` opens at launch. The
host lends it a `BookHost`: `folders`, called at each search, for the folders Find books searches;
`pick`, which shows its picker and resolves to the files chosen; and `add_folder`, on a host that
can only search folders the reader allowed. `ui/` never reads the platform, so without a host the
library has no Add and no Find books.

- **The Mac** searches Desktop, Documents and Downloads and picks with the open panel.
- **The iPhone** searches the app's own Documents (shown in the Files app as "Inkling") and every
  folder the reader allowed through Search another folder. The Swift host keeps those as
  security-scoped bookmarks in `book-folders.plist` and opens them at each search, holding their
  access until the next one, because chosen books are read from them in place. Add uses the system
  document picker, which copies the files in. Rust asks through `inkling_host_pick_books`,
  `inkling_host_pick_folder` and `inkling_host_book_folders`; Swift answers a picker through
  `inkling_ios_picked` with the token Rust gave it. iCloud files not downloaded yet are hidden
  placeholders, which the search passes over like any hidden file.

- **Library** (`ui/src/library.rs`). The books newest first, each with its cover or its title on a
  plain tile, and a progress line once started. Add calls `pick`; Find books runs `scan` over the
  host's folders and lists what isn't in the library, EPUBs ticked and PDFs not. Each file is parsed
  and stored by `inkling_books::prepare` on the background executor, then recorded with
  `Library::add` on the UI thread. A file that can't be added shows `ImportError`'s words.
- **A chapter** is Markdown (ADR 0006): CommonMark with GFM footnotes, opening with `# <title>`.
  `books/src/epub.rs` writes it in two passes over the spine: the first reads every footnote (an
  element marked as one, or the target of a link marked as a marker) and every id; the second
  writes the text, with `[^n]` for each marker and its definition at the chapter's end, images as
  `![alt](images/<id>/<n>.<ext> "caption")` and links inside the book as `#<chapter>:<byte>`,
  settled once every chapter is written. `books/src/markdown.rs` escapes book text so it reads
  back as itself, which a property checks against the parser GPUI Kit draws with. A place in a
  chapter is a byte offset into its text. `read_book` returns the text as stored, and sync sends
  the same.
- **Articles.** `prepare_article` takes a page's HTML, the address it came from and a way to
  fetch pictures, and stores it like `prepare` stores a file: the HTML in `originals/<id>.html`,
  each picture once in `images/<id>/` (a picture that can't be fetched is left out, so reading
  never sends a request), then the book. Its id is the hash of the address without its fragment,
  so saving a page twice finds it. Readability keeps only the classes that mark footnotes, which
  the walker needs. An article is never parsed again on open, because that would fetch its
  pictures again. Fetching the page itself is the caller's.
- **Upgrades.** `Book::parser` names the parser that wrote a book. `prepare`, when a file is
  added, and `library::open`, when a book is opened, parse a book from an older parser again from
  its original, and `Library::add` moves the place to the chapter with the same title. A book
  stored before chapters were Markdown has no original: its blocks read as Markdown, chapter for
  chapter, so it opens at its place until it is added again. `load`, which `inkling-mcp` uses,
  never writes.
- **Reading** (`ui/src/reading.rs`). The book is loaded on the background executor, then one
  chapter at a time is set in reading's look: the body face, size, line spacing, page width and
  colours. `ui/src/chapter.rs` parses the chapter once, with GPUI Kit's Markdown parser, into a
  flat list of blocks, each knowing where it starts in the text: the title, headings, paragraphs
  with their emphasis, links and footnote markers, each paragraph of a quote and each list item
  on its own, images with their captions, and scene breaks. Each block is one `StyledText`. A tap
  is hit-tested against the text's layout, on the Mac and on iPhone alike: a marker opens its note
  beside it (`Reading::note`), a link inside the book goes to its chapter and the block that holds
  its offset, and any other tap passes through to the page's edges. It draws a link back a
  chapter and Next at the end, a Contents list and a progress line (the chapter and how far
  through the book). Leaving a book for the page or the library keeps it laid out in
  `Books::parked`, so going back to it reads nothing from disk.
- **Highlights and quotes** (`ui/src/passage.rs`). A selection is a range of the chapter's text as
  drawn: every block's text in turn, a newline between, which `chapter.rs` maps back byte by byte
  to the chapter's Markdown. A highlight (`inkling_books::Highlight`) keeps the chapter, where the
  passage starts and ends in the Markdown, its words and when it was made, in the book's own file
  apart from its text, so sync can send it as it is. It is drawn where it was made while its words
  are there, else at its words nearest to it after a new parse, else not at all, and it stays in
  the list. Quote writes the passage with its italics as a block quote with the author and title
  (`inkling_books::highlights::quote`) and puts it in the open document's page with
  `inkling_core::place_block`, after the top-level block that holds the caret (closing a code
  fence left open there; where the inserted quote wouldn't read as a block quote of its own, before
  that block, else at the start, else at the end), or at the end while the writer hasn't put a
  caret in it since it opened, as one undo step.
- **Scroll and Paged.** The chapter column reports each child's bounds after layout
  (`on_children_prepainted`), with the line height of the blocks a page may break inside. In
  Scroll the column scrolls. In Paged, `paginate` cuts the chapter into pages the height of the
  reading area, each ending at the last whole line or block, and a page is a clipped window onto the
  same column. Keys and clicks on the page's edges turn pages; past either end of a chapter (the last
  page, or pushing the wheel on past the bottom) goes into the next chapter or the end of the one
  before.
- **The place.** A `Position` is the chapter and an anchor: how far down the chapter the view's top
  is, as a fraction of its height. Scrolling and page turns move it; a new layout (a mode switch, a
  resize, a new text size, the panel opening) puts the view back on it. It is saved with
  `Library::set_position` 500 ms after the last move, on a chapter turn and on leaving the book.
  `Library::opened` records the last book opened, which the cog's Keep reading and the Read half of
  the Write and Read toggle (`Inkling::show_view`, Cmd-1 and Cmd-2) open again.

## Documents
`core::Documents` is the folder `documents/` in the data folder: one Markdown file per document,
and the folder is the list, with no index. A file's stem is the document's id, the time it was made
in milliseconds, and never changes; `documents::title` reads the title from the first line with
text, so retitling never moves a file. `list` is newest first by the file's modified time.
`settings.json`'s `document` is the id open last; `Documents::last_open` falls back to the newest.
At launch `Documents::migrate` moves the old `draft.md` in as `documents/draft.md` with its date,
or under a new id when a different `draft` is already there. A second run changes nothing.

`Inkling::page` is the open document's page and `revision` its AI state. `ui::documents::Docs` keeps
every other document opened since launch as a `Parked` page, revision and version number, so each
keeps its undo history and its review. Switching saves the page now, stops a request still running,
parks the page leaving (or removes its file if it is empty) and swaps the next one in. A new page
has no file until it has text. Asking reads only `Inkling::page`, so it sends only the open
document. The list (`Screen::Documents`) holds the document removed last until the next action;
Cmd-Z or Undo writes it back with its old modified time and gives back its parked page.

## Sync
`ui::account` runs `inkling_sync::run` on the background executor at launch, when the window
becomes active, 3 s after a change settles (an autosave, a reading place saved, a book added, a
document removed or brought back) and every 5 minutes, one at a time; one asked for meanwhile runs
when it ends. Only while signed in: a sign-in is `Secret::ServerSignIn` in the key store.

- **Documents.** `sync.json` in the data folder holds this install's id and, per document, the
  version and SHA-256 of the text it last agreed with the server on. Changed here and there, the
  other device's text is kept under a new id with its own date and this device's goes up on the
  original. `documents::apply` makes each change on disk only while the file still holds what sync
  read; the page is saved first, a document under review or with a request running is held, and the
  open document is never removed under the writer. A skipped change is a conflict at the next sync,
  so both texts are kept. A changed open document's page is rebuilt from its file; a changed
  parked one is dropped and opens afresh.
- **Books and places.** A book goes up as its parsed file, its chapters' Markdown, the pictures
  its chapters link to (`images/<id>/`) and its cover, never its original. Those that came down
  are written off the UI thread (they are named by the book's hash); `Library::record`, `take_position` and `add` record them on the UI
  thread. Readwise uploads are parsed with `prepare` like an Add.
- **Failure.** Any failed sync sets `stopped`: the cog's dot and Account's "Sync stopped". The next
  sync that succeeds clears it. An ended grant also signs the device out.

## What is open
`Inkling::screen` is one `Screen`: the page, the key screen, the feedback screen, the library, a
book being read or the documents. The library carries `Option<Finding>` while Find books is open over it.
`Inkling::panel` is `Option<AppearancePanel>`, drawn beside the page, the library or a book; the key
and feedback screens close it. The panel carries `Option<Picker>`, whose `Option<Custom>` holds the
wheel, so a picker exists only inside the panel. Opening a screen drops whatever the one before
held. The cog's `Menu` (`Closed`, `Open { pinned }`,
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
- The documents and settings are files under the app's data folder (`Paths`), with `sync.json`
  once signed in. The page autosaves
  500 ms after the last edit into its document's file. Settings hold the provider, the models, the
  appearance and the document open last.
- API keys and the feedback link are in the Apple Keychain (`KeychainKeys`, keyed by `Secret`),
  never in a file. The key screen shows "Saved" from `KeyStore::has`, which never reads them.
- The books are under `library/` in the data folder: `library.json`, each parsed book and the covers.
- What this device sent as feedback is `sent-feedback.json` in the data folder. What happened to
  it is compiled into the build.

## Server
`server/` is a Cloudflare Worker on the free plan (ADR 0004), in TypeScript. No request may spend
more than 10 ms of CPU, so it never parses a book.

- **Sign-in.** `@cloudflare/workers-oauth-provider` is the OAuth 2.1 server: dynamic client
  registration, client ID metadata documents, PKCE, refresh tokens, and tokens kept only as hashes
  in KV. There are two resources: `/mcp` for agents and `/v1` for the devices. A token is bound to
  one of them. `src/authorize.ts` shows the consent form, with the draft unticked unless the
  writer ticks it. It then sends the browser to Sign in with Google (`src/google.ts`, with PKCE
  and a nonce), and grants only the Google account whose verified email is `OWNER_EMAIL`.
- **MCP.** `src/mcp.ts` serves `mcp/tools.json` and answers each tool from D1, with the same text as
  `inkling-mcp`. Each chapter is stored as the Markdown `read_book` gives, in a trigram FTS5 index,
  so a search runs inside D1. It finds one hit per chapter, and a query under three characters
  falls back to `instr`.
- **Sync.** `src/sync.ts`. A device uploads a book's parsed JSON and its pictures to KV, then each chapter, then its
  row, which is refused until the rest is there. A position carries when it changed (an entry's
  `position_at`), and the later wins. A document carries a `version`, counted up on every write
  the server takes; a write names the version it was made on and is refused with 409 and what the
  server has otherwise, in one statement so two devices can't both take a version. A delete is a
  write that keeps the last text. Times are milliseconds since the Unix epoch.
- **Readwise.** `src/readwise.ts`. The token is kept as AES-GCM ciphertext under `SECRET_KEY`. The
  poll every 30 minutes streams each new EPUB or PDF from Reader into KV as an inbox item. The next
  device to sync downloads it, parses it into its library and settles the item. Only documents
  saved after the link was made come in.
- **iPhone builds.** `src/builds.ts` (TOB-518). `ios/ota.sh` uploads an ad hoc .ipa with a bearer
  token to R2, where the newest five are kept. The install page, its manifest and the .ipa are
  each served only on a URL signed with HMAC under a key derived from `SECRET_KEY`, which expires
  24 hours after the upload, because iOS fetches the manifest and the .ipa without cookies.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.
Feedback goes to the T3 webhook in the writer's feedback link, and only the feedback text and
where it was sent from. An agent the writer connects through `inkling-mcp` reads the library and
the documents, and the agent's own app decides where that goes. Inkling's own server (above)
holds the library and the documents for agents away from the Mac and for sync, once the writer
signs in on Account. It calls Readwise with the writer's token and Google to sign in.

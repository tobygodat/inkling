# Architecture

## Crates
One Cargo workspace at the root holds five crates. `ios/` is a separate workspace for the iPhone
app.

- `core/` is `inkling-core`. It has no UI dependency. It holds the requests (`Ask`, `Request`), the
  three providers, the review model (`Review`, `Change`, `Decision`), `Revision`, feedback
  (`feedback::Desk`, `Build`), settings with the writer's `Appearance`, and key storage. Its build script compiles the feedback
  records in `feedback/` into the crate.
- `books/` is `inkling-books`, ported from the reader's `reader-core` (ADR 0003). It has no UI
  dependency. It parses an EPUB (chapters from the navigation document, the cover it names) or a
  PDF with a text layer (pages in sections of ten, running headers and footers dropped) into one
  `Book` of chapters and blocks. `Library` keeps `library.json`, each parsed book as
  `books/<sha-256>.json` and covers in `covers/`, writing each file atomically, with each book's
  reading position. `scan` finds books in folders that the library doesn't have yet. The Library
  and Reading screens in `ui/` use it.
- `mcp/` is `inkling-mcp`, the MCP server of ADR 0004. It has no UI dependency. `Server` answers
  one JSON-RPC message at a time, whatever carries it. Its four tools (`list_books`, `read_book`,
  `search_library`, `read_draft`) only read, through a `Store`, and are defined once in
  `mcp/tools.json`, which the planned Worker serves too. `Folder` is the store over the data
  folder, read afresh on every call so a book the app just added shows at once. The binary,
  `inkling-mcp --stdio`, is what Claude Desktop and Claude Code start on the Mac. Until the
  Integrations screen exists, `inkling-mcp sign-in` and `inkling-mcp sync` put the Mac on the
  server: `signin` is an OAuth client with a one-time listener on 127.0.0.1, which keeps its refresh
  token in the Keychain, and `sync` sends up the books, positions and draft the server lacks and
  saves waiting Readwise files in `~/Downloads/Readwise` for Find books. It writes nothing in the
  data folder, because the running app owns `library.json`.
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
`core::appearance::Appearance` is one look: Light, Dark, Custom or one of the paper tones (Paper,
Sepia, Sage, Dusk, Night), the four custom colours, the body face, text size, line spacing, page width, and the Markdown and maths switches. It lives in `settings.json` and is
read leniently: a missing field or one that can't be read takes its default, and every number is
clamped, so neither an old file nor a hand edit can load an invalid look or cost the other
settings. `Appearance::tokens` derives every colour the views draw; Light is exactly the table in
`DESIGN.md`.

`Settings` holds two: `appearance` for writing and `reading.look` for the library and books, with
`reading.synced` and `reading.pages` (Scroll or Paged) beside it (ADR 0004). `Settings::change_look`
is the one way to change a look: while synced it gives the other view the same colours, face, sizes
and width, and never touches writing's Markdown and maths switches. A file without `reading`, or
with junk for it, loads with reading as a copy of writing, synced.

`ui::theme::Theme::new` turns the appearance into GPUI colours and the body's face and sizes, and
is the one place views read them. `Inkling::theme` is the look of the view on screen
(`Inkling::view`: reading while the library or a book is open). The Mac uses it in a face its text
system has (asked once at launch); the iPhone always gets the default look at its own sizes.
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
host lends it a `BookHost`: the folders Find books searches and a `pick` callback that shows its
open panel and resolves to the files chosen. `ui/` never reads the platform, so without a host the
library has no Add and no Find books. The iPhone host doesn't pass one yet.

- **Library** (`ui/src/library.rs`). The books newest first, each with its cover or its title on a
  plain tile, and a progress line once started. Add calls `pick`; Find books runs `scan` over the
  host's folders and lists what isn't in the library, EPUBs ticked and PDFs not. Each file is parsed
  and stored by `inkling_books::prepare` on the background executor, then recorded with
  `Library::add` on the UI thread. A file that can't be added shows `ImportError`'s words.
- **Reading** (`ui/src/reading.rs`). The book is loaded on the background executor, then one
  chapter at a time is set in reading's look: the body face, size, line spacing, page width and
  colours. It draws the chapter title, headings, quotes and scene breaks, a link back a chapter and
  Next at the end, a Contents list and a progress line (the chapter and how far through the book).
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

## What is open
`Inkling::screen` is one `Screen`: the page, the key screen, the feedback screen, the library or a
book being read. The library carries `Option<Finding>` while Find books is open over it.
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
- The draft and settings are files under the app's data folder (`Paths`). The page autosaves 500 ms
  after the last edit. Settings hold the provider, the models and the appearance.
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
  writer ticks it. It then sends the browser to Sign in with Apple, and grants only the Apple ID in
  `OWNER_APPLE_SUB`.
- **MCP.** `src/mcp.ts` serves `mcp/tools.json` and answers each tool from D1, with the same text as
  `inkling-mcp`. Each chapter is stored as the Markdown `read_book` gives, in a trigram FTS5 index,
  so a search runs inside D1. It finds one hit per chapter, and a query under three characters
  falls back to `instr`.
- **Sync.** `src/sync.ts`. A device uploads a book's parsed JSON to KV, then each chapter, then its
  row, which is refused until the rest is there. A position carries when it changed (an entry's
  `position_at`), and the later wins. A draft save carries when it was saved; the later wins, and
  20 versions are kept. Both times are milliseconds since the Unix epoch.
- **Readwise.** `src/readwise.ts`. The token and webhook secret are kept as AES-GCM ciphertext under
  `SECRET_KEY`. A webhook, or the poll every 30 minutes, streams each new EPUB or PDF from Reader
  into KV as an inbox item. A device downloads it and settles the item; for now `inkling-mcp sync`
  leaves the file for Find books, and the writer adds it. Only documents saved after the link was
  made come in.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.
Feedback goes to the T3 webhook in the writer's feedback link, and only the feedback text and
where it was sent from. An agent the writer connects through `inkling-mcp` reads the library and
the draft, and the agent's own app decides where that goes. Inkling's own server (above) holds the library and the draft for agents away from the Mac and
for sync. The Mac syncs to it with `inkling-mcp sync`. It calls Readwise with the writer's token and Apple to sign in.

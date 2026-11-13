# 0004. Integrations: an MCP server, Readwise, and a server of Inkling's own

Status: Proposed, 2026-11-13

## Context

On 2026-11-13 Toby asked for an Integrations option with three parts: an MCP server that an agent
such as Claude or ChatGPT can connect to, Readwise so that what he uploads to Reader lands in
Inkling's library, and a hosted backend on Railway with storage and OAuth sign-in to support them.

Inkling has no account and no server (PRODUCT.md), and ADR 0003 left the reader's sync out for that
reason. This decision is the one ADR 0003 said bringing sync back would need.

Not every part needs a server:

- **An agent on the Mac.** Claude Desktop and Claude Code start a local MCP server as a process and
  talk to it over stdio. That process can read the data folder directly. No server, no account.
- **Agents anywhere else.** Claude on the web and iPhone, and ChatGPT, connect only to a remote MCP
  server at a public HTTPS address, and sign in to it with OAuth 2.1 (the MCP authorization spec).
  An address with no sign-in would hand the library and the draft to anyone who found it. That
  server needs its own copy of the library and the draft. This is what needs Railway.
- **Readwise.** Reader's API takes a personal access token (there is no Readwise OAuth). Listing
  documents with `withRawSourceUrl=true` gives a link to the original EPUB or PDF, valid for an
  hour. Reader also sends webhooks, including `reader.non_feed_document.created` when a document
  is added by hand. The app could poll Reader itself, but only while it is open, once per device,
  and with the token on every device. A server can take the webhook and import once.

## Decision

### Three phases

1. **Local MCP, no infrastructure.** A new crate, `mcp/` (`inkling-mcp`), with no UI dependency,
   holds the tools over a `Store` trait. `inkling-mcp --stdio` serves them from the data folder on
   the Mac, for Claude Desktop and Claude Code. Nothing leaves the Mac except what the agent reads.
2. **The server.** A new crate, `server/` (`inkling-server`), in Rust on axum, deployed to Railway.
   It is the OAuth authorization server, the remote MCP endpoint (Streamable HTTP at `/mcp`, the
   same tools over a Postgres-backed `Store`), the sync API and the Readwise importer.
3. **The app.** An Integrations screen, reached from the cog, with Sign in, Sync, Readwise and
   "Connect an agent" (the MCP address to paste into Claude or ChatGPT). Signed out, the screen
   shows only the local MCP command and nothing leaves the device.

### The tools

- `list_books`, `read_book` (one chapter at a time), `search_library` (text across books) and
  `reading_position`.
- `read_draft`.
- `propose_changes`: the agent sends a revised draft and a one-line note. It never writes the
  draft. The app shows the proposal and opens it as a `Review` against the draft it was based on,
  so each change is the writer's to accept, as PRODUCT.md requires. If the draft has changed since,
  the proposal is shown as out of date and is not applied.
- `add_book` (server only): a URL to an EPUB or PDF, imported like a Readwise upload.

Each tool sits behind a scope (`library:read`, `draft:read`, `draft:propose`, `library:write`), and
the consent page says which the agent asked for. The draft scopes are off unless the writer turns
on "Agents can read the draft".

### Sign-in

- **One authorization server, in `inkling-server`.** OAuth 2.1 with PKCE, short-lived access
  tokens and rotating refresh tokens, stored only as hashes. It publishes the MCP protected
  resource metadata (RFC 9728) and its own metadata (RFC 8414).
- **Four kinds of client.** Claude and ChatGPT register themselves (dynamic client registration,
  RFC 7591, or a client ID metadata document). The Mac and iPhone apps are public clients that sign
  in through the system browser sheet (`ASWebAuthenticationSession`) with PKCE.
- **Sign in with Apple is the identity.** The sign-in page uses it on the web, and it is native on
  both devices. A third-party login alone (GitHub, Google) would also need Apple's under App Store
  guideline 4.8. There is one account: the server accepts only the Apple subject in its
  `OWNER_APPLE_SUB` variable, so nobody else can sign up.

### Storage on Railway

- **Postgres** for the account, OAuth clients, grants and token hashes, the library's rows and
  positions, the draft and its last 20 versions, proposals, and the Readwise link and cursor.
- **A Railway bucket** (S3-compatible) for each book's original file, its parsed JSON and its cover,
  under the book's SHA-256, as on the device.
- The Readwise token is encrypted with AES-256-GCM under a key held only in a Railway variable.
  AI keys and the feedback link never leave the Keychain and are never sent to the server.

### Sync

- Optional, and off until the writer signs in. It reuses the reader's model
  (`reader/crates/reader-core/src/sync.rs`): a book's id is the hash of its file and books are only
  added, so they can't conflict; a reading position is stamped and the later stamp wins.
- The draft is uploaded after autosave while signed in. The later save wins, and the server keeps
  the last 20 versions so a lost edit can be got back.
- Settings and appearance do not sync. Each device keeps its own look, as today.

### Readwise

- The writer pastes a token from readwise.io/access_token on the Integrations screen. It goes to the
  server once and is not kept on the device.
- The screen then shows a webhook address to add in Readwise's settings (Readwise has no API to
  add one) for `reader.non_feed_document.created`. The server checks the webhook's secret.
- On a webhook, and every 30 minutes as a backstop (`updatedAfter`, within Reader's 20 requests a
  minute), the server takes each new document of category `epub` or `pdf`, downloads its original
  through `raw_source_url`, runs `inkling_books::prepare` on it and adds it to the library. Devices
  get it on their next sync. A file that can't be parsed is listed on the Integrations screen with
  `ImportError`'s words.
- Articles, highlights and sending positions back to Reader are left out of this decision.

### Deployment

- One Railway project, `inkling`, with a `server` service built from `server/Dockerfile` on each
  push to `main`, a Postgres database and a bucket, at a Railway-generated `*.up.railway.app`
  address until there is a custom domain.
- Secrets are Railway variables: the token encryption key, the Apple sign-in key, `OWNER_APPLE_SUB`.
- For one writer it should stay near the Hobby plan's included usage (5 USD a month at the time of
  writing). The first week's metrics are checked against that.
- `core` and `books` already compile on Linux, and `mcp` and `server` will too. The server's logic (token checks, the
  Readwise parser, the sync plan) is tested without a database, in CI with the rest.

### What changes elsewhere

- ADR 0003's "No sync" is replaced by this decision's optional sync.
- PRODUCT.md's positioning becomes: no account is needed; signing in is optional and turns on
  sync, Readwise and agents anywhere; signed out, nothing leaves the device. Writing still goes to
  an AI provider only with the writer's key, and only to the one they picked.
- `docs/ARCHITECTURE.md` gains the two crates, and Readwise and Inkling's server under External
  services.

## Alternatives rejected

- **Only a local MCP server.** No server to run, but no ChatGPT, and no Claude on the iPhone or the
  web.
- **An MCP tool that edits the draft.** Faster for the agent, but the AI would change the draft on
  its own.
- **The reader's Cloudflare Worker.** It already syncs a library, and it is cheaper. But the server
  must parse EPUBs and PDFs with `inkling-books`, download files of tens of megabytes and keep a
  timer, which fits a long-running Rust container better than a Worker, and Toby asked for Railway.
- **A hosted sign-in service** (Supabase Auth, Clerk, WorkOS). One more vendor and account for one
  writer, when the authorization server is a few hundred lines over a library.
- **Readwise polled by the app.** No server, but only while the app is open, once per device, and
  the token would be on every device.
- **A Railway volume instead of a bucket.** A volume belongs to one service and has to be backed up
  with it. Book files are immutable blobs, which is what a bucket is for.

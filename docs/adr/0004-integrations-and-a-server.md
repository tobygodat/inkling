# 0004. Integrations: an MCP server, Readwise, and a server of Inkling's own

Status: Accepted, 2026-11-13. Revised the same day for Cloudflare in place of Railway.

## Context

On 2026-11-13 Toby asked for an Integrations option with three parts: an MCP server that an agent
such as Claude or ChatGPT can connect to, Readwise so that what he uploads to Reader lands in
Inkling's library, and a hosted backend with storage and OAuth sign-in to support them. He first
named Railway. Railway's trial on his account had expired, and the same day he chose Cloudflare's
free plan instead, where the reader's sync Worker (`reader/server`) already runs.

Inkling has no account and no server (PRODUCT.md), and ADR 0003 left the reader's sync out for that
reason. This decision is the one ADR 0003 said bringing sync back would need.

Not every part needs a server:

- **An agent on the Mac.** Claude Desktop and Claude Code start a local MCP server as a process and
  talk to it over stdio. That process can read the data folder directly. No server, no account.
- **Agents anywhere else.** Claude on the web and iPhone, and ChatGPT, connect only to a remote MCP
  server at a public HTTPS address, and sign in to it with OAuth 2.1 (the MCP authorization spec).
  An address with no sign-in would hand the library and the draft to anyone who found it. That
  server needs its own copy of the library and the draft.
- **Readwise.** Reader's API takes a personal access token (there is no Readwise OAuth). Listing
  documents with `withRawSourceUrl=true` gives a link to the original EPUB or PDF, valid for an
  hour. Reader also sends webhooks, including `reader.non_feed_document.created` when a document
  is added by hand. The app could poll Reader itself, but only while it is open, once per device,
  and with the token on every device. A server can take the webhook and fetch the file once.

Cloudflare's free plan (at the time of writing) allows 100,000 Worker requests a day, 10 ms of CPU
per request and 50 outgoing requests per request; KV holds 1 GB with values up to 25 MiB and takes
1,000 writes a day; D1 holds 5 GB and takes 100,000 row writes a day. 10 ms of CPU is far too
little to parse an EPUB or PDF, or to search a library by reading every book. Those limits shape
the decision below.

## Decision

### Three phases

1. **Local MCP, no infrastructure.** A new crate, `mcp/` (`inkling-mcp`), with no UI dependency,
   holds the tools over a `Store` trait. `inkling-mcp --stdio` serves them from the data folder on
   the Mac, for Claude Desktop and Claude Code. Nothing leaves the Mac except what the agent reads.
2. **The server.** `server/` is `inkling-server`, a Cloudflare Worker in TypeScript, built the way
   `reader/server` is. It is the OAuth authorization server, the remote MCP endpoint (Streamable
   HTTP at `/mcp`), the sync API and the Readwise inbox. It never parses a book: the devices do.
3. **The app.** An Integrations screen, reached from the cog, with Sign in, Sync, Readwise and
   "Connect an agent" (the MCP address to paste into Claude or ChatGPT). Signed out, the screen
   shows only the local MCP command and nothing leaves the device.

### The tools

- `list_books`, `read_book` (one chapter at a time, with the position in `list_books`),
  `search_library` (text across books) and `read_draft`. `mcp/tools.json` defines them once, and
  both `inkling-mcp` and the Worker serve that file, so an agent sees the same tools on the Mac and
  away from it.
- `propose_changes`: the agent sends a revised draft and a one-line note. It never writes the
  draft. The app shows the proposal and opens it as a `Review` against the draft it was based on,
  so each change is the writer's to accept, as PRODUCT.md requires. If the draft has changed since,
  the proposal is shown as out of date and is not applied.
- `add_book` (server only): a URL to an EPUB or PDF, put in the inbox like a Readwise upload.

Each tool sits behind a scope (`library:read`, `draft:read`, and later `draft:propose` and
`library:write`). The consent page lists them as boxes to tick, and the draft's box starts
unticked, so an agent reads the draft only when the writer ticks it.

### Sign-in

- **One authorization server, in the Worker,** on Cloudflare's `workers-oauth-provider`: OAuth 2.1
  with PKCE, dynamic client registration (RFC 7591), the MCP protected resource metadata (RFC 9728)
  and its own metadata (RFC 8414), with tokens kept only as hashes in KV.
- **Four kinds of client.** Claude and ChatGPT register themselves. The Mac and iPhone apps are
  public clients that sign in through the system browser sheet (`ASWebAuthenticationSession`) with
  PKCE.
- **Sign in with Apple is the identity.** The sign-in page uses it on the web, and it is native on
  both devices. A third-party login alone (GitHub, Google) would also need Apple's under App Store
  guideline 4.8. There is one account: the Worker accepts only the Apple subject in its
  `OWNER_APPLE_SUB` variable, so nobody else can sign up. The reader's email-and-password accounts
  are not reused.

### Storage on Cloudflare

- **D1** for the account, the library's rows and positions, each chapter's text (as `read_book`
  gives it) in an FTS5 index, the draft and its last 20 versions, proposals, the Readwise link and
  cursor, and the inbox. A tool call is one or two queries, and the search runs inside D1, so no
  request comes near 10 ms of CPU.
- **KV** for each book's parsed JSON and its cover, under the book's SHA-256 as on the device, and
  for each inbox file until a device has imported it. OAuth's own records live in KV too.
- The Readwise token is encrypted with AES-256-GCM (WebCrypto) under a key held only in a Worker
  secret. AI keys and the feedback link never leave the Keychain and are never sent to the server.

### Sync

- Optional, and off until the writer signs in. It reuses the reader's model
  (`reader/crates/reader-core/src/sync.rs`): a book's id is the hash of its file and books are only
  added, so they can't conflict; a reading position is stamped and the later stamp wins.
- The device that imports a book uploads it: its row, its parsed JSON and cover to KV, and its
  chapters to D1, one request per chapter so each stays small. Another device downloads the parsed
  JSON instead of parsing again.
- The draft is uploaded after autosave while signed in. The later save wins, and the server keeps
  the last 20 versions so a lost edit can be got back.
- Settings and appearance do not sync. Each device keeps its own look, as today.

### Readwise

- The writer pastes a token from readwise.io/access_token on the Integrations screen. It goes to the
  server once and is not kept on the device.
- The screen then shows a webhook address to add in Readwise's settings (Readwise has no API to
  add one) for `reader.non_feed_document.created`. The Worker checks the webhook's secret.
- On a webhook, and on a Cron Trigger every 30 minutes as a backstop (`updatedAfter`, within
  Reader's 20 requests a minute), the Worker takes each new document of category `epub` or `pdf`,
  streams its original from `raw_source_url` into KV, and adds it to the inbox. Streaming costs
  almost no CPU.
- The next device to sync downloads each inbox file, runs `inkling_books::prepare` on it, uploads
  the book as above and clears the item. So a Readwise upload reaches the library when a device next
  syncs (at launch and every few minutes while the app is open), and reaches remote agents once a
  device has imported it.
- A file over KV's 25 MiB is listed as too large on the Integrations screen; a file that can't be
  parsed is listed with `ImportError`'s words. Turning on R2 would lift the size limit.
- Articles, highlights and sending positions back to Reader are left out of this decision.

### Deployment

- `wrangler deploy` from `server/`, to `inkling-server` on Toby's `workers.dev` subdomain until
  there is a custom domain. CI deploys it on each push to `main` that changes `server/`, with a
  Cloudflare API token in the repository's secrets.
- Secrets are Worker secrets: the token encryption key, the Apple sign-in key, `OWNER_APPLE_SUB`.
- It runs on the free plan. Nothing in it needs the paid plan's longer CPU time.
- The Worker's tests run with Bun against `wrangler dev`, as `reader/server`'s do, in a CI job of
  their own on Linux.

### What changes elsewhere

- ADR 0003's "No sync" is replaced by this decision's optional sync.
- PRODUCT.md's positioning becomes: no account is needed; signing in is optional and turns on
  sync, Readwise and agents anywhere; signed out, nothing leaves the device. Writing still goes to
  an AI provider only with the writer's key, and only to the one they picked.
- `docs/ARCHITECTURE.md` gains `mcp/` and `server/`, and Readwise and Inkling's server under
  External services.

## Alternatives rejected

- **Only a local MCP server.** No server to run, but no ChatGPT, and no Claude on the iPhone or the
  web.
- **An MCP tool that edits the draft.** Faster for the agent, but the AI would change the draft on
  its own.
- **Railway, a Rust container with Postgres and a bucket.** Toby's first choice. The server could
  parse books itself and reuse `inkling-books`, but it costs 5 USD a month and his trial had
  expired.
- **Google Cloud Run with Neon Postgres.** Free within limits and close to the Railway design, but
  it needs a Google Cloud billing account, and Cloudflare already runs the reader's Worker.
- **Render or Koyeb's free tiers.** They sleep when idle and take up to a minute to wake, too slow
  for a webhook or an agent's first call.
- **The Mac as the server, through a tunnel.** Free and with no copy of anything, but only while the
  Mac is awake, and nothing for the iPhone.
- **Rust on Workers (`workers-rs`) to parse on the server.** It would reuse `inkling-books`, but
  parsing a book takes far more than 10 ms of CPU.
- **A hosted sign-in service** (Supabase Auth, Clerk, WorkOS). One more vendor and account for one
  writer, when `workers-oauth-provider` does the protocol.
- **Readwise polled by the app.** No server, but only while the app is open, once per device, and
  the token would be on every device.

# inkling-server

The Cloudflare Worker of ADR 0004. It runs on the free plan and handles four things:

- the OAuth sign-in, through Sign in with Apple, for one owner;
- the remote MCP endpoint at `/mcp`;
- the devices' sync API under `/v1`;
- the Readwise inbox.

`docs/ARCHITECTURE.md` says how the pieces fit.

## Develop

```sh
cd server
bun install
bun run check   # types
bun run test    # vitest in workerd, with local D1 and KV
```

The tests sign in the way a real client does. Only Apple's token endpoint and Readwise are faked.

## Deploy, the first time

Run these on the Mac, where `wrangler` is signed in to Cloudflare. Deploying changes what agents
and devices reach, so only Toby runs it.

Steps 1, 2 and the `SECRET_KEY` in step 4 were done on 2026-11-13, and the database and namespace
ids are in `wrangler.jsonc`.

1. `bunx wrangler deploy` creates the D1 database and the two KV namespaces. The Worker is then at
   `https://inkling-server.tobygodat.workers.dev`, which is `PUBLIC_URL`. A first deploy can fail
   with "This Worker does not exist on your account [10007]" just after creating it; run it again.
   Put the ids it prints into `wrangler.jsonc`.
2. `bunx wrangler d1 migrations apply inkling --remote` creates the tables.
3. Set up Sign in with Apple at developer.apple.com, team Q6NAWA42Y2:
   - Under Identifiers, add a Services ID (for example `com.tobygodat.inkling.web`). Turn on Sign
     in with Apple for it, with the domain `inkling-server.tobygodat.workers.dev` and the return URL
     `https://inkling-server.tobygodat.workers.dev/apple/callback`.
   - Under Keys, add a key with Sign in with Apple. Download the `.p8` file and note its Key ID.
4. Set the secrets. Each command asks for the value; none of them goes in a file:
   ```sh
   openssl rand -base64 32 | bunx wrangler secret put SECRET_KEY
   bunx wrangler secret put APPLE_CLIENT_ID     # the Services ID
   bunx wrangler secret put APPLE_TEAM_ID       # Q6NAWA42Y2
   bunx wrangler secret put APPLE_KEY_ID
   bunx wrangler secret put APPLE_PRIVATE_KEY < AuthKey_XXXXXXXXXX.p8
   ```
5. Connect an agent once (below) and sign in. The page shows your Apple ID's subject. Set it with
   `bunx wrangler secret put OWNER_APPLE_SUB`. From then on that Apple ID is the only one that can
   sign in.

After that, `bun run deploy` deploys and applies any new migration.

## Connect an agent

- **Claude:** Settings, Connectors, Add custom connector, with the URL
  `https://inkling-server.tobygodat.workers.dev/mcp`.
- **ChatGPT:** add a connector in developer mode with the same URL.

The consent page lets you tick what the agent may read. The draft is unticked unless you tick it.

## Connect the Mac

Agents see only what a device has sent up. Until the Integrations screen exists, the Mac sends it
with `inkling-mcp`:

```sh
cargo install --path mcp --locked
inkling-mcp sign-in      # the browser opens; allow it and sign in with Apple
inkling-mcp sync         # run it again whenever you want the server to catch up
```

The sign-in is kept in the Keychain. `sync` sends up the books, reading positions and draft the
server doesn't have yet, and it never changes anything in the app's data folder.

## Readwise

`inkling-mcp readwise` asks for the token from readwise.io/access_token and gives the webhook
address. Add a webhook at readwise.io/webhook for "Reader: new non-feed document" with that address
and paste its secret when asked, so uploads arrive within a minute; without it the server checks
every 30 minutes. Underneath, it is `PUT /v1/readwise` with `{"token": …, "webhook_secret": …}`.

Only documents saved after you connect come in. EPUBs and PDFs land in the inbox, and the next
`inkling-mcp sync` saves them in `~/Downloads/Readwise`, where Find books shows them.

# inkling-server

The Cloudflare Worker of ADR 0004 and ADR 0007. It runs on the free plan and handles:

- the OAuth sign-in, through Sign in with Google, for one owner;
- the remote MCP endpoint at `/mcp`;
- the devices' sync API under `/v1` (books, articles, documents, highlights, feeds and incoming pages);
- saving URLs from anywhere at `/save` and `/v1/save` (TOB-554);
- RSS and Atom feed polling on the 30-minute cron (TOB-554);
- Readwise Reader uploads in, and highlights out once each to Readwise (TOB-461, TOB-554);
- newsletters delivered to the Worker's `email()` handler through Cloudflare Email Routing (TOB-554);
- the iPhone builds' signed install links under `/builds` (TOB-518).

`docs/ARCHITECTURE.md` says how the pieces fit.

## Develop

```sh
cd server
bun install
bun run lint    # oxlint with anti-slop (tools/oxlint/anti-slop)
bun run check   # types
bun run test    # vitest in workerd, with local D1 and KV
```

The tests sign in the way a real client does. Only Google's token endpoint and Readwise are faked.

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
3. Make a Google OAuth client at console.cloud.google.com, in any project:
   - Under APIs & Services, OAuth consent screen (Google Auth Platform), set it up as External with
     an app name such as Inkling, and add your own Google account as a test user. Leaving it in
     Testing is fine for one person.
   - Under Clients, create a client of type Web application with the authorized redirect URI
     `https://inkling-server.tobygodat.workers.dev/google/callback`. Note its client ID and secret.
4. Set the secrets. Each command asks for the value; none of them goes in a file:
   ```sh
   openssl rand -base64 32 | bunx wrangler secret put SECRET_KEY
   bunx wrangler secret put GOOGLE_CLIENT_ID
   bunx wrangler secret put GOOGLE_CLIENT_SECRET
   bunx wrangler secret put OWNER_EMAIL         # your Google account's email
   ```
   From then on only the Google account with that email can sign in. Without `OWNER_EMAIL`,
   signing in only shows the email Google returned.

After that, `bun run deploy` deploys and applies any new migration.

## Connect an agent

- **Claude:** Settings, Connectors, Add custom connector, with the URL
  `https://inkling-server.tobygodat.workers.dev/mcp`.
- **ChatGPT:** add a connector in developer mode with the same URL.

The consent page lets you tick what the agent may read. The draft is unticked unless you tick it.

## Connect a device

Agents see only what a device has sent up. In the app, open Account from the cog (the end of the
Appearance sheet on iPhone) and Sign in with Google. The sign-in is kept in the Keychain, and from
then on the app syncs its documents, books and reading positions on its own.

## Readwise

On Account, Readwise's Connect asks for the token from readwise.io/access_token. Underneath, it is
`PUT /v1/readwise` with `{"token": …}`. The token is sealed with AES-256-GCM under `SECRET_KEY` and
never leaves the server. The 30-minute cron checks Reader for new EPUBs and PDFs saved after you
connect (which land in the inbox for a device to parse), and exports any unsent highlights made on
any device once each via `POST https://readwise.io/api/v2/highlights/` (also triggered immediately
when a device syncs new highlights).

## Save from anywhere

`POST /save` takes a URL from an Apple Shortcut, share sheet, or browser bookmarklet, fetches the
HTML without parsing it, and stages it in `incoming` so the next syncing device parses it with
`inkling-books` (TOB-534) into the Queue. Signed-in clients can also call `POST /v1/save` with
their OAuth `sync` token (and may pass pre-fetched `html` for paywalled pages).

Set the secret once before deploying:

```sh
openssl rand -hex 32 | bunx wrangler secret put SAVE_TOKEN
```

Without `SAVE_TOKEN`, `POST /save` refuses every request.

- **Apple Shortcut / curl:** `POST https://inkling-server.tobygodat.workers.dev/save` with header
  `Authorization: Bearer <SAVE_TOKEN>` and JSON body `{"url": "https://…"}`.
- **Bookmarklet:** submits an `application/x-www-form-urlencoded` form POST to `/save` with `url`
  and `token` in the POST body (never in the URL query string) and opens a plain confirmation page.

## Feeds

Devices follow RSS 2.0, Atom 1.0, and RSS 1.0/RDF feeds through `/v1/feeds` (`GET` to list,
`POST {"url": "…"}` with HTML `<link rel="alternate">` autodiscovery to follow, `DELETE /v1/feeds/<id>`
to unfollow). The 30-minute cron checks feeds with conditional GET (`If-None-Match`,
`If-Modified-Since`), deduplicates items in `feed_seen`, and stages new items in `incoming` with
`destination = "inbox"` so syncing devices parse them into the Inbox without duplicates.

## Newsletters (Cloudflare Email Routing)

The Worker exports an `email()` handler that accepts mail addressed to `NEWSLETTER_ADDRESS` in
`wrangler.jsonc` (default `read@tobygodat.com`, plus `read+<tag>@tobygodat.com` aliases), unpacks
the MIME message with `postal-mime`, deduplicates by `Message-ID`, and stages the HTML in `incoming`
with `destination = "inbox"` so syncing devices parse it into the Inbox as an article. Mail to any
other address is rejected during SMTP.

Set up Cloudflare Email Routing on `tobygodat.com` once, after deploying the Worker:

1. In the Cloudflare Dashboard, open zone `tobygodat.com` → **Email** → **Email Routing** and
   enable Email Routing. Cloudflare creates the zone's MX records (`route1`–`route3.mx.cloudflare.net`)
   and SPF TXT record (`v=spf1 include:_spf.mx.cloudflare.net ~all`).
2. Under **Email Routing** → **Routing rules** → **Custom addresses**, create a rule for
   `read@tobygodat.com` with action **Send to a Worker** → `inkling-server`.
3. Optional, for `+tag` aliases (`read+substack@tobygodat.com`): set the **Catch-all address** rule
   to **Send to a Worker** → `inkling-server` (the Worker rejects every recipient that doesn't match
   `NEWSLETTER_ADDRESS` or its `+tag` form).
4. Run `cd server && bun run deploy` to deploy the Worker and apply `migrations/0003_worker.sql`.

## Over-the-air builds

`apple/ota.sh` uploads a development-signed .ipa of the native SwiftUI iPhone app (`apple/`,
`com.tobygodat.inkling.dev`, "Inkling Dev") with `POST /builds?version=…&build=…` and the upload
token as a bearer token. TestFlight (`ios/testflight.sh`) stays on the GPUI iPhone app (`ios/`,
`com.tobygodat.inkling`, "Inkling"). The Worker stores each upload in the R2 bucket `inkling-builds`
under a random ID, deletes all but the newest five, and answers with a link to `/builds/<id>`. The
install page, its `manifest.plist` and `Inkling.ipa` are each refused unless their URL carries an
HMAC of its path and expiry, under a key derived from `SECRET_KEY`. A link works for 24 hours after
the upload, and the page and the manifest pass that same expiry on. Every response sends
`Cache-Control: no-store`. A Worker on the free plan takes request bodies of up to 100 MB, which
caps the .ipa.

Set it up once, on the Mac. Do it before the next deploy of any change: the deploy fails while the
bucket is missing.

1. Enable R2 in the Cloudflare dashboard (R2 Object Storage). On 2026-11-21 the account's API
   answered "Please enable R2 through the Cloudflare Dashboard." The free tier covers five builds.
2. Create the bucket: `bunx wrangler r2 bucket create inkling-builds`.
3. Make the upload token, keep it in the login Keychain, and give the Worker the same value. It
   never goes in a file. `security -i` reads its command from stdin, so the token never shows up in
   `ps`:
   ```sh
   printf 'add-generic-password -s inkling-ota -a upload-token -w %s\n' "$(openssl rand -hex 32)" | security -i
   security find-generic-password -s inkling-ota -a upload-token -w | bunx wrangler secret put BUILD_UPLOAD_TOKEN
   ```
   Without `BUILD_UPLOAD_TOKEN`, every upload is refused.
4. `bun run deploy`. Only Toby runs it.

Then `apple/ota.sh` works. `ios/README.md` has the rest, including registering the iPhone's UDID.

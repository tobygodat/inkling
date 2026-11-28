# Sign-in and sync

A signed-out device shows Sign in with Google. Once signed in it syncs documents, books, reading
positions, feeds and incoming pages with the Worker, connects Readwise with a token that only the
server keeps, and follows and unfollows feeds. Headless runs sign in to the sync crate's fake server
(`fake-server.json` in the data folder); the Worker's own side is proven in workerd.

## Sub-features

- `signin-out` signed out, Profile and Settings show Sign in with Google.
- `signin-in` signing in shows the newsletter address, Copy, and Sign out.
- `sync-down` staged feed posts, a newsletter and a saved link land in the Inbox under their filters; a remote document lands in Writing.
- `sync-up` a document written on the device reaches the server after Sync now.
- `readwise` Connect takes the token, and no file in the app's data folder holds it.
- `feeds` follow two feeds, unfollow one.
- `worker-signin` the Worker's OAuth: a challenge without a token, only the ticked scopes, anyone but the owner refused.
- `worker-sync` the Worker's library and documents: later positions win, a document only on its own version.

## How to get to it (user POV)

- iPhone: Profile, Sign in with Google; then Settings (the cog) for Readwise, feeds and Sync now.
- Mac: Settings, Account.

## Driving it with scripts/verify

Preconditions: the baseline in [README.md](README.md).

- **iPhone.** `scripts/verify iphone testSignInSyncInboxFeedsAndReadwise`. Look at
  `iphone/<appearance>-sign-in.png`, `-profile-signed-in.png`, `-settings-account.png` and
  `-inbox-feeds.png`. The journey reads `fake-server.json` back for `Written on iPhone`.
- **Mac.** `scripts/verify mac 'signInSyncInboxFeedsAndReadwise()'`. Look at `mac/<appearance>-sign-in.png`,
  `-settings-account.png`, `-settings-feeds.png` and `-inbox-feeds.png`.
- **Worker.** `scripts/verify worker test/signin.test.ts test/library.test.ts test/feeds.test.ts test/readwise.test.ts`.
  `worker-tests.log` ends with every file passed.

## Gotchas

- The app picks the fake server whenever `INKLING_FAKE_AI=1` is set, so no app journey reaches the
  local Worker, and Google sign-in can't run locally. The Worker's OAuth is proven only by its
  workerd tests, which fake Google's token endpoint.
- The Readwise check reads every file in the data folder except `fake-server.json`, which is the
  server's side and is meant to hold it.

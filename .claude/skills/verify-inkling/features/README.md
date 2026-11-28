# Inkling verification map

One file per user-facing feature: what proves it, which journeys drive it, and what to look at. Each
journey is an existing test in `apple/Snapshots` or `server/test`; `scripts/verify`
(`.claude/skills/verify-inkling/scripts/verify` from the repo root) runs it.

## Baseline

- `scripts/verify launch` is up and `scripts/verify doctor` prints only `ok` lines.
- Journeys seed their own data: the demo articles in `apple/Demo`, the fixture books from
  `books/fetch-fixtures.sh`, and `fake-server.json` for sync. None needs a prior journey.
- Every iPhone and Mac journey runs in Light, then Dark, and names its screenshots `<light|dark>-<step>`.

## Proof

- A feature is proven when every journey its file lists passed and the screenshots it names show
  what the file says.
- Report a journey that fails with the failure line from its log. Never report a feature as proven
  through a different journey, and never weaken a journey to make it pass.

## Features

- [Tabs and the Ask bar](tabs-ask-bar.md): the four tabs, the Ask pill, the + sheet, the composer, an
  answer with a citation.
- [Sign-in and sync](sign-in-sync.md): Sign in with Google, sync through the fake server, Readwise,
  feeds, and the Worker's side of both.
- [OTA install](ota-install.md): a Release build uploaded to the local Worker and the signed link
  chain an iPhone follows.
- [Inbox and reader](inbox-reader.md): reading, highlights, archive, appearance, chapters and footnotes.

# 0003. Reading joins Inkling

Status: Accepted, 2026-11-13

## Context

Toby built a minimalist EPUB and PDF reader for Mac and iPhone (TOB-402) as its own app, on Rune,
in `tobygodat/ios-reader`. On 2026-11-13 he chose to make Inkling one app that both writes and
reads (TOB-460), over keeping the reader as a second app in the same repo. Inkling is on GPUI Kit
(ADR 0001), so the reader's Rune UI can't be reused. Its core (`reader-core`) has no UI dependency.

The reader also has optional sign-in and sync through a Cloudflare Worker. Inkling has no account
and no server (PRODUCT.md).

## Decision

- **The reader's history moves in first.** `reader/` holds `ios-reader` with all its commits, so
  the ported code can be traced back.
- **One crate for books.** `reader-core`'s parsing, library and folder search become `books/`,
  `inkling-books`, in the root workspace. It has no UI dependency. It is a crate of its own, not a
  module of `inkling-core`, so the AI and writing code doesn't build the PDF and EPUB stack.
- **One look.** Reading uses Inkling's `Appearance` (Light, Dark or Custom, the body face, size and
  line spacing). The reader's page themes are not ported. Page mode (scroll or paged) is the one
  reading-only setting.
- **New screens in `inkling-ui`.** A Library and a Reading screen join `Screen`, on GPUI Kit, shared
  by the Mac and iPhone hosts. The hosts own file pickers and folder access, as they do for the
  data folder today.
- **No sync.** The reader's sync and account are left out, so Inkling still sends nothing to a
  server of its own. Bringing them back would be a new decision.
- **`reader/` goes.** It is deleted once Inkling reads books on both devices. `ios-reader` itself is
  not changed.

## Alternatives rejected

- **Two apps in one repo.** Cheap, but Toby asked for one app.
- **Keep Rune for reading inside the GPUI app.** Two UI frameworks, two text stacks and two event
  loops in one window, and Rune has no iPhone text input (Notion research, 2026-11-11).
- **Fold the parser into `inkling-core`.** Every core change would rebuild `lopdf`, `pdf-extract`
  and `zip`, and the review model's tests would compile them.

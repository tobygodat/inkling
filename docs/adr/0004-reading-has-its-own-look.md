# 0004. Reading has its own look, with a sync switch

Status: Accepted, 2026-11-13

## Context

ADR 0003 gave reading and writing one look: the writer's `Appearance`, with page mode as the only
reading setting. Once the Library and Reading screens were in (PR #12), Toby asked for more: the
Appearance panel should work on whichever view is open, reading should have customisation of its
own, and he should be able to keep the two in step with one click.

## Decision

- **Two looks.** `Settings` keeps the writing `Appearance` and a `ReadingLook`: its own
  `Appearance`, whether it is synced, and the page mode (Scroll or Paged). Both live in
  `settings.json` and are read leniently. A file from before this has no reading look, and loads
  with reading as a copy of writing, synced.
- **One switch to keep them in step.** "Same look for reading and writing" in the panel. While it
  is on, a change in either view shows in both; turning it on gives the other view the look on
  screen. The look is the colours, face, size, line spacing and page width. Writing's Markdown and
  maths switches and reading's page mode stay with their own view.
- **The panel follows the view.** It opens beside the page, the library or a book, and edits that
  view's look.
- **The reader's paper tones come back.** Paper, Sepia, Sage, Dusk and Night join Light, Dark and
  Custom as modes. Each meets 4.5:1 for text and labels. They are offered in both views, so a
  synced look is always one both views can draw.

## Alternatives rejected

- **Keep one look (ADR 0003).** It is what Toby asked to change.
- **Only reading-only settings (page mode, tones) on a shared look.** Writing in Dark and reading on
  Sepia would then be impossible.
- **Tones for reading only.** Sync would then have to translate a tone into something writing can
  draw, and the two views would no longer match while synced.

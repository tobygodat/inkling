# 0006. Books are Markdown, drawn by the reading view

Status: Accepted, 2026-11-15

## Context

A book in Inkling was plain text: `inkling-books` kept headings, paragraphs, quotes and scene
breaks, and dropped italics, images, footnotes, lists and links (TOB-483). The library kept only
the parsed book, so a better parser meant adding every book again. Toby wants a book to read as
well as in any other reader, and the next issues (TOB-488 highlights and quotes, TOB-489 Find)
need to point at a place in a chapter's text.

The issue asked for a spike: draw a chapter with the live page's block renderer (GPUI Kit's
`editor_host`, read-only), and keep it only if Paged mode works and a long chapter opens and
scrolls as fast as today; otherwise the reading view draws the new blocks itself.

## Decision

- **A chapter is Markdown.** `Chapter { title, text }`, where `text` is CommonMark with GFM
  footnotes, parsed by the `markdown` crate GPUI Kit uses. It opens with `# <title>`. An image is
  `![alt](images/<id>/<n>.<ext> "caption")`, a footnote `[^n]` with its definition at the
  chapter's end, a link inside the book `#<chapter>:<byte>`. `read_book` returns it as stored and
  sync sends the same text, so `mcp`'s own Markdown writer is gone.
- **A place is a byte offset.** Highlights, quotes and Find address a chapter's text by (chapter,
  byte offset into `text`), as `docs/CONVENTIONS.md` asks. Each laid-out block knows where it
  starts in `text`.
- **Book text is escaped, and a property proves it.** Whatever the book printed reads back out of
  the parser as the same text: no stray `*`, no heading, list, formula, code or autolink the book
  didn't have. Emphasis is written only where it will parse as emphasis.
- **The original is kept.** The library keeps the file as added (`originals/`) and the chapters'
  images (`images/<id>/`). `Book::parser` names the parser that wrote it; a book from an older
  parser is parsed again from its original when it is opened or added again, and its place
  follows its chapter by title. A book stored before this, with no original, reads its old blocks
  as Markdown, so it opens at its place unchanged.
- **The reading view draws the blocks itself.** `ui/src/chapter.rs` parses a chapter once with
  GPUI Kit's parser into a flat list of blocks (a quote's paragraphs and a list's items each a
  block), and `ui/src/reading.rs` sets each in one `StyledText`, as it set plain paragraphs before.
  Pagination keeps cutting between whole lines.
- **Switching views keeps the book.** The book left for the page stays laid out in memory, so
  Cmd-1 and Cmd-2 never read it from disk again.

## The spike

Mac, release build, headless, a 17,840-word chapter of Pride and Prejudice with its italics. The
median of the three quietest of eight rounds on a shared machine (raw runs on the branch
`spike/kit-reader-tob-483`):

| Renderer | Open | Scroll frame | Page turn | Pages |
| --- | --- | --- | --- | --- |
| Before this (plain blocks) | 44.5 ms | 3.6 ms | 3.7 ms | 103 |
| The reading view's own blocks | 44.2 ms | 5.1 ms | 5.2 ms | 104 |
| The live page's renderer, read-only | 56.7 ms | 9.2 ms | 8.9 ms | 116 |

The live page's renderer took about two and a half times as long per frame to scroll and turn a
page, and it can only end a page between top-level blocks, so a long list or quote is cut at the
page's foot instead of after a whole line. It failed both conditions, so the reading view draws
the blocks itself. Its frame stays well inside a 60 Hz frame and opens as fast as before.

## Alternatives rejected

- **The live page's renderer, read-only.** It would have given TOB-488 and TOB-489 GPUI Kit's
  leaf geometry for free, but it is slower and pages worse (above).
- **Blocks with styled runs in JSON instead of Markdown.** One more format for agents and the
  server to read, and no way for a person or an agent to read a stored chapter as text.
- **Reading images out of the original on demand.** Saves the disk the extracted images take, but
  every draw would open the archive; the images are extracted once instead.

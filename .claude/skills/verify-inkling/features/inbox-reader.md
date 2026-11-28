# Inbox and reader

The Inbox lists articles still to read; swiping archives one. Opening an article or a book shows the
reader: pictures drawn, highlights that last through a relaunch, Appearance with paper tones,
bars that get out of the way, chapters with footnotes and `Next: …`.

## Sub-features

- `inbox-list` the Inbox, search at its top, Compact and Comfortable density.
- `inbox-archive` swipe to archive, and the item is in Profile's Archive.
- `reader-highlight` a highlight made in the reader is in Highlights and survives a relaunch.
- `reader-appearance` Sepia paper is kept through a relaunch; Auto paper follows Light or Dark.
- `reader-chapters` footnote marks, `Next: …` to the next chapter, Contents back.

## How to get to it (user POV)

- iPhone: Inbox, tap a row; Library, tap a book; Appearance in the reader's top bar.
- Mac: the sidebar's Inbox and Library.

## Driving it with scripts/verify

Preconditions: the baseline in [README.md](README.md).

- **iPhone.** `scripts/verify iphone testEveryScreenInLightAndDark testChaptersAndFootnotes testAutoPaperFollowsTheAppearanceChosenWhileReading`.
  Look at `iphone/<appearance>-inbox.png`, `-inbox-swipe.png`, `-reader-highlight.png`,
  `-reader-sepia.png`, `-archive.png` and `-highlights.png`.
- **Mac.** `scripts/verify mac 'everyScreenInLightAndDark()' 'archivingHighlightsAndThePlaceOutliveARelaunch()' 'chaptersAndFootnotes()'`.
  Look at `mac/<appearance>-reader-chapters.png` and `-reader-footnotes.png`.

## Gotchas

- The Library's books are the fixture EPUBs `books/fetch-fixtures.sh` downloads; launch fetches them.
  Without them the reader journeys fail at their first book.

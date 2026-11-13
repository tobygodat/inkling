# Design system: Inkling

The chosen direction is F2, "Tick or cross each change", on the Claude Design canvas
(https://claude.ai/artifact/U9EJjJmsZMn77dCjgEZya2), boards `Version-Inline` and `Version-Inline-iPhone`.

## Overview
A light page for writing. Asking the AI makes a new version of the draft. The version shows over the
draft as inline changes, and the writer accepts or rejects each one, or all the rest at once.

- The draft is never changed until a change is accepted.
- A version bar names the version, its source, the size of the change and how many are accepted.

## Colors
The writer picks Light, Dark, Custom or a paper tone (Paper, Sepia, Sage, Dusk, Night) on the
Appearance panel, once for writing and once for reading, or one look for both while "Same look for
reading and writing" is on. Every colour a view draws comes
from `inkling_core::Appearance::tokens`. Light is the default.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| Ground | `#FBFBF9` | `#15171A` | Window and page |
| Surface | `#FFFFFF` | `#1E2125` | Bars, fields, sheets, the panel |
| Ink | `#1F2328` | `#E8E6E1` | Text, primary buttons, icons |
| Muted | `#5C6168` | `#A3A7AD` | Secondary text |
| Hairline | `#DEDFDA` | `#34383E` | Borders |
| Segment | `#EEEFEB` | `#25282D` | Switch tracks, hovered and chosen rows |
| Added text / fill / line | `#116329` / `#DAFBE1` / `#8DD6A0` | `#7EE2A0` / `#12361F` / `#2EA043` | Insertions, accepted marks |
| Added count | `#1A7F37` | `#56D364` | "+n" in the version bar |
| Removed text / fill / line | `#82071E` / `#FFEBE9` / `#CF222E` | `#FFA198` / `#3D1418` / `#F85149` | Deletions, reject |
| Link (accent) | `#0969DA` | `#58A6FF` | Links, the panel's sliders |
| Selection | `#B6D4FE` | `#264F78` | Selected text |

- Custom takes four colours: Background (ground), Foreground (surface: bars, fields, chips, the
  panel), Text (ink) and Accent (link). Selection, muted, hairline and segment are mixed from them,
  and muted is pulled toward the text until it reads at 4.5:1. Changes use Light's or Dark's
  greens and reds, whichever suits the background.
- Changing one colour while Light or Dark is on switches to Custom, starting from the colours on
  screen. The custom colours are kept while another mode is on.
- Green and red appear only on changes and their controls.
- A removal is also struck through and an accepted change is also underlined, so neither depends on colour.

## Typography
- Body: Source Serif 4, 21 pt on Mac and 19 pt on iPhone, line height 1.75 and 1.65.
- On the Mac the writer can change the body: Source Serif 4 or a face the Mac has (Charter,
  Georgia, Iowan Old Style, Palatino, Avenir Next), 15 to 32 pt, line height 1.20 to 2.20. Titles
  follow the body face.
- Title: Source Serif 4 semibold, 36 pt on Mac and 28 pt on iPhone.
- UI: Inter, 13 pt on Mac and 14 to 15 pt on iPhone.

- Body text is never set in the UI face.

## Layout
- One centred column, at most 700 pt wide by default, 480 to 960 pt on the Appearance panel. No
  sidebar and no toolbar.
- Mac, top to bottom: version bar (only during review), the page, quick actions, the ask field, a footer.
- iPhone: the version bar at the top, the page, then a sheet for the change that was tapped.

- Nothing sits beside the text column (or a book's), except the Appearance panel. It is 300 pt wide on the left
  while it is open, because a look is judged on the page: the writer sees each change on the real
  draft as they make it, and closes the panel with Done.
- Mac corners: the cog and the Write and Read toggle at the top left, and the round feedback button
  at the bottom right, on every screen. The cog's menu opens below the cog and never reaches the
  column at 1280 by 800. Bars along the top start after the corner. On iPhone a 44 pt cog and the
  toggle sit at the top left on the page, the library and a book, outside a review. The cog there
  opens Appearance directly, as a sheet over the lower 60% of the screen with 44 pt rows, so the
  page above it shows each change. The iPhone draws the same choices a little smaller (19 pt at
  1.65 by default, scaled from the Mac's 21 at 1.75) across the full width, so its sheet has no
  page width.

## Components
- **Version bar.** Version name, source, added and removed counts, accepted count, "Back to draft",
  "Accept the rest".
- **Change.** A removal, an insertion, or a pair. On Mac it carries inline accept and reject buttons,
  and "n more" when there are other wordings. On iPhone a tap selects it and a sheet offers Keep and Use.
- **Quick actions.** Fix grammar, Find a word, Next line, Ideas.
- **Ask field.** One line, with Send.
- **Key screen.** Provider choice, masked key field, model field.
- **Cog.** A 32 pt button with the cog icon. Hover opens its menu and leaving closes it; a click pins
  it open until a second click, Escape or a choice. Rows: Library with the book icon (Writing with
  the pen while the books are open), Keep reading with the book icon once a book has been opened
  (not while one is open), Appearance with the artboard icon, and Provider with the chosen
  provider's logo, its name unwritten. The menu is as wide as its rows.
- **Library.** The title, Find books and Add at the top, then the books as 150 pt tiles, newest
  first: the cover or the title in the body face on a Segment tile, a progress line once started,
  then the title and the author or format and how far through. Find books lists what it found with
  a tick each and one Add button.
- **Reading.** "‹ Library", the book's title and Contents at the top, one chapter in reading's
  column, face, size and line spacing, and the chapter, how far through the book and, in pages, the
  page, in muted ink at the bottom. Scroll, or Paged: a page fills the area between the bars and
  ends on a whole line; Right, Space, Page Down and a click on the right third turn forward, Left,
  Shift-Space, Page Up and the left third turn back, Home and End go to the chapter's ends. Past
  either end of a chapter is the next or the one before. Contents lists the chapters under its
  button, the current one on Segment.
- **Write and Read toggle.** One pill on Segment holding two square halves, pen then book, 28 pt on
  the Mac and 44 pt on iPhone. The view on screen sits on a lighter square (the surface on a light
  ground, the hairline tone on a dark one) with its icon in ink; the other is muted. Write goes to the
  draft; Read goes back to where reading was left: the library, or the last book at its place (the
  library before any reading since launch). Keep reading in the cog's menu always opens the last book.
  Cmd-1 and Cmd-2 do the same; each half has a tooltip and a label, "Writing" and "Reading".
- **Feedback button.** 40 pt round, an ink outline and the message icon. Shown once a feedback link
  is saved.
- **Appearance panel.** Beside the page, the library or a book, editing that view's look. Header
  ("Appearance" and Done, after the corner, whose toggle names the view), "Same look for reading and writing" with the link
  icon and a switch, Light / Dark / Custom, the paper tones as "Aa" swatches in their own colours,
  Font, Text size, Line spacing, Page width, then Drawing (a Markdown switch and a Maths switch) for
  writing or Pages (Scroll / Paged) for reading, Colours, Reset. Every change shows at once and is
  saved. A switch hands the caret back to the page.
- **Colour picker.** Under the colour row it belongs to: ten swatches that suit that colour, then
  Custom, which adds a wheel (angle is hue, distance from the centre is strength), a lightness
  slider and a hex field.
- **Icons.** One colour, the ink. Lucide for the cog, message, artboard, robot, book, pen and link; Simple Icons for
  Claude and Gemini. ChatGPT shows the robot. Sources are in `ui/icons/SOURCES.md`.

- Inline buttons are 28 pt on Mac. No control on iPhone is under 44 pt.

## The live page
The page draws its Markdown and maths while the writer types, on the Mac and on iPhone. The draft
on disk stays plain Markdown. Every block is drawn except the one that holds the caret, which
shows its source.

- **Headings.** Semibold in the body face, larger than body text by level. A heading keeps its size
  while the caret is in it, so the line doesn't jump.
- **Lists.** A bullet or a number in muted ink, then the item indented.
- **Emphasis.** Bold, italic and struck-through text draw as they say.
- **Code.** In a monospaced face on a Segment fill. A block has a Hairline border and its language
  at the top right. Inline code takes the same fill.
- **Tables.** Hairline rules between rows and columns, and the header row on Segment. Cells follow
  the alignment row.
- **Links.** In Link (accent), underlined. A click puts the caret there. Links never open from the
  page.
- **Images.** A local file draws at its size, inside the column. A path is read relative to the
  draft's folder. A remote image draws as its alt text in brackets, muted, and is never fetched.
- **Maths.** `$…$` draws inline, sitting on the text's baseline. `$$…$$` draws centred, as its own block.
  Both are in Ink, at the body size. A formula that doesn't parse, and a price such as `$5`, stay text.
- **The caret's block.** Its source, with the Markdown syntax (`#`, `**`, `-`, `$`, the backticks)
  in muted ink and the words in Ink. A heading's source keeps the heading's size and code keeps
  its fill.
- **Markdown off.** Every block shows its source, in Ink, at the body size. Maths is off too,
  because `$` is Markdown here, so the Maths switch is disabled and keeps its value.
- **Maths off.** Formulas show their source. The rest still draws.
- **During review.** The review draws the source, syntax included, so every change sits on the
  bytes it replaces.
- **On iPhone.** Until the page is tapped it has no caret, so every block is drawn, and an empty
  page says "Start writing" in muted ink. A tap puts the caret on the tapped character and brings
  the keyboard up. The caret, the selection, its two handles and the edit menu (Cut, Copy, Paste,
  Select All) are iOS's own. Done draws every block again.

## Motion
- One duration, 200 ms, with a quick ease-out (quint), for anything that appears or goes.
- The cog's menu fades in while dropping 6 pt into place, and fades out the same way. It is short
  because the menu is reached for mid-sentence: it should feel instant and still show where it
  came from.
- The panel's switches slide their thumb with GPUI Kit's own spring.
- Nothing on the page itself moves.

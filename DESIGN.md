# Design system: Inkling

The chosen direction is F2, "Tick or cross each change", on the Claude Design canvas
(https://claude.ai/artifact/U9EJjJmsZMn77dCjgEZya2), boards `Version-Inline` and `Version-Inline-iPhone`.

## Overview
A light page for writing. Asking the AI makes a new version of the draft. The version shows over the
draft as inline changes, and the writer accepts or rejects each one, or all the rest at once.

- The draft is never changed until a change is accepted.
- A version bar names the version, its source, the size of the change and how many are accepted.

## Colors
The writer picks Light, Dark or Custom on the Appearance panel. Every colour a view draws comes
from `inkling_core::Appearance::tokens`. Light is the default and the iPhone's only look.

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

- Nothing sits beside the text column, except the Appearance panel. It is 300 pt wide on the left
  while it is open, because a look is judged on the page: the writer sees each change on the real
  draft as they make it, and closes the panel with Done.
- Mac corners: the cog at the top left and the round feedback button at the bottom right, on every
  screen. The cog's menu opens below the cog and never reaches the column at 1280 by 800.

## Components
- **Version bar.** Version name, source, added and removed counts, accepted count, "Back to draft",
  "Accept the rest".
- **Change.** A removal, an insertion, or a pair. On Mac it carries inline accept and reject buttons,
  and "n more" when there are other wordings. On iPhone a tap selects it and a sheet offers Keep and Use.
- **Quick actions.** Fix grammar, Find a word, Next line, Ideas.
- **Ask field.** One line, with Send.
- **Key screen.** Provider choice, masked key field, model field.
- **Cog.** A 32 pt button with the cog icon. Hover opens its menu and leaving closes it; a click pins
  it open until a second click, Escape or a choice. Rows: Appearance with the artboard icon, and
  Provider with the chosen provider's logo, its name unwritten. The menu is as wide as its rows.
- **Feedback button.** 40 pt round, an ink outline and the message icon. Shown once a feedback link
  is saved.
- **Appearance panel.** Header (artboard, "Appearance", Done), Light / Dark / Custom, Font, Text
  size, Line spacing, Page width, Colours, Reset. Every change shows at once and is saved.
- **Colour picker.** Under the colour row it belongs to: ten swatches that suit that colour, then
  Custom, which adds a wheel (angle is hue, distance from the centre is strength), a lightness
  slider and a hex field.
- **Icons.** One colour, the ink. Lucide for the cog, message, artboard and robot; Simple Icons for
  Claude and Gemini. ChatGPT shows the robot. Sources are in `ui/icons/SOURCES.md`.

- Inline buttons are 28 pt on Mac. No control on iPhone is under 44 pt.

## Motion
- One duration, 200 ms, with a quick ease-out (quint), for anything that appears or goes.
- The cog's menu fades in while dropping 6 pt into place, and fades out the same way. It is the
  only motion so far. It is short because the menu is reached for mid-sentence: it should feel
  instant and still show where it came from.
- Nothing on the page itself moves.

# Design system: Inkling

The chosen direction is F2, "Tick or cross each change", on the Claude Design canvas
(https://claude.ai/artifact/U9EJjJmsZMn77dCjgEZya2), boards `Version-Inline` and `Version-Inline-iPhone`.

## Overview
A light page for writing. Asking the AI makes a new version of the draft. The version shows over the
draft as inline changes, and the writer accepts or rejects each one, or all the rest at once.

- The draft is never changed until a change is accepted.
- A version bar names the version, its source, the size of the change and how many are accepted.

## Colors
| Token | Value | Use |
| --- | --- | --- |
| Ground | `#FBFBF9` | Window and page |
| Surface | `#FFFFFF` | Bars, fields, sheets |
| Ink | `#1F2328` | Text, primary buttons |
| Muted | `#5C6168` | Secondary text |
| Hairline | `#DEDFDA` | Borders |
| Added text / fill / line | `#116329` / `#DAFBE1` / `#8DD6A0` | Insertions, accepted marks |
| Removed text / fill / line | `#82071E` / `#FFEBE9` / `#CF222E` | Deletions, reject |
| Link | `#0969DA` | Links only |

- Green and red appear only on changes and their controls.
- A removal is also struck through and an accepted change is also underlined, so neither depends on colour.

## Typography
- Body: Source Serif 4, 21 pt on Mac and 19 pt on iPhone, line height 1.75 and 1.65.
- Title: Source Serif 4 semibold, 36 pt on Mac and 28 pt on iPhone.
- UI: Inter, 13 pt on Mac and 14 to 15 pt on iPhone.

- Body text is never set in the UI face.

## Layout
- One centred column, at most 700 pt wide. No sidebar and no toolbar.
- Mac, top to bottom: version bar (only during review), the page, quick actions, the ask field, a footer.
- iPhone: the version bar at the top, the page, then a sheet for the change that was tapped.

- Nothing sits beside the text column.

## Components
- **Version bar.** Version name, source, added and removed counts, accepted count, "Back to draft",
  "Accept the rest".
- **Change.** A removal, an insertion, or a pair. On Mac it carries inline accept and reject buttons,
  and "n more" when there are other wordings. On iPhone a tap selects it and a sheet offers Keep and Use.
- **Quick actions.** Fix grammar, Find a word, Next line, Ideas.
- **Ask field.** One line, with Send.
- **Key screen.** Provider choice, masked key field, model field.

- Inline buttons are 28 pt on Mac. No control on iPhone is under 44 pt.

## Motion
Not designed yet.

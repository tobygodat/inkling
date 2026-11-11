# The prototype slice

Every prototype builds this same slice on `inkling-core`, so the stacks can be compared on the real
thing. The prototypes are throwaway. The comparison is the deliverable.

## What it does

1. **Open.** The window shows the draft from `Paths::mac().load_draft()` in one centred column, at most
   680 pt wide. There is no toolbar and no sidebar.
2. **Write.** A multi-line editor with word wrap and scrolling: typing, Return, Backspace, arrow keys,
   Shift-arrow and mouse selection, Cmd-A, Cmd-C, Cmd-X, Cmd-V, Cmd-Z and Shift-Cmd-Z. Marked text from
   an input method works if the stack's text widget gives it.
3. **Autosave.** 500 ms after the last edit the draft is saved with `Paths::save_draft`. A relaunch
   shows the same text.
4. **Continue.** Cmd-J with no selection starts `Action::Continue` at the cursor. The answer streams in
   as ghost text after the cursor, in the muted colour, without changing the document. Tab accepts it
   through `Assist::accept`. Esc dismisses it and cancels the request. Typing also dismisses it.
5. **Rewrite.** Cmd-K with a selection opens a one-line command bar at the bottom of the window, with
   the placeholder "Tell Inkling what to change". Return starts `Action::Rewrite` for the selection. The
   selected text dims and the proposal follows it in the accent colour. Tab or Return accepts, Esc
   dismisses. Cmd-K with no selection does the same as Cmd-J.
6. **Keys.** Cmd-Comma opens settings: the three providers as one choice, a masked key field, and a
   model field whose placeholder is the provider's default model. Save writes the key through the
   `KeyStore` from `default_keys()` and the rest through `Settings::save`.
7. **Missing key.** When `backend_for` returns `Err(provider)`, settings opens on that provider.
8. **Failure.** `Assist::Failed(message)` shows the message in one muted line at the bottom until Esc
   or the next action.
9. **Footer.** One muted 12 pt line: the word count on the left, the provider's name on the right.

All AI state lives in `inkling_core::Assist`. A prototype renders it and forwards events to it. It
doesn't keep a second copy.

## Look

A baseline only. The real design comes from the direction Toby picks.

| Token | Value |
| --- | --- |
| Ground | `#F4F5F1` |
| Ink | `#1C211E` |
| Muted, ghost text | `#7C857E` |
| Accent | `#2F6B4F` |
| Hairline | `#DDE0DA` |
| Body | 18 pt, line height 1.6, a system serif (`Iowan Old Style`, else `Georgia`) |
| UI text | 13 pt system sans |

## Proof

Each prototype has `apps/<stack>/snap.sh`, which renders these into `docs/screenshots/<stack>/` with
`INKLING_FAKE_AI=1` and `INKLING_DATA_DIR` set to a temp folder, by scripted input, without taking focus:

- `01-empty.png`
- `02-typed.png`, three paragraphs typed by scripted key events, one long enough to wrap
- `03-continue-streaming.png`, mid-stream
- `04-continue-accepted.png`
- `05-rewrite-proposal.png`
- `06-settings.png`
- `07-missing-key.png`, run without `INKLING_FAKE_AI`

Look at every image before reporting. A blank or wrong image is a failure, not a screenshot.

## Report

Each builder reports, measured on this Mac and labelled as measured:

- Clean build time, and rebuild time after a one-line change to a view.
- Lines of app code, and the stripped release binary's size.
- What the stack gave for free and what had to be built by hand, above all in the editor.
- Whether input-method text, undo and the clipboard work, and how that was checked.
- Framework bugs and blockers, each with how it was worked around.
- Every part of the slice that isn't done or wasn't proved.

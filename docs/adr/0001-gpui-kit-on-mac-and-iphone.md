# 0001. GPUI Kit for the UI on both Mac and iPhone

Status: Accepted, 2026-11-11

## Context

Inkling needs a UI stack for a Mac app and an iPhone app, both in Rust, over one core crate. TOB-428
built the same writing slice on bare gpui-fast, GPUI Kit and Rune, and ran an iPhone text-input spike
on Rune and on GPUI Kit. The code and screenshots are at tag `prototypes-tob-428`.

Measured on the M5 Pro, with sccache off:

| | gpui-fast (bare) | GPUI Kit | Rune |
| --- | --- | --- | --- |
| App code, lines | 1,200 (editor 690) | 481 | 510 |
| Clean dev build | 24.1 s | 38.3 s | 32.9 s |
| Rebuild after a one-line edit | 1.0 s | 1.2 s | 1.1 s |
| Stripped release binary | 8.1 MB | 18.6 MB | 21.3 MB |

On the iPhone Simulator (iOS 27.2):

| | GPUI Kit | Rune |
| --- | --- | --- |
| Typing, Backspace, Return | Works | Works |
| Autocorrect, auto-capitalisation | Off | Works |
| Tapping a suggestion | Changes nothing | Works |
| Word selection, handles, Cut/Copy/Paste | Works, drawn by the kit | Missing |
| Loupe | Missing | Missing |
| Inline diff with tap to accept | Works | Not built |

Rune keeps text geometry private, so selection can't be added from an app. GPUI's input handler exposes
the text, the selection and the bounds of a range, so GPUI Kit's gaps are in the platform crate
(`longbridge/gpui-mobile`), which types through a hidden text view iOS can't read.

The chosen design (see `DESIGN.md`) reviews the AI's changes in a read-only view with tap targets, so
the AI never writes into the live editor.

## Decision

- The UI is GPUI Kit on both devices, in one shared view crate.
- On the Mac it runs on gpui-fast through GPUI Kit's `gpui-fast` feature.
- On iPhone it runs in a Swift UIKit host through `gpui-mobile`, pinned to a rev and patched where needed.
- The first iPhone task is a text-input bridge in `gpui-mobile` that shows iOS the document, to get
  autocorrect, working suggestions and, if possible, the loupe.

## Consequences

- One Rust UI codebase, at the price of living on a patched, experimental platform crate on iPhone.
- If the bridge doesn't give acceptable typing, the iPhone writing screen becomes a native
  `UITextView` over the same core. That would be a new ADR.
- Every platform crate must be on the same `gpui-pre` version as GPUI Kit. A GPUI Kit upgrade is also a
  `gpui-mobile` upgrade.
- iPhone runs on `gpui-pre`, not gpui-fast, so the two devices can differ in rendering behaviour.
- Rune and bare gpui-fast are not used. Rune is worth another look if it ships public text geometry
  or its own iOS text input.

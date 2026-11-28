# Tabs and the Ask bar

The iPhone app has four tabs (Inbox, Library, Writing, Profile) with a persistent Ask bar above them:
a pill at rest that names what is on screen, a + sheet of suggestions, a compact composer, and
answers that cite passages that open where they are. The Mac app has the same bar under its sidebar.

## Sub-features

- `tabs-four` the four tabs exist and switch.
- `ask-pill` at rest the bar is a pill labelled for the screen (`Ask about Walden` in its reader, `Ask` in the Inbox).
- `ask-add` + opens suggestions for the screen: Explain and Summarize in the reader, the writing actions in Writing.
- `ask-compose` tapping the pill opens the compact composer.
- `ask-cite` an answer cites a passage of Walden and the citation opens it.
- `ask-keeps` the conversation survives a tab switch and, on the Mac, a relaunch.
- `ask-fold` on the Mac the bar folds as the page scrolls on and opens when scrolling back.

## How to get to it (user POV)

- iPhone: Library, Walden, then the Ask pill; or + beside it.
- iPhone: any tab; the bar hides only in the immersive reader.
- Mac: the bar under the sidebar on any page.

## Driving it with scripts/verify

Preconditions: the baseline in [README.md](README.md).

- **iPhone Ask bar.** `scripts/verify iphone testAskBarAnswersWithACitationAcrossTabs`. Passes with
  `ok   apple/snap.sh iphone`. Look at `iphone/<appearance>-ask-bar.png` (the pill),
  `-ask-add.png` (the + sheet), `-ask-open.png` (the compact composer) and `-ask-answer.png`
  (the answer with citation 1).
- **iPhone tabs.** `scripts/verify iphone testEveryScreenInLightAndDark`. It asserts four tabs and
  the bar above them, and walks every tab. Look at `-inbox.png`, `-library.png` and `-profile.png`.
- **Mac.** `scripts/verify mac 'theAskBarAnswersFromTheLibraryAndKeepsOneConversation()' 'theAskBarFoldsWhileThePageScrollsOn()' 'aCancelledQuestionNeverAnswersTheNextOne()'`.
  Look at `mac/<appearance>-ask-bar.png`, `-ask-open.png`, `-ask-add.png`, `-ask-answer.png` and `-ask-folded.png`.

## Gotchas

- A new Simulator's first keyboard shows iOS's slide-to-type tip over the composer. `launch` turns it
  off; on a Simulator it didn't make, `-ask-open.png` shows the tip instead of the composer.
- `testEveryScreenInLightAndDark` is the longest iPhone journey. Run it when tabs or the screens it
  walks changed.
- The Mac journeys set the bar's state directly on its model and render offscreen; the iPhone
  journeys tap. Only the iPhone ones prove a gesture.

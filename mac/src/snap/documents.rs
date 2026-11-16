//! Headless proof of the documents: `draft.md` moved in at launch, New, the list, switching with
//! each document's undo history and review kept, Delete and Cmd-Z, the Write half, a relaunch,
//! and the list on iPhone with its Undo button.

use super::*;
use inkling_core::Documents;

const FIRST: &str = "# On keeping accounts\n\nEvery list i keep grows until it is teh work itself.";
const ADDED: &str = " Thoreau saw it coming.";

fn titles(s: &mut Snap) -> Vec<String> {
    s.read(|v, _| v.document_list().map(|(list, _)| list.iter().map(|d| d.title.clone()).collect()).unwrap_or_default())
}

pub(super) fn count(s: &mut Snap) -> Option<(usize, usize)> {
    s.read(|v, _| v.find_count())
}

pub(super) fn finding(s: &mut Snap) -> bool {
    s.read(|v, _| v.finding())
}

fn chosen(s: &mut Snap) -> Option<usize> {
    s.read(|v, _| v.document_list().and_then(|(_, chosen)| chosen))
}

fn open_id(s: &mut Snap) -> String {
    s.read(|v, _| v.open_document_id().to_string())
}

fn choose(s: &mut Snap, title: &str) {
    let at = titles(s).iter().position(|t| t == title).unwrap_or_else(|| fail(&format!("{title:?} is in the list")));
    s.click(("document", at));
}

pub fn run(dir: &Path) {
    let paths = Paths::mac();
    let docs = Documents::new(&paths);
    seed(&paths, FIRST);
    let mut s = Snap::open(dir);
    check(!paths.dir.join("draft.md").exists() && docs.read("draft").ok().as_deref() == Some(FIRST), "draft.md moves into documents/ at launch, unchanged");
    check(s.text() == FIRST, "the moved draft is the page");
    let link = s.bounds("open-documents").unwrap_or_else(|| fail("the page shows ‹ Documents"));
    let toggle = s.bounds("views").unwrap_or_else(|| fail("the toggle is laid out"));
    check(link.left() > toggle.right() && (link.center().y - toggle.center().y).abs() < px(2.), "‹ Documents sits after the corner, level with it");

    s.select(FIRST.len()..FIRST.len());
    s.input(ADDED);
    s.advance(501);
    let first = format!("{FIRST}{ADDED}");
    check(docs.read("draft").ok() == Some(first.clone()), "an edit autosaves into the document's own file");
    s.shot("documents-01-page");

    // Find on the page.
    s.press("cmd-f");
    let field = s.bounds("find-field").unwrap_or_else(|| fail("Cmd-F opens Find's field"));
    let toggle = s.bounds("views").unwrap_or_else(|| fail("the toggle is laid out"));
    check(field.left() > toggle.right() && field.top() < toggle.bottom(), "the field sits in the top bar, after the corner");
    s.input("KEEP");
    let keeps: Vec<_> = first.match_indices("keep").map(|(at, w)| at..at + w.len()).collect();
    check(count(&mut s) == Some((1, 2)) && s.selected() == keeps[0], "every match is found, ignoring case, the first in Selection: 1 / 2");
    s.shot("documents-07-find-page");
    s.press("enter");
    check(count(&mut s) == Some((2, 2)) && s.selected() == keeps[1], "Return goes to the next match");
    s.press("shift-enter");
    check(count(&mut s) == Some((1, 2)) && s.selected() == keeps[0], "Shift-Return to the one before");
    s.press("escape");
    check(!finding(&mut s) && s.page_focused() && s.selected() == keeps[0], "Escape closes the field and leaves the caret on the current match");

    s.press("cmd-n");
    let letter = open_id(&mut s);
    check(letter != "draft" && s.text().is_empty() && s.page_focused(), "Cmd-N opens an empty page with the caret in it");
    s.input("# Letter to the board");
    s.press("enter");
    s.press("enter");
    s.input("Dear all,");
    s.advance(501);
    check(docs.read(&letter).ok().as_deref() == Some("# Letter to the board\n\nDear all,"), "the new document is saved in a file of its own");
    check(docs.read("draft").ok() == Some(first.clone()), "writing in the new one leaves the first alone");

    s.click("open-documents");
    check(titles(&mut s) == ["Letter to the board", "On keeping accounts"], "the list shows the titles, newest first");
    check(chosen(&mut s) == Some(0), "the open document is on Segment");
    s.shot("documents-02-list");
    s.press("cmd-f");
    s.input("letter");
    check(titles(&mut s) == ["Letter to the board"], "Find on the list filters the documents by title");
    s.shot("documents-08-find-list");
    s.press("escape");
    check(!finding(&mut s) && titles(&mut s).len() == 2 && chosen(&mut s) == Some(0), "Escape shows every document again");

    s.press("down");
    s.press("enter");
    check(open_id(&mut s) == "draft" && s.text() == first, "the arrows choose a row and Enter opens it");
    for _ in 0..8 {
        if s.text() == FIRST {
            break;
        }
        s.press("cmd-z");
    }
    check(s.text() == FIRST, "the document's own undo history came back with it");
    s.advance(501);

    s.click("fix-grammar");
    s.wait(10_000, reviewing);
    check(s.review().draft() == FIRST, "asking sends only the open document");
    s.click("open-documents");
    choose(&mut s, "Letter to the board");
    s.press("enter");
    check(!reviewing(&s.revision()) && s.text().starts_with("# Letter"), "the other document opens without the first one's review");
    s.click("open-documents");
    choose(&mut s, "On keeping accounts");
    s.press("enter");
    check(reviewing(&s.revision()) && s.review().draft() == FIRST, "the first document's review waits for it");
    s.shot("documents-03-review-kept");
    s.press("escape");

    s.click("open-documents");
    let before = titles(&mut s);
    choose(&mut s, "Letter to the board");
    s.press("backspace");
    check(titles(&mut s) == ["On keeping accounts"] && !docs.path(&letter).exists(), "Delete removes the chosen document and its file");
    s.shot("documents-04-deleted");
    s.press("cmd-z");
    check(titles(&mut s) == before, "Cmd-Z brings it back in its place");
    check(docs.read(&letter).ok().as_deref() == Some("# Letter to the board\n\nDear all,"), "with every word");
    s.shot("documents-05-undone");

    s.press("cmd-1");
    check(!s.read(|v, _| v.documents_open()) && open_id(&mut s) == "draft", "Cmd-1 goes back to the document open last");
    drop(s);

    let mut s = Snap::open(dir);
    check(open_id(&mut s) == "draft" && s.text() == FIRST, "a relaunch opens the document open last");
    drop(s);

    let mut phone = Snap::open_as(dir, FormFactor::Phone);
    phone.click("open-documents");
    check(titles(&mut phone).len() == 2, "‹ Documents opens the list on iPhone");
    phone.click("open-find");
    check(finding(&mut phone) && phone.bounds("find-done").is_some(), "on iPhone the magnifier opens Find, with Done beside it");
    phone.input("ACCOUNTS");
    check(titles(&mut phone) == ["On keeping accounts"], "and filters the list");
    phone.shot("documents-09-iphone-find");
    phone.click("find-done");
    check(!finding(&mut phone) && titles(&mut phone).len() == 2, "Done closes it");
    phone.update(|v, w, cx| v.remove_document(1, w, cx));
    check(titles(&mut phone).len() == 1 && phone.bounds("undo-remove").is_some(), "a removed row leaves an Undo button at the bottom");
    phone.shot("documents-06-iphone-undo");
    phone.click("undo-remove");
    check(titles(&mut phone).len() == 2 && phone.bounds("undo-remove").is_none(), "Undo brings the document back and goes");
    phone.click("new-document");
    check(phone.text().is_empty() && phone.page_focused(), "New on iPhone opens an empty page with the caret in it");
}

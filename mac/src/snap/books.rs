//! Headless proof of the Library and Reading screens: the fixture books (`books/fetch-fixtures.sh`)
//! added through Add and Find books, a chapter read in Light and Dark, and the place kept across
//! a relaunch. The host's open panel and folders are stand-ins, so nothing reads the real Desktop.

use super::*;
use inkling_books::Format;
use inkling_core::{Face, Mode, PageMode, View};
use inkling_ui::BookHost;
use std::cell::RefCell;

const DRAFT: &str = "A draft about the ferry, left open while reading.";
const NOT_A_BOOK: &str = "“notes.txt” isn’t an EPUB or a PDF.";

/// What the stand-in open panel answers with next.
type Picked = Rc<RefCell<Vec<PathBuf>>>;

fn host(folders: Vec<PathBuf>, picked: &Picked) -> BookHost {
    let picked = picked.clone();
    BookHost { folders: Box::new(move |_| folders.clone()), pick: Box::new(move |_| Task::ready(picked.borrow().clone())), add_folder: None }
}

fn open(dir: &Path, folder: &Path, picked: &Picked) -> Snap {
    let mut s = Snap::open(dir);
    let host = host(vec![folder.to_path_buf()], picked);
    s.update(|v, _, cx| v.set_book_host(host, cx));
    s.frame();
    s
}

fn entries(s: &mut Snap) -> Vec<inkling_books::Entry> {
    s.read(|v, _| v.library_entries().to_vec())
}

fn position(s: &mut Snap) -> Option<inkling_books::Position> {
    s.read(|v, _| v.reading().map(|(_, at)| at))
}

fn set_mode(s: &mut Snap, mode: Mode) {
    s.update(|v, w, cx| v.set_appearance(|a| a.mode = mode, w, cx));
    s.frame();
}

/// Lets the background imports, loads and cover decodes finish.
fn settle_work(s: &mut Snap, until: impl Fn(&mut Snap) -> bool) {
    for _ in 0..200 {
        if until(s) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        s.advance(16);
    }
    s.frame();
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).expect("screenshot dir");
    let fixtures = PathBuf::from(std::env::var_os("INKLING_FIXTURES").unwrap_or_else(|| fail("INKLING_FIXTURES names the fixture folder")));
    let (epub, pdf) = (fixtures.join("pride-and-prejudice.epub"), fixtures.join("rfc2616.pdf"));
    check(epub.is_file() && pdf.is_file(), "the two fixture books are downloaded");
    let paths = Paths::mac();
    let folder = paths.dir.with_file_name("books-folder").join("Downloads");
    std::fs::create_dir_all(&folder).expect("the stand-in Downloads folder");
    for file in [&epub, &pdf] {
        std::fs::copy(file, folder.join(file.file_name().expect("a file name"))).expect("copy a fixture");
    }
    let notes = folder.join("notes.txt");
    std::fs::write(&notes, "Not a book.").expect("write notes.txt");
    let picked: Picked = Rc::default();

    let mut s = open(dir, &folder, &picked);
    s.input(DRAFT);
    let (cog, toggle) = (s.bounds("cog"), s.bounds("views"));
    check(
        cog.zip(toggle).is_some_and(|(c, t)| t.left() > c.right() && t.right() < px(140.) && (t.center().y - c.center().y).abs() < px(1.)),
        "the Write and Read toggle sits beside the cog",
    );
    check(s.bounds("show-writing").zip(s.bounds("show-reading")).is_some_and(|(w, r)| w.right() <= r.left()), "Write, then Read");
    s.shot("books-00-toggle-writing");
    s.click("show-reading");
    check(s.read(|v, _| v.library_open()), "Read with no book read yet opens the library");
    s.click("show-writing");
    check(!s.read(|v, _| v.library_open()) && s.page_focused() && s.text() == DRAFT, "Write goes back to the draft");
    s.press("cmd-2");
    check(s.read(|v, _| v.library_open()), "Cmd-2 is Read");
    s.press("cmd-1");
    check(!s.read(|v, _| v.library_open()) && s.text() == DRAFT, "and Cmd-1 is Write");
    s.click("cog");
    check(s.bounds("menu-library").is_some(), "the cog's menu offers the Library");
    s.click("menu-library");
    check(s.read(|v, _| v.library_open()), "Library in the cog's menu opens the library");
    s.hover("library");
    s.advance(200);
    check(entries(&mut s).is_empty() && s.bounds("library-empty").is_some(), "a new library says there is nothing to read yet");
    check(paths.dir.join("library").is_dir(), "the library lives under the data folder");
    s.shot("books-01-empty");
    set_mode(&mut s, Mode::Dark);
    s.shot("books-01-empty-dark");
    set_mode(&mut s, Mode::Light);

    *picked.borrow_mut() = vec![notes.clone()];
    s.click("add-book");
    settle_work(&mut s, |s| s.read(|v, _| v.adding() == 0));
    let errors = s.read(|v, _| v.library_errors().to_vec());
    check(errors == [NOT_A_BOOK] && s.bounds("library-message").is_some(), "a file that isn't a book shows ImportError's words");
    check(entries(&mut s).is_empty(), "and nothing is added");
    s.shot("books-02-not-a-book");

    *picked.borrow_mut() = vec![epub.clone()];
    s.click("add-book");
    check(s.read(|v, _| v.adding()) == 1 || entries(&mut s).len() == 1, "Add starts the import");
    settle_work(&mut s, |s| s.read(|v, _| v.adding() == 0));
    let shelf = entries(&mut s);
    check(shelf.len() == 1 && shelf[0].title == "Pride and Prejudice", "Add puts the EPUB from the open panel in the library");
    check(shelf[0].cover.is_some() && s.read(|v, _| v.library_errors().is_empty()), "with its cover, and the old error is gone");

    s.click("find-books");
    settle_work(&mut s, |s| s.read(|v, _| v.found_books().is_some()));
    let found: Vec<(Format, bool)> = s.read(|v, _| v.found_books().unwrap_or_default().iter().map(|(f, chosen)| (f.format, *chosen)).collect());
    check(found == [(Format::Pdf, false)], "Find books finds only the book not added yet, a PDF, unticked");
    s.click(("found", 0usize));
    let ticked = s.read(|v, _| v.found_books().is_some_and(|f| f.iter().all(|(_, chosen)| *chosen)));
    check(ticked, "a click ticks it");
    s.shot("books-03-find");
    s.click("add-found");
    settle_work(&mut s, |s| s.read(|v, _| v.adding() == 0));
    let shelf = entries(&mut s);
    check(
        shelf.len() == 2 && shelf[0].format == Format::Pdf && shelf[1].title == "Pride and Prejudice",
        "the ticked book is added and the library lists newest first",
    );
    check(s.bounds(("book", 0usize)).zip(s.bounds(("book", 1usize))).is_some_and(|(a, b)| a.right() < b.left()), "the books sit side by side");
    s.shot("books-04-library");

    s.click("cog");
    s.click("menu-appearance");
    s.hover("library");
    s.advance(200);
    let panel = s.bounds("appearance").unwrap_or_else(|| fail("Appearance opens over the library"));
    check(
        s.bounds(("book", 0usize)).is_some_and(|b| b.left() > panel.right()) && s.read(|v, _| v.view() == View::Reading),
        "Appearance opens beside the library, for reading",
    );
    s.click("appearance-done");

    s.click(("book", 1usize));
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    let target = s.read(|v, _| {
        let (book, _) = v.reading().unwrap_or_else(|| fail("a click opens the book"));
        book.chapters.iter().position(|c| c.title.as_deref().is_some_and(|t| t.to_lowercase().starts_with("chapter"))).unwrap_or(0)
    });
    check(position(&mut s) == Some(Default::default()), "the book opens at its start");
    s.click("contents");
    check(s.bounds(("chapter", target)).is_some(), "Contents lists the chapters");
    s.shot("books-05-contents");
    s.click(("chapter", target));
    check(position(&mut s).map(|at| at.chapter) == Some(target) && s.bounds("contents-list").is_none(), &format!("choosing chapter {} jumps to it", target + 1));
    let column = s.bounds("reading-column").unwrap_or_else(|| fail("the chapter is laid out"));
    check(column.size.width == px(700.), "the chapter is set in a 700 pt column");
    check(s.bounds("chapter-title").is_some_and(|t| t.top() > px(56.)) && s.bounds("prev-chapter").is_some(), "it starts at its title, with the way back a chapter above");

    // Scroll mode: the wheel, then the keys across both ends of the chapter.
    for _ in 0..3 {
        s.with(|w, cx| w.scroll("reading-body", ScrollDelta::Pixels(point(px(0.), px(-300.))), cx));
        s.frame();
    }
    let read = position(&mut s).unwrap_or_else(|| fail("the book is still open"));
    check(read.chapter == target && read.fraction > 0.05 && read.fraction < 0.95, &format!("the wheel moves through the chapter ({:.2})", read.fraction));
    s.advance(499);
    check(entries(&mut s)[1].position.fraction == 0., "the place isn't saved until 500 ms after the scroll");
    s.advance(2);
    check(entries(&mut s)[1].position == read, "the place is saved 500 ms after the last scroll");
    s.shot("books-06-reading");
    s.press("end");
    check(s.bounds("next-chapter").is_some_and(|n| n.bottom() < px(800.)), "End goes to the end of the chapter");
    s.press("space");
    check(position(&mut s).map(|at| (at.chapter, at.fraction)) == Some((target + 1, 0.)), "Space at the end of a chapter goes on into the next");
    s.press("shift-space");
    let back = position(&mut s).unwrap_or_default();
    check(back.chapter == target && back.fraction > 0.5, &format!("and Shift-Space at its start goes back to the end of the one before ({:.2})", back.fraction));
    for _ in 0..4 {
        s.with(|w, cx| w.scroll("reading-body", ScrollDelta::Pixels(point(px(0.), px(-200.))), cx));
        s.frame();
    }
    check(position(&mut s).map(|at| at.chapter) == Some(target + 1), "pushing the wheel on past the end goes into the next chapter");
    s.press("home");
    s.press("pageup");
    check(position(&mut s).map(|at| at.chapter) == Some(target), "Page Up at the top goes back a chapter");
    s.press("home");
    check(position(&mut s).map(|at| at.fraction) == Some(0.) && s.bounds("chapter-title").is_some_and(|t| t.top() > px(56.)), "Home goes to the chapter's start");

    // The panel beside the book, editing reading's look.
    s.click("cog");
    s.click("menu-appearance");
    s.hover("reading-body");
    s.advance(200);
    let panel = s.bounds("appearance").unwrap_or_else(|| fail("Appearance opens beside the book"));
    let column = s.bounds("reading-column").unwrap_or_else(|| fail("the book stays open"));
    check(panel.left() == px(0.) && column.left() >= panel.right() && s.read(|v, _| v.reading().is_some()), "Appearance opens beside the book, which stays open");
    check(s.read(|v, _| v.settings().reading.synced), "reading starts with the same look as writing");
    s.shot("books-07-panel-beside-the-book");
    s.click("sync");
    check(!s.read(|v, _| v.settings().reading.synced), "the link switch turns sync off");
    for tone in Mode::TONES {
        s.click(("mode", tone as usize));
        let (reading, writing) = s.read(|v, _| (v.settings().reading.look.mode, v.settings().appearance.mode));
        check(reading == tone && writing == Mode::Light, &format!("{} changes reading and leaves writing Light", tone.label()));
        s.shot(&format!("books-08-tone-{}", tone.label().to_lowercase()));
    }
    let saved = std::fs::read_to_string(paths.settings()).unwrap_or_default();
    check(saved.contains("\"night\"") && saved.contains("\"synced\": false"), "the reading look is saved in settings.json");
    s.click(("mode", Mode::Sepia as usize));
    s.click("sync");
    let (reading, writing) = s.read(|v, _| (v.settings().reading.look.clone(), v.settings().appearance.clone()));
    check(writing.mode == Mode::Sepia && writing.same_look(&reading), "turning sync on gives writing the look on screen");
    s.click(("face", 1usize));
    let (reading, writing) = s.read(|v, _| (v.settings().reading.look.face, v.settings().appearance.face));
    check(reading == writing && reading != Face::SourceSerif, "while synced a change in reading shows in writing");
    s.click(("mode", Mode::Light as usize));
    s.click(("face", 0usize));
    s.click("sync");
    s.click(("mode", Mode::Sepia as usize));
    check(s.read(|v, _| v.settings().appearance.mode == Mode::Light && v.settings().reading.look.mode == Mode::Sepia), "with sync off again, writing is Light and reading Sepia");

    // Paged mode.
    let before = position(&mut s).unwrap_or_default();
    s.click(("pages", 1usize));
    s.frame();
    check(s.read(|v, _| v.settings().reading.pages == PageMode::Paged), "Paged in the panel lays the book out in pages");
    let after = position(&mut s).unwrap_or_default();
    check(after.chapter == before.chapter && after.fraction <= before.fraction + 1e-3 && before.fraction - after.fraction < 0.5, "the place survives the switch to pages");
    s.click("appearance-done");
    s.press("end");
    s.press("home");
    let pages = s.update(|v, w, _| v.reading_page(w)).unwrap_or_else(|| fail("the chapter is in pages"));
    check(pages.0 == 0 && pages.1 > 1, &format!("Chapter I is {} pages", pages.1));
    s.shot("books-09-paged");
    let page = s.bounds("page").unwrap_or_else(|| fail("the page is drawn"));
    check(page.bottom() <= px(800. - 40.) && s.bounds("page-forward").is_some(), "the page fits the window above the progress line");
    s.press("right");
    check(s.update(|v, w, _| v.reading_page(w)).map(|p| p.0) == Some(1), "Right turns the page");
    s.click("page-forward");
    check(s.update(|v, w, _| v.reading_page(w)).map(|p| p.0) == Some(2.min(pages.1 - 1)), "a click on the page's right edge turns it");
    s.click("page-back");
    check(s.update(|v, w, _| v.reading_page(w)).map(|p| p.0) == Some(1), "and its left edge turns it back");
    s.shot("books-10-page-turned");
    s.press("end");
    check(s.update(|v, w, _| v.reading_page(w)).map(|p| p.0 + 1 == p.1) == Some(true), "End goes to the chapter's last page");
    s.press("right");
    check(position(&mut s).map(|at| (at.chapter, at.fraction)) == Some((target + 1, 0.)), "turning past the last page opens the next chapter");
    s.press("left");
    let last = s.update(|v, w, _| v.reading_page(w)).unwrap_or_default();
    check(position(&mut s).map(|at| at.chapter) == Some(target) && last.0 + 1 == last.1, "and turning back lands on the last page of the one before");

    s.press("home");
    s.press("right");
    s.press("right");
    let paged = position(&mut s).unwrap_or_default();
    s.click("cog");
    s.click("menu-appearance");
    s.click(("pages", 0usize));
    let scrolled = position(&mut s).unwrap_or_default();
    check(scrolled == paged, &format!("switching to Scroll keeps the place ({:.3})", paged.fraction));
    check(s.bounds("chapter-title").is_some_and(|t| t.bottom() < px(56.)), "and the chapter is scrolled to it");
    s.click(("pages", 1usize));
    check(position(&mut s) == Some(paged), "and back to Paged lands on the same page");
    let count = s.update(|v, w, _| v.reading_page(w)).map(|p| p.1).unwrap_or_default();
    s.slide("text-size", 6. / 17., 14. / 17.);
    s.frame();
    let bigger = position(&mut s).unwrap_or_default();
    let now = s.update(|v, w, _| v.reading_page(w)).unwrap_or_default();
    check(now.1 > count, &format!("a bigger text size lays the chapter out in more pages ({count} → {})", now.1));
    check(bigger.chapter == paged.chapter && bigger.fraction <= paged.fraction + 1e-3 && paged.fraction - bigger.fraction < 1. / now.1 as f32 + 1e-3, "and the page shown still holds the place");
    s.click("appearance-done");
    s.shot("books-11-bigger-text");

    set_mode(&mut s, Mode::Dark);
    check(s.read(|v, _| v.settings().appearance.mode) == Mode::Light, "Dark for reading leaves writing Light");
    s.shot("books-12-reading-dark");
    s.advance(501);
    s.press("escape");
    check(s.read(|v, _| v.library_open()), "Escape goes back to the library");
    check(entries(&mut s)[1].progress() > 0., "where the reader is shows on the book");
    s.shot("books-13-library-dark");
    s.click("cog");
    s.click("menu-writing");
    check(!s.read(|v, _| v.library_open()) && s.page_focused(), "Writing in the cog's menu goes back to the page");
    check(s.read(|v, _| v.appearance().mode) == Mode::Light, "in writing's own look");
    let kept = entries(&mut s)[1].position;
    drop(s);

    let mut s = open(dir, &folder, &picked);
    s.click("show-reading");
    check(s.read(|v, _| v.library_open()), "after a relaunch, Read starts at the library");
    s.click("show-writing");
    s.click("cog");
    check(s.bounds("menu-keep-reading").is_some(), "after a relaunch the cog's menu offers Keep reading");
    s.click("menu-keep-reading");
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    s.frame();
    let back = position(&mut s).unwrap_or_else(|| fail("Keep reading opens the book"));
    check(back == kept, &format!("in the place it was left ({back:?})"));
    let big = s.update(|v, w, _| v.reading_page(w)).unwrap_or_default();
    check(big.0 > 0, "on the same page, in pages, in Dark");
    s.hover("reading-column");
    s.advance(200);
    s.shot("books-14-kept-reading");
    check(s.read(|v, _| v.view()) == View::Reading, "the toggle shows Read while a book is open");
    s.shot("books-16-toggle-reading");
    s.click("show-writing");
    check(s.read(|v, _| v.reading().is_none() && v.view() == View::Writing) && s.text() == DRAFT, "Write from a book goes back to the draft, unchanged");
    s.click("show-reading");
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    s.frame();
    check(position(&mut s) == Some(kept), "Read from the page reopens the last book where it was left");
    s.click("back-to-library");
    s.click("show-writing");
    s.click("show-reading");
    settle_work(&mut s, |s| s.read(|v, _| v.library_open()));
    check(s.read(|v, _| v.library_open() && v.reading().is_none()), "left in the library, Read goes back to the library, not the book");
    check(entries(&mut s)[1].position == kept, "and the book keeps its place");
    s.click(("book", 1usize));
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    s.click("show-writing");
    s.click("show-reading");
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    check(s.read(|v, _| v.reading().is_some()), "left in a book, Read goes back to the book");
    drop(s);

    let mut s = Snap::open_sized(dir, size(px(1000.), px(640.)));
    let host = host(vec![folder.clone()], &picked);
    s.update(|v, _, cx| v.set_book_host(host, cx));
    s.click("cog");
    s.click("menu-keep-reading");
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    s.frame();
    let small = s.update(|v, w, _| v.reading_page(w)).unwrap_or_default();
    let here = position(&mut s).unwrap_or_default();
    check(small.1 > big.1, &format!("a smaller window lays the chapter out in more pages ({} → {})", big.1, small.1));
    check(
        here.chapter == kept.chapter && here.fraction <= kept.fraction + 1e-3 && kept.fraction - here.fraction < 1. / small.1 as f32 + 1e-3,
        "and opens on the page that holds the place",
    );
    s.hover("reading-column");
    s.advance(200);
    s.shot("books-15-smaller-window");
}

//! Headless proof of the Library and Reading screens: the fixture books (`books/fetch-fixtures.sh`)
//! added through Add and Find books, a chapter read in Light and Dark, and the place kept across
//! a relaunch. The host's open panel and folders are stand-ins, so nothing reads the real Desktop.

use super::*;
use inkling_books::Format;
use inkling_core::Mode;
use inkling_ui::BookHost;
use std::cell::RefCell;

const NOT_A_BOOK: &str = "“notes.txt” isn’t an EPUB or a PDF.";

/// What the stand-in open panel answers with next.
type Picked = Rc<RefCell<Vec<PathBuf>>>;

fn host(folders: Vec<PathBuf>, picked: &Picked) -> BookHost {
    let picked = picked.clone();
    BookHost { folders, pick: Box::new(move |_| Task::ready(picked.borrow().clone())) }
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

    s.click(("book", 1usize));
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    let (chapters, target) = s.read(|v, _| {
        let (book, _) = v.reading().unwrap_or_else(|| fail("a click opens the book"));
        let target = book.chapters.iter().position(|c| c.title.as_deref().is_some_and(|t| t.to_lowercase().starts_with("chapter"))).unwrap_or(0);
        (book.chapters.len(), target)
    });
    check(position(&mut s) == Some(Default::default()), &format!("the book opens at its start ({chapters} chapters)"));
    check(target > 0 && target < 15, &format!("chapter {} is Chapter I, after the title pages", target + 1));
    while position(&mut s).is_some_and(|at| at.chapter < target) {
        s.with(|w, cx| w.scroll("reading-body", ScrollDelta::Pixels(point(px(0.), px(-100_000.))), cx));
        s.frame();
        s.click("next-chapter");
    }
    check(s.bounds("chapter-title").is_some_and(|t| t.top() > px(56.)), "the next chapter starts at its top");
    check(position(&mut s).map(|at| at.chapter) == Some(target), "Next turns to the following chapter");
    let column = s.bounds("reading-column").unwrap_or_else(|| fail("the chapter is laid out"));
    check(column.size.width == px(700.), "the chapter is set in the page's 700 pt column");
    check(s.bounds("chapter-title").is_some() && s.bounds("prev-chapter").is_some(), "with its title, and the way back a chapter");
    s.with(|w, cx| w.scroll("reading-body", ScrollDelta::Pixels(point(px(0.), px(-900.))), cx));
    s.frame();
    let read = position(&mut s).unwrap_or_else(|| fail("the book is still open"));
    check(read.chapter == target && read.fraction > 0.05 && read.fraction < 0.95, &format!("scrolling moves through the chapter ({:.2})", read.fraction));
    s.advance(499);
    check(entries(&mut s)[1].position.fraction == 0., "the place isn't saved until 500 ms after the scroll");
    s.advance(2);
    let kept = entries(&mut s)[1].position;
    check(kept == read, "the place is saved 500 ms after the last scroll");
    s.shot("books-05-reading");

    set_mode(&mut s, Mode::Dark);
    check(s.bounds("reading-column").is_some(), "the chapter stays open in Dark");
    s.shot("books-06-reading-dark");
    s.press("escape");
    check(s.read(|v, _| v.library_open()), "Escape goes back to the library");
    check(entries(&mut s)[1].progress() > 0., "where the reader is shows on the book");
    s.shot("books-07-library-dark");
    set_mode(&mut s, Mode::Light);
    s.click("cog");
    s.click("menu-writing");
    check(!s.read(|v, _| v.library_open()) && s.page_focused(), "Writing in the cog's menu goes back to the page");
    drop(s);

    let mut s = open(dir, &folder, &picked);
    s.click("cog");
    s.click("menu-library");
    s.hover("library");
    s.advance(200);
    check(entries(&mut s).len() == 2, "a relaunch keeps both books");
    s.click(("book", 1usize));
    settle_work(&mut s, |s| s.read(|v, _| v.reading().is_some()));
    s.frame();
    let back = position(&mut s).unwrap_or_else(|| fail("the book opens again"));
    check(back.chapter == kept.chapter && (back.fraction - kept.fraction).abs() < 0.01, &format!("and the book opens where it was left ({back:?})"));
    check(s.bounds("chapter-title").is_some_and(|t| t.bottom() < px(0.)), "scrolled down to it, past the chapter's title");
    s.shot("books-08-reopened");
}

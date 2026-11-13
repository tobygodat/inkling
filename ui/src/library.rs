//! The Library screen: the books on this device, newest first, with Add and Find books. The
//! library lives in `<data>/library/`. Parsing, the folder search and loading a book run on the
//! background executor; only `Library`'s small index is written on the UI thread.

use crate::app::{Inkling, Screen, ghost_button, primary_button};
use crate::reading::Reading;
use crate::theme::Colors;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_books::scan::{self, Found};
use inkling_books::{BookId, Entry, Format, Library, Prepared};
use std::collections::HashSet;
use std::path::PathBuf;

/// Shows the host's open panel and resolves to the files chosen, or none when the writer cancels.
pub type PickBooks = Box<dyn Fn(&mut App) -> Task<Vec<PathBuf>>>;

/// Gives the folders Find books searches, at the moment it searches.
pub type BookFolders = Box<dyn Fn(&mut App) -> Vec<PathBuf>>;

/// What the host lends the library, the way it lends `Paths` and the key store. The views never
/// read the platform.
pub struct BookHost {
    /// The folders Find books looks through. On the Mac: Desktop, Documents and Downloads. On
    /// iPhone: the app's Documents and the folders the reader allowed, which the host opens (and
    /// keeps open while their books are read) each time it is asked.
    pub folders: BookFolders,
    /// Asks the reader for book files: Add.
    pub pick: PickBooks,
    /// Asks for one more folder to search and remembers it, on hosts that can only search what the
    /// reader allows (iPhone). Resolves to the folder, or none when the reader cancels.
    pub add_folder: Option<PickBooks>,
}

pub(crate) struct Books {
    pub library: Result<Library, String>,
    pub host: Option<BookHost>,
    /// Files still being added.
    pub adding: usize,
    /// Why the last files couldn't be added or opened, in `ImportError`'s words.
    pub errors: Vec<String>,
    /// The book being loaded to open, until it opens or the reader goes elsewhere.
    pub opening: Option<BookId>,
    /// Whether reading was last left in a book rather than the library, so Read goes back there.
    /// False at launch: Read starts at the library.
    pub left_in_book: bool,
}

impl Books {
    pub fn open(dir: PathBuf) -> Self {
        let library = Library::open(dir).map_err(|e| format!("The library couldn't be opened ({e})."));
        Books { library, host: None, adding: 0, errors: vec![], opening: None, left_in_book: false }
    }

    pub fn entries(&self) -> &[Entry] {
        self.library.as_ref().map(Library::entries).unwrap_or_default()
    }
}

/// Find books, while it is open over the library.
pub(crate) enum Finding {
    Searching,
    Found(Vec<Candidate>),
}

pub(crate) struct Candidate {
    pub found: Found,
    pub chosen: bool,
}

const TILE: Pixels = px(150.);

impl Inkling {
    /// The host's file panel and book folders. Without them the library has no Add and no Find
    /// books, which is how the iPhone runs until its host passes its own.
    pub fn set_book_host(&mut self, host: BookHost, cx: &mut Context<Self>) {
        self.books.host = Some(host);
        cx.notify();
    }

    /// The Library row of the cog's menu.
    pub fn open_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.save_position();
        self.books.left_in_book = false;
        self.screen = Screen::Library(None);
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        cx.notify();
    }

    /// The book opened last, for the cog's Keep reading.
    pub fn last_book(&self) -> Option<&Entry> {
        self.books.library.as_ref().ok()?.last_opened()
    }

    /// Keep reading in the cog's menu: back into the book opened last, where it was left.
    pub fn keep_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        let Some(id) = self.last_book().map(|e| e.id.clone()) else { return };
        self.save_position();
        self.open_book(id, cx);
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
    }

    /// The Writing row of the cog's menu, and Escape on the library.
    pub fn back_to_writing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.save_position();
        self.books.opening = None;
        self.screen = Screen::Page;
        self.focus_page(window, cx);
        cx.notify();
    }

    pub fn library_open(&self) -> bool {
        matches!(self.screen, Screen::Library(_))
    }

    /// The books in the library, newest first.
    pub fn library_entries(&self) -> &[Entry] {
        self.books.entries()
    }

    /// What the library says went wrong, in `ImportError`'s words.
    pub fn library_errors(&self) -> &[String] {
        &self.books.errors
    }

    /// How many files are still being added.
    pub fn adding(&self) -> usize {
        self.books.adding
    }

    /// Find books' results, each with whether it is ticked. `None` while it searches or is closed.
    pub fn found_books(&self) -> Option<Vec<(&Found, bool)>> {
        match &self.screen {
            Screen::Library(Some(Finding::Found(found))) => Some(found.iter().map(|c| (&c.found, c.chosen)).collect()),
            _ => None,
        }
    }

    /// Add: the host's open panel, then each file chosen is added.
    pub fn add_books(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &self.books.host else { return };
        let picked = (host.pick)(cx);
        cx.spawn(async move |this, cx| {
            let paths = picked.await;
            let _ = this.update(cx, |this, cx| this.import(paths, cx));
        })
        .detach();
    }

    /// Parses and stores each file on the background executor, then records it in the index.
    pub fn import(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Ok(library) = &self.books.library else { return };
        if paths.is_empty() {
            return;
        }
        self.books.errors.clear();
        self.books.adding += paths.len();
        for path in paths {
            let dir = library.dir().to_path_buf();
            let job = cx.background_spawn(async move { inkling_books::prepare(&dir, &path) });
            cx.spawn(async move |this, cx| {
                let result = job.await;
                let _ = this.update(cx, |this, cx| this.imported(result.map_err(|e| e.to_string()), cx));
            })
            .detach();
        }
        cx.notify();
    }

    fn imported(&mut self, result: Result<Prepared, String>, cx: &mut Context<Self>) {
        self.books.adding = self.books.adding.saturating_sub(1);
        let added = result.and_then(|prepared| match &mut self.books.library {
            Ok(library) => library.add(&prepared).map(|_| ()).map_err(|e| format!("The book couldn't be added ({e}).")),
            Err(e) => Err(e.clone()),
        });
        if let Err(why) = added {
            self.books.errors.push(why);
        }
        cx.notify();
    }

    /// Find books: looks through the host's folders on the background executor for books the
    /// library doesn't have yet.
    pub fn find_books(&mut self, cx: &mut Context<Self>) {
        let Some(host) = &self.books.host else { return };
        let folders = (host.folders)(cx);
        let known: HashSet<BookId> = self.books.entries().iter().map(|e| e.id.clone()).collect();
        self.books.errors.clear();
        self.screen = Screen::Library(Some(Finding::Searching));
        let search = cx.background_spawn(async move { scan::scan(&folders, &known) });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            let _ = this.update(cx, |this, cx| {
                if let Screen::Library(finding @ Some(Finding::Searching)) = &mut this.screen {
                    // PDFs start unticked: many are papers and invoices, not books.
                    let found = found.into_iter().map(|found| Candidate { chosen: found.format == Format::Epub, found }).collect();
                    *finding = Some(Finding::Found(found));
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub fn toggle_found(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Screen::Library(Some(Finding::Found(found))) = &mut self.screen
            && let Some(candidate) = found.get_mut(at)
        {
            candidate.chosen = !candidate.chosen;
            cx.notify();
        }
    }

    /// Adds the ticked books and goes back to the library.
    pub fn add_found(&mut self, cx: &mut Context<Self>) {
        let Screen::Library(Some(Finding::Found(found))) = &self.screen else { return };
        let paths: Vec<PathBuf> = found.iter().filter(|c| c.chosen).map(|c| c.found.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        self.screen = Screen::Library(None);
        self.import(paths, cx);
    }

    /// "Search another folder" on Find books: the host asks for a folder and remembers it, then
    /// Find books searches again with it.
    pub fn add_book_folder(&mut self, cx: &mut Context<Self>) {
        let Some(add) = self.books.host.as_ref().and_then(|h| h.add_folder.as_ref()) else { return };
        let chosen = add(cx);
        cx.spawn(async move |this, cx| {
            let folder = chosen.await;
            let _ = this.update(cx, |this, cx| {
                if !folder.is_empty() && matches!(this.screen, Screen::Library(Some(_))) {
                    this.find_books(cx);
                }
            });
        })
        .detach();
    }

    pub fn close_finding(&mut self, cx: &mut Context<Self>) {
        if let Screen::Library(finding) = &mut self.screen {
            *finding = None;
            cx.notify();
        }
    }

    /// Loads the book on the background executor and opens it where the reader left off.
    pub fn open_book(&mut self, id: BookId, cx: &mut Context<Self>) {
        let Ok(library) = &self.books.library else { return };
        let Some(entry) = library.entry(&id) else { return };
        let (dir, position) = (library.dir().to_path_buf(), entry.position);
        self.books.opening = Some(id.clone());
        let load = cx.background_spawn({
            let id = id.clone();
            async move { inkling_books::load(&dir, &id) }
        });
        cx.spawn(async move |this, cx| {
            let book = load.await;
            let _ = this.update(cx, |this, cx| {
                if this.books.opening.as_ref() != Some(&id) || this.screen.keys().is_some() || this.screen.feedback().is_some() {
                    return;
                }
                this.books.opening = None;
                match book {
                    Ok(book) => {
                        if let Ok(library) = &mut this.books.library {
                            let _ = library.opened(&book.id);
                        }
                        this.books.left_in_book = true;
                        this.screen = Screen::Reading(Reading::new(book, position));
                    }
                    Err(e) => this.books.errors = vec![format!("The book couldn't be opened ({e}).")],
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_library(&self, finding: Option<&Finding>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.metrics();
        let side = if self.form == crate::FormFactor::Phone { m.gutter } else { m.gutter.max(px(56.)) };
        let content = div().w_full().max_w(px(1000.)).px(side).pt(px(56.)).pb_8().flex().flex_col().gap_6();
        let content = match finding {
            Some(finding) => content.child(self.render_finding(finding, cx)),
            None => content.child(self.render_shelf_header(cx)).children(self.render_library_status()).child(self.render_shelf(cx)),
        };
        div().id("library").size_full().flex().flex_col().items_center().overflow_y_scroll().child(content).test_support()
    }

    fn render_title(&self, title: &'static str) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .font_family(self.theme().body.face)
            .text_size(px(30.))
            .line_height(relative(1.2))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title)
    }

    fn render_shelf_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let host = self.books.host.as_ref();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(self.render_title("Library"))
            .when(host.is_some(), |row| {
                row.child(
                    outlined_button(&c, "find-books", "Find books")
                        .h(m.control.max(px(32.)))
                        .on_click(cx.listener(|this, _, _, cx| this.find_books(cx)))
                        .test_support(),
                )
            })
            .when(host.is_some(), |row| {
                row.child(primary_button(&c, "add-book", "Add", m).on_click(cx.listener(|this, _, _, cx| this.add_books(cx))).test_support())
            })
    }

    /// "Adding 2 books…", or why the last ones couldn't be added.
    fn render_library_status(&self) -> Option<impl IntoElement + use<>> {
        let c = self.theme().c;
        let library_failed = self.books.library.as_ref().err().cloned();
        let lines: Vec<String> = match self.books.adding {
            0 => library_failed.into_iter().chain(self.books.errors.iter().cloned()).collect(),
            1 => vec!["Adding 1 book…".into()],
            n => vec![format!("Adding {n} books…")],
        };
        (!lines.is_empty()).then(|| {
            div()
                .id("library-message")
                .flex()
                .flex_col()
                .gap_1()
                .text_size(px(14.))
                .text_color(c.ink)
                .children(lines.into_iter().map(|line| div().child(line)))
                .test_support()
        })
    }

    fn render_shelf(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let c = t.c;
        let entries = self.books.entries();
        if entries.is_empty() {
            let hint = if self.books.host.is_some() { "Add an EPUB or a PDF, or find the books already in your folders." } else { "" };
            return div()
                .id("library-empty")
                .pt(px(120.))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(div().font_family(t.body.face).text_size(px(22.)).text_color(c.muted).child("Nothing to read yet."))
                .child(div().text_size(px(14.)).text_color(c.muted).text_center().child(hint))
                .test_support()
                .into_any_element();
        }
        let covers: Vec<Option<PathBuf>> = match &self.books.library {
            Ok(library) => entries.iter().map(|e| library.cover_path(e)).collect(),
            Err(_) => vec![None; entries.len()],
        };
        div()
            .id("shelf")
            .flex()
            .flex_wrap()
            .gap_x(px(32.))
            .gap_y(px(36.))
            .children(entries.iter().zip(covers).enumerate().map(|(at, (entry, cover))| {
                let id = entry.id.clone();
                self.render_tile(entry, cover, &c)
                    .id(("book", at))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_book(id.clone(), cx)))
                    .test_support()
            }))
            .test_support()
            .into_any_element()
    }

    /// A book: its cover, or its title on a plain tile, with a progress line once it is started.
    fn render_tile(&self, entry: &Entry, cover: Option<PathBuf>, c: &Colors) -> Div {
        let face = self.theme().body.face;
        let progress = entry.progress();
        let pictured = cover.is_some();
        let frame = div().relative().w(TILE).h(TILE * 1.5).rounded(px(5.)).overflow_hidden().border_1().border_color(c.hairline).bg(c.segment);
        let frame = match cover {
            Some(path) => frame.child(img(path).size_full().object_fit(ObjectFit::Cover)),
            None => frame.p(px(14.)).child(
                div().font_family(face).text_size(px(18.)).line_height(relative(1.25)).font_weight(FontWeight::SEMIBOLD).text_color(c.ink).child(entry.title.clone()),
            ),
        };
        let frame = frame.when(progress > 0., |frame| {
            let (track, done) = if pictured { (hsla(0., 0., 0., 0.35), hsla(0., 0., 1., 0.9)) } else { (c.hairline.into(), c.ink.into()) };
            frame.child(
                div()
                    .absolute()
                    .left(px(10.))
                    .right(px(10.))
                    .bottom(px(10.))
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(track)
                    .child(div().h_full().w(relative(progress.max(0.02))).rounded(px(2.)).bg(done)),
            )
        });
        let kind = match entry.format {
            Format::Epub => "EPUB",
            Format::Pdf => "PDF",
        };
        let mut detail = entry.author.clone().unwrap_or_else(|| kind.to_string());
        if progress > 0. {
            detail = format!("{detail} · {}%", (progress * 100.).round().max(1.));
        }
        div()
            .w(TILE)
            .flex()
            .flex_col()
            .gap(px(4.))
            .cursor_pointer()
            .child(frame.mb(px(6.)))
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).truncate().child(entry.title.clone()))
            .child(div().text_size(px(12.)).text_color(c.muted).truncate().child(detail))
    }

    fn render_finding(&self, finding: &Finding, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let header = div().flex().items_center().gap_2().child(self.render_title("Find books")).child(
            outlined_button(&c, "find-cancel", "Cancel")
                .h(m.control.max(px(32.)))
                .on_click(cx.listener(|this, _, _, cx| this.close_finding(cx)))
                .test_support(),
        );
        let note = |words: String| div().id("find-note").text_size(px(14.)).text_color(c.muted).child(words).test_support();
        let column = div().w_full().max_w(px(640.)).flex().flex_col().gap_4().child(header);
        // Where the host can search only folders the reader allows (iPhone), a way to allow one more.
        let another = (self.books.host.as_ref().is_some_and(|h| h.add_folder.is_some()) && matches!(finding, Finding::Found(_))).then(|| {
            div().flex().child(
                outlined_button(&c, "add-folder", "Search another folder")
                    .h(m.control.max(px(32.)))
                    .on_click(cx.listener(|this, _, _, cx| this.add_book_folder(cx)))
                    .test_support(),
            )
        });
        let found = match finding {
            Finding::Searching => return column.child(note("Looking through your folders. Nothing leaves this device.".into())),
            Finding::Found(found) if found.is_empty() => return column.child(note("No new books in your folders.".into())).children(another),
            Finding::Found(found) => found,
        };
        let chosen = found.iter().filter(|c| c.chosen).count();
        let count = match found.len() {
            1 => "Found 1 book you don't have yet.".to_string(),
            n => format!("Found {n} books you don't have yet."),
        };
        let add_label = match chosen {
            0 => "Choose books to add".to_string(),
            1 => "Add 1 book".to_string(),
            n => format!("Add {n} books"),
        };
        let add = div()
            .id("add-found")
            .h(px(48.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .map(|add| if chosen > 0 { add.bg(c.ink).text_color(c.surface).cursor_pointer() } else { add.bg(c.segment).text_color(c.muted) })
            .child(add_label)
            .on_click(cx.listener(|this, _, _, cx| this.add_found(cx)))
            .test_support();
        column
            .child(note(format!("{count} Choose the ones to add. PDFs start unticked, since many aren't books.")))
            .child(div().flex().flex_col().children(found.iter().enumerate().map(|(at, candidate)| {
                found_row(candidate, &c, self.theme().body.face)
                    .min_h(m.control)
                    .id(("found", at))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_found(at, cx)))
                    .test_support()
            })))
            .child(add)
            .children(another)
    }
}

fn found_row(candidate: &Candidate, c: &Colors, face: &'static str) -> Div {
    let f = &candidate.found;
    let kind = match f.format {
        Format::Epub => "EPUB",
        Format::Pdf => "PDF",
    };
    let size = if f.bytes >= 1_000_000 { format!("{:.1} MB", f.bytes as f32 / 1e6) } else { format!("{} KB", (f.bytes / 1000).max(1)) };
    let detail: Vec<&str> = [Some(kind), f.author.as_deref(), (!f.folder.is_empty()).then_some(f.folder.as_str()), Some(size.as_str())].into_iter().flatten().collect();
    let mark = div().flex_none().size(px(20.)).rounded(px(5.)).flex().items_center().justify_center();
    let mark = if candidate.chosen {
        mark.bg(c.ink).text_color(c.surface).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("✓")
    } else {
        mark.border(px(1.5)).border_color(c.muted)
    };
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px_2()
        .py(px(10.))
        .rounded(px(8.))
        .hover(|row| row.bg(c.segment))
        .cursor_pointer()
        .child(mark)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_family(face).text_size(px(17.)).font_weight(FontWeight::MEDIUM).truncate().child(f.title.clone()))
                .child(div().text_size(px(12.5)).text_color(c.muted).truncate().child(detail.join(" · "))),
        )
}

pub(crate) fn outlined_button(c: &Colors, id: &'static str, label: &'static str) -> Stateful<Div> {
    ghost_button(c, id, label).px(px(14.)).border_1().border_color(c.hairline).rounded(px(8.)).text_size(px(13.)).text_color(c.ink)
}

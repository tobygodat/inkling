//! Find: one field in the top bar, after the corner, on the page, in a book, in the library and in
//! the document list. On the page and in a book it finds every match, tints them and steps
//! through them; in the two lists it filters the rows. Matching ignores case
//! (`inkling_core::find`).

use crate::FormFactor;
use crate::app::{Inkling, Screen, ghost_button};
use crate::chapter;
use crate::icons::Icon;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;
use inkling_books::BookId;
use inkling_core::find::find_all;
use std::ops::Range;

gpui_kit::actions!(inkling, [OpenFind]);

/// The screen Find was opened on. It closes when that screen goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Place {
    Page,
    Book(BookId),
    Library,
    Documents,
}

/// A match in a book: its chapter, its block, and the bytes of the block's text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BookHit {
    pub chapter: usize,
    pub block: usize,
    pub range: Range<usize>,
}

/// What Find has found, by where it was opened.
pub(crate) enum Hits {
    /// Ranges of the page's Markdown.
    Page(Vec<Range<usize>>),
    /// `texts` holds each chapter's blocks as the reading view lays them out, read once when Find
    /// opens, since a match is tinted in that text.
    Book { texts: Vec<Vec<SharedString>>, hits: Vec<BookHit> },
    /// The lists filter their rows by the query.
    Rows,
}

impl Hits {
    fn len(&self) -> usize {
        match self {
            Hits::Page(hits) => hits.len(),
            Hits::Book { hits, .. } => hits.len(),
            Hits::Rows => 0,
        }
    }
}

/// Find, while its field is open.
pub(crate) struct Finder {
    pub place: Place,
    pub hits: Hits,
    pub current: usize,
    /// Where Find was opened: the first match at or after it is current while the query changes.
    /// A byte of the page, or a chapter and a block of the book.
    origin: (usize, usize),
}

impl Finder {
    /// The book's matches in `chapter`, by block, each with whether it is the current one.
    pub fn in_chapter(&self, chapter: usize) -> Vec<(usize, Range<usize>, bool)> {
        let Hits::Book { hits, .. } = &self.hits else { return vec![] };
        hits.iter().enumerate().filter(|(_, h)| h.chapter == chapter).map(|(i, h)| (h.block, h.range.clone(), i == self.current)).collect()
    }
}

/// The field. It lives as long as the window, so the last query is there when Find opens again.
pub(crate) fn new_field(window: &mut Window, cx: &mut Context<Inkling>) -> (Entity<InputState>, Subscription) {
    let field = cx.new(|cx| InputState::new(window, cx).placeholder("Find").input_action(TextInputAction::Search));
    let sub = cx.subscribe_in(&field, window, |this, _, event: &InputEvent, _, cx| match event {
        InputEvent::Change => this.found_changed(cx),
        InputEvent::PressEnter { shift, .. } => this.step_find(!*shift, cx),
        _ => {}
    });
    (field, sub)
}

impl Inkling {
    /// Where Find would open on the screen now, if it can: not during a review, nor over Find books.
    fn find_place(&self) -> Option<Place> {
        match &self.screen {
            Screen::Page if self.revision.review().is_none() && !self.revision.is_busy() => Some(Place::Page),
            Screen::Reading(r) => Some(Place::Book(r.book.id.clone())),
            Screen::Library(None) => Some(Place::Library),
            Screen::Documents(_) => Some(Place::Documents),
            _ => None,
        }
    }

    pub fn finding(&self) -> bool {
        self.finder.is_some()
    }

    /// The query, as typed.
    pub fn find_query(&self, cx: &App) -> String {
        self.find_field.read(cx).value().to_string()
    }

    /// The count the field shows: the current match, from 1, and how many there are.
    pub fn find_count(&self) -> Option<(usize, usize)> {
        let f = self.finder.as_ref()?;
        match f.hits {
            Hits::Rows => None,
            _ => Some((if f.hits.len() == 0 { 0 } else { f.current + 1 }, f.hits.len())),
        }
    }

    /// The query a list filters by while Find is open on it.
    pub(crate) fn row_filter(&self, place: Place, cx: &App) -> Option<String> {
        self.finder.as_ref().filter(|f| f.place == place)?;
        let q = self.find_query(cx);
        (!q.is_empty()).then_some(q)
    }

    /// Cmd-F, or the magnifier on iPhone: opens the field with its last query chosen, so typing
    /// replaces it.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(place) = self.find_place() else { return };
        self.close_menu(cx);
        if self.appearance_open() {
            self.close_appearance(cx);
        }
        if self.finder.as_ref().is_none_or(|f| f.place != place) {
            let origin = match &self.screen {
                Screen::Page => (self.page.selected_range(cx).start, 0),
                Screen::Reading(r) => (r.chapter, r.block_on_screen(self.settings.reading.pages)),
                _ => (0, 0),
            };
            let hits = match &self.screen {
                Screen::Page => Hits::Page(vec![]),
                Screen::Reading(r) => Hits::Book {
                    texts: r.book.chapters.iter().map(|c| chapter::lay(&c.text).blocks.into_iter().map(|b| b.text.text).collect()).collect(),
                    hits: vec![],
                },
                _ => Hits::Rows,
            };
            self.finder = Some(Finder { place, hits, current: 0, origin });
            self.found_changed(cx);
        }
        self.find_field.update(cx, |field, cx| {
            field.focus(window, cx);
            field.select_all(window, cx);
        });
        if self.form == FormFactor::Phone {
            window.request_virtual_keyboard();
        }
        cx.notify();
    }

    pub(crate) fn on_open_find(&mut self, _: &OpenFind, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
    }

    /// The query changed: find again, and make the first match at or after where Find opened the
    /// current one. The lists filter instead.
    pub(crate) fn found_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.find_query(cx);
        let Some(f) = &mut self.finder else { return };
        match &mut f.hits {
            Hits::Page(hits) => {
                *hits = find_all(&self.page.value(cx), &query);
                f.current = hits.iter().position(|h| h.start >= f.origin.0).unwrap_or(0);
            }
            Hits::Book { texts, hits } => {
                *hits = texts
                    .iter()
                    .enumerate()
                    .flat_map(|(chapter, blocks)| {
                        let query = &query;
                        blocks.iter().enumerate().flat_map(move |(block, text)| find_all(text, query).into_iter().map(move |range| BookHit { chapter, block, range }))
                    })
                    .collect();
                f.current = hits.iter().position(|h| (h.chapter, h.block) >= f.origin).unwrap_or(0);
            }
            Hits::Rows => {}
        }
        match f.place.clone() {
            Place::Documents => self.filter_documents(cx),
            Place::Library => self.books.chosen = None,
            _ => {}
        }
        self.show_found(cx);
    }

    /// The page changed under Find: its matches follow the text, and the caret stays put.
    pub(crate) fn page_edited_while_finding(&mut self, cx: &mut Context<Self>) {
        let Some(Finder { hits: Hits::Page(hits), current, .. }) = &mut self.finder else { return };
        let query = self.find_field.read(cx).value().to_string();
        *hits = find_all(&self.page.value(cx), &query);
        let at = self.page.selected_range(cx).start;
        *current = hits.iter().position(|h| h.end >= at).unwrap_or(0);
        self.page.set_found(hits.clone(), None, cx);
    }

    /// Return and Shift-Return in the field, and iPhone's Search key: the next match or the one
    /// before, round from the last to the first.
    pub fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.finder else { return };
        let n = f.hits.len();
        if n == 0 {
            return;
        }
        f.current = if forward { (f.current + 1) % n } else { (f.current + n - 1) % n };
        self.show_found(cx);
    }

    /// Puts the current match on screen, in Selection, with the others tinted.
    fn show_found(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.finder else { return };
        match &f.hits {
            Hits::Page(hits) => {
                let current = hits.get(f.current).cloned();
                self.page.set_found(hits.clone(), current, cx);
            }
            Hits::Book { hits, .. } => {
                if let Some(hit) = hits.get(f.current).cloned() {
                    self.reveal_in_book(hit.chapter, hit.block, hit.range.start, cx);
                }
            }
            Hits::Rows => {}
        }
        cx.notify();
    }

    /// Escape or Done: the field goes, leaving the caret on the current match, or the book at it.
    /// The lists show every row again.
    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.finder.take() else { return };
        self.forget_find(f, cx);
        self.focus.focus(window, cx);
        if matches!(self.screen, Screen::Page) {
            self.focus_page(window, cx);
        }
        window.dismiss_virtual_keyboard();
        cx.notify();
    }

    fn forget_find(&mut self, f: Finder, cx: &mut Context<Self>) {
        match f.place {
            Place::Page => self.page.set_found(vec![], None, cx),
            Place::Book(_) => self.save_position(),
            Place::Documents => self.filter_documents(cx),
            Place::Library => {}
        }
    }

    /// Find belongs to the screen it opened on: another screen closes it, and takes the keys and
    /// the keyboard from its field.
    pub(crate) fn find_follows_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let here = self.find_place();
        if self.finder.as_ref().is_some_and(|f| Some(&f.place) != here.as_ref())
            && let Some(f) = self.finder.take()
        {
            self.forget_find(f, cx);
            if self.find_field.read(cx).focus_handle(cx).is_focused(window) {
                self.focus.focus(window, cx);
            }
            // The tap that changed the screen may already have taken the focus, not the keyboard.
            window.dismiss_virtual_keyboard();
        }
    }

    /// The field over the top bar, after the corner on the Mac; on iPhone across the top, with Done.
    pub(crate) fn render_find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.finder.as_ref()?;
        let t = self.theme();
        let c = t.c;
        let m = self.metrics();
        let phone = self.form == FormFactor::Phone;
        let count = self.find_count().filter(|_| !self.find_query(cx).is_empty()).map(|(at, of)| {
            div().id("find-count").flex_none().text_size(px(12.)).text_color(c.muted).child(format!("{at} / {of}")).test_support()
        });
        let field = div()
            .id("find-field")
            .flex_1()
            .min_w_0()
            .h(if phone { px(36.) } else { px(30.) })
            .pl(px(10.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap_2()
            .bg(c.surface)
            .border_1()
            .border_color(c.hairline)
            .rounded(px(8.))
            .text_size(if phone { px(15.) } else { m.ui })
            .child(div().flex_1().min_w_0().child(Input::new(&self.find_field).id("find").appearance(false)))
            .children(count)
            .test_support();
        let bar = div()
            .id("find-bar")
            .absolute()
            .top_0()
            .right_0()
            .h(self.top_bar_height())
            .flex()
            .items_center()
            .gap_2()
            .bg(c.ground)
            // It covers the header, so accessibility leaves out what it covers.
            .occlude()
            .role(Role::Dialog)
            .aria_label("Find")
            .child(field)
            .test_support();
        let bar = match self.form {
            FormFactor::Mac => bar.left(self.corner_width()).pr(m.gutter),
            FormFactor::Phone => bar.left_0().pl(px(8.)).pr(px(4.)).child(
                ghost_button(&c, "find-done", "Done")
                    .h(m.control)
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c.ink)
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx)))
                    .test_support(),
            ),
        };
        Some(bar.into_any_element())
    }

    /// How tall the top bar of the screen on show is, so the field covers it.
    fn top_bar_height(&self) -> Pixels {
        match (&self.screen, self.form) {
            (Screen::Reading(_) | Screen::Library(_), _) | (_, FormFactor::Mac) => px(56.),
            (_, FormFactor::Phone) => self.metrics().control,
        }
    }

    /// iPhone: the magnifier at the top right that opens Find.
    pub(crate) fn render_find_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.form != FormFactor::Phone || self.find_place().is_none() {
            return None;
        }
        let c = self.theme().c;
        let size = self.metrics().control;
        Some(
            div()
                .id("open-find")
                .role(Role::Button)
                .accessibility_id("open-find")
                .flex_none()
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .aria_label("Find")
                .child(Icon::Search.draw(px(20.), c.ink))
                .on_click(cx.listener(|this, _, window, cx| this.open_find(window, cx)))
                .test_support()
                .into_any_element(),
        )
    }
}

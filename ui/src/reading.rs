//! The Reading screen: one chapter at a time, reflowed in reading's own look (`Theme` for
//! `View::Reading`), scrolled or laid out in pages. Moving past either end of a chapter goes on
//! into the next or the one before, the contents list jumps to any chapter, and the place in each
//! book is kept with `Library::set_position` and restored when the book opens again.
//!
//! The place is held as an anchor: how far down the chapter the top of the view is, as a fraction
//! of the chapter's height. Scrolling and turning pages move it; a new layout (a mode switch, a
//! resize, a new text size) puts the view back on it.

use crate::app::{Inkling, Screen, ghost_button};
use crate::library::outlined_button;
use crate::theme::{Body, Colors, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_books::{Block, Book, Position};
use inkling_core::PageMode;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// How long after the last scroll the place is saved, as the page autosaves.
const SAVE_AFTER: Duration = Duration::from_millis(500);
const TOP_BAR: Pixels = px(56.);
const BOTTOM_LINE: Pixels = px(40.);
/// Space above and below the text inside the reading area.
const MARGIN: Pixels = px(24.);
/// How far a wheel has to push past the end of a chapter to go on to the next.
const OVERSCROLL: Pixels = px(240.);
/// How far a sideways swipe goes before it turns the page, and how long its scroll events must
/// pause before another swipe can turn one, so a fling's tail doesn't turn a second page.
const SWIPE: Pixels = px(48.);
const SWIPE_PAUSE: Duration = Duration::from_millis(120);

/// One laid-out child of the chapter column, relative to the column's top. `line` is its line
/// height when a page may break between its lines.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    top: Pixels,
    bottom: Pixels,
    line: Option<Pixels>,
}

/// The chapter as the last frame laid it out.
#[derive(Clone, Debug, Default, PartialEq)]
struct Layout {
    pieces: Vec<Piece>,
    height: Pixels,
}

/// Open while `Inkling::screen` is `Screen::Reading(_)`.
pub(crate) struct Reading {
    pub book: Book,
    pub chapter: usize,
    pub scroll: ScrollHandle,
    /// How far down the chapter the view's top is, from 0 to 1. See the module comment.
    anchor: f32,
    /// The anchor still has to be applied to a layout.
    pending: bool,
    /// Written by the column after each layout.
    layout: Rc<RefCell<Layout>>,
    /// The chapter height the view was last put on, to notice a new layout.
    height: Pixels,
    /// Paged: the page on screen.
    page: usize,
    /// The contents list is open.
    pub contents: bool,
    /// How far a wheel has pushed past the end of the chapter, down (+) or up (−).
    overscroll: Pixels,
    /// Paged: how far a sideways swipe has gone, when the last scroll event came, and whether this
    /// stream of events has already turned its page.
    swipe: Pixels,
    last_scroll: Option<std::time::Instant>,
    turned: bool,
}

impl Reading {
    pub fn new(book: Book, at: Position) -> Self {
        let chapter = at.chapter.min(book.chapters.len().saturating_sub(1));
        let anchor = if chapter == at.chapter { at.fraction.clamp(0., 1.) } else { 0. };
        Reading {
            book,
            chapter,
            scroll: ScrollHandle::new(),
            anchor,
            pending: true,
            layout: Rc::default(),
            height: px(0.),
            page: 0,
            contents: false,
            overscroll: px(0.),
            swipe: px(0.),
            last_scroll: None,
            turned: false,
        }
    }

    /// Where the reader is: the chapter, and how far down it the view starts.
    pub fn position(&self, pages: PageMode) -> Position {
        Position { chapter: self.chapter, fraction: self.anchor_now(pages) }
    }

    fn anchor_now(&self, pages: PageMode) -> f32 {
        if self.pending || pages == PageMode::Paged || self.height <= px(0.) {
            return self.anchor;
        }
        (-self.scroll.offset().y / self.height).clamp(0., 1.)
    }

    fn chapter_title(&self, at: usize) -> String {
        self.book.chapters.get(at).and_then(|c| c.title.as_deref()).map(display_title).unwrap_or_else(|| format!("Chapter {}", at + 1))
    }

    /// The top of each page, in the chapter, for pages `page` tall.
    fn pages(&self, page: Pixels) -> Vec<Pixels> {
        paginate(&self.layout.borrow(), page)
    }
}

/// Where each page starts, for pages at most `page` tall. A page ends at the last line or block
/// that fits whole, so no line is ever cut; a block taller than a page with no lines to break
/// between is cut at the page's foot.
fn paginate(layout: &Layout, page: Pixels) -> Vec<Pixels> {
    let mut tops = vec![px(0.)];
    if page <= px(1.) {
        return tops;
    }
    let mut top = px(0.);
    while top + page < layout.height {
        let limit = top + page;
        let mut next = None;
        for piece in &layout.pieces {
            if piece.top > limit {
                break;
            }
            if piece.top > top {
                next = Some(piece.top);
            }
            if let Some(line) = piece.line.filter(|l| *l > px(0.))
                && piece.bottom > limit
            {
                let fits = ((limit - piece.top) / line).floor();
                if fits >= 1. {
                    let cut = piece.top + line * fits;
                    if cut > top {
                        next = Some(cut);
                    }
                }
            }
        }
        top = next.filter(|n| *n > top).unwrap_or(limit);
        tops.push(top);
    }
    tops
}

fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.5,
        2 => 1.3,
        _ => 1.12,
    }
}

impl Inkling {
    /// The open book and where the reader is in it.
    pub fn reading(&self) -> Option<(&Book, Position)> {
        match &self.screen {
            Screen::Reading(r) => Some((&r.book, r.position(self.settings.reading.pages))),
            _ => None,
        }
    }

    /// Paged: the page on screen and how many the chapter has.
    pub fn reading_page(&self, window: &Window) -> Option<(usize, usize)> {
        let Screen::Reading(r) = &self.screen else { return None };
        (self.settings.reading.pages == PageMode::Paged).then(|| (r.page, r.pages(page_height(window)).len()))
    }

    /// Back to the library, keeping the place.
    pub fn close_book(&mut self, cx: &mut Context<Self>) {
        self.save_position();
        self.books.opening = None;
        self.books.left_in_book = false;
        self.screen = Screen::Library(None);
        cx.notify();
    }

    /// Opens chapter `at` with the view `anchor` of the way down it: 0 for its start, 1 for its end.
    pub fn go_to_chapter(&mut self, at: usize, anchor: f32, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &mut self.screen else { return };
        if at >= r.book.chapters.len() {
            return;
        }
        r.chapter = at;
        r.anchor = anchor.clamp(0., 1.);
        r.pending = true;
        *r.layout.borrow_mut() = Layout::default();
        r.height = px(0.);
        r.page = 0;
        r.contents = false;
        r.overscroll = px(0.);
        r.scroll.set_offset(point(px(0.), px(0.)));
        self.saves += 1;
        self.save_position();
        cx.notify();
    }

    pub fn toggle_contents(&mut self, cx: &mut Context<Self>) {
        if let Screen::Reading(r) = &mut self.screen {
            r.contents = !r.contents;
            cx.notify();
        }
    }

    /// Forward a page: the next page, or a screen further down, or on into the next chapter.
    pub fn page_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.turn(true, window, cx);
    }

    pub fn page_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.turn(false, window, cx);
    }

    fn turn(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pages = self.settings.reading.pages;
        let page_h = page_height(window);
        let Screen::Reading(r) = &mut self.screen else { return };
        if r.pending {
            return;
        }
        let (chapter, last) = (r.chapter, r.book.chapters.len().saturating_sub(1));
        let at_edge = match pages {
            PageMode::Paged => {
                let tops = r.pages(page_h);
                let moved = if forward && r.page + 1 < tops.len() {
                    r.page += 1;
                    true
                } else if !forward && r.page > 0 {
                    r.page -= 1;
                    true
                } else {
                    false
                };
                if moved {
                    r.anchor = (tops[r.page] / r.height.max(px(1.))).clamp(0., 1.);
                }
                !moved
            }
            PageMode::Scroll => {
                let (offset, max) = (-r.scroll.offset().y, r.scroll.max_offset().y);
                let step = (page_h - px(48.)).max(px(48.));
                if forward && offset + px(1.) < max {
                    r.scroll.set_offset(point(px(0.), -(offset + step).min(max)));
                    false
                } else if !forward && offset > px(1.) {
                    r.scroll.set_offset(point(px(0.), -(offset - step).max(px(0.))));
                    false
                } else {
                    true
                }
            }
        };
        if at_edge {
            match forward {
                true if chapter < last => self.go_to_chapter(chapter + 1, 0., cx),
                false if chapter > 0 => self.go_to_chapter(chapter - 1, 1., cx),
                _ => {}
            }
        } else {
            self.after_move(cx);
        }
        cx.notify();
    }

    /// Home and End: the start or the end of the chapter.
    pub fn chapter_edge(&mut self, end: bool, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &mut self.screen else { return };
        r.anchor = if end { 1. } else { 0. };
        r.pending = true;
        self.after_move(cx);
        cx.notify();
    }

    /// Up and down: a few lines in scroll mode, a page in paged mode.
    fn nudge(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.reading.pages == PageMode::Paged {
            return self.turn(down, window, cx);
        }
        let line = self.theme().body.size * self.theme().body.line;
        let Screen::Reading(r) = &mut self.screen else { return };
        let (offset, max) = (-r.scroll.offset().y, r.scroll.max_offset().y);
        let to = if down { (offset + line * 3.).min(max) } else { (offset - line * 3.).max(px(0.)) };
        r.scroll.set_offset(point(px(0.), -to));
        self.after_move(cx);
        cx.notify();
    }

    /// Keys while a book is open. Runs before any binding.
    pub(crate) fn reading_key(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(self.screen, Screen::Reading(_)) || self.typing_hex(window, cx) || key.modifiers.platform || key.modifiers.control {
            return false;
        }
        let shift = key.modifiers.shift;
        match key.key.as_str() {
            "right" | "pagedown" => self.page_forward(window, cx),
            "left" | "pageup" => self.page_back(window, cx),
            "space" if shift => self.page_back(window, cx),
            "space" => self.page_forward(window, cx),
            "down" => self.nudge(true, window, cx),
            "up" => self.nudge(false, window, cx),
            "home" => self.chapter_edge(false, cx),
            "end" => self.chapter_edge(true, cx),
            _ => return false,
        }
        true
    }

    /// Writes the open book's place to the library, if it moved.
    pub(crate) fn save_position(&mut self) {
        let Screen::Reading(r) = &self.screen else { return };
        let (id, at) = (r.book.id.clone(), r.position(self.settings.reading.pages));
        if let Ok(library) = &mut self.books.library
            && let Err(e) = library.set_position(&id, at)
        {
            self.books.errors = vec![format!("Couldn't keep your place ({e}).")];
        }
    }

    /// Saves the place 500 ms after the last move. The timer reads the place when it fires, from
    /// the scroll handle and the anchor it holds, so it still saves after the reader has gone to
    /// another screen.
    fn after_move(&mut self, cx: &mut Context<Self>) {
        let pages = self.settings.reading.pages;
        let Screen::Reading(r) = &self.screen else { return };
        self.saves += 1;
        let save = self.saves;
        let (id, chapter, scroll, height) = (r.book.id.clone(), r.chapter, r.scroll.clone(), r.height);
        let anchor = (pages == PageMode::Paged || r.pending).then_some(r.anchor);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let _ = this.update(cx, |this, _| {
                if this.saves == save
                    && let Ok(library) = &mut this.books.library
                {
                    let fraction = anchor.unwrap_or_else(|| if height > px(0.) { (-scroll.offset().y / height).clamp(0., 1.) } else { 0. });
                    let _ = library.set_position(&id, Position { chapter, fraction });
                }
            });
        })
        .detach();
    }

    /// A wheel on the scrolled chapter. Pushing on past either end goes into the next chapter or
    /// the end of the one before.
    fn wheeled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let line = self.theme().body.size * self.theme().body.line;
        let Screen::Reading(r) = &mut self.screen else { return };
        let dy = event.delta.pixel_delta(line).y;
        let (offset, max) = (-r.scroll.offset().y, r.scroll.max_offset().y);
        let (chapter, last) = (r.chapter, r.book.chapters.len().saturating_sub(1));
        let at_end = offset + px(1.) >= max && dy < px(0.);
        let at_start = offset <= px(1.) && dy > px(0.);
        if at_end || at_start {
            r.overscroll -= dy;
        } else {
            r.overscroll = px(0.);
        }
        let push = r.overscroll;
        if push >= OVERSCROLL && chapter < last {
            self.go_to_chapter(chapter + 1, 0., cx);
        } else if push <= -OVERSCROLL && chapter > 0 {
            self.go_to_chapter(chapter - 1, 1., cx);
        } else {
            self.after_move(cx);
            cx.notify();
        }
    }

    /// Paged: a sideways swipe (a finger on iPhone, two on a trackpad) turns the page, left for
    /// forward. One swipe turns one page, however far it goes: after a turn, the rest of that stream
    /// of scroll events, the fling after the finger lifts included, is passed over until it pauses
    /// for `SWIPE_PAUSE` or a new touch starts.
    fn swiped(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Screen::Reading(r) = &mut self.screen else { return };
        let paused = r.last_scroll.is_none_or(|at| now.duration_since(at) >= SWIPE_PAUSE);
        r.last_scroll = Some(now);
        if paused || event.touch_phase == TouchPhase::Started {
            (r.turned, r.swipe) = (false, px(0.));
        }
        if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            r.swipe = px(0.);
            return;
        }
        let delta = event.delta.pixel_delta(px(20.));
        if r.turned || delta.x.abs() <= delta.y.abs() {
            return;
        }
        r.swipe += delta.x;
        if r.swipe.abs() < SWIPE {
            return;
        }
        let forward = r.swipe < px(0.);
        (r.swipe, r.turned) = (px(0.), true);
        self.turn(forward, window, cx);
    }

    /// Puts the view on the anchor once the chapter is laid out, and again after any new layout.
    pub(crate) fn restore_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pages = self.settings.reading.pages;
        let page_h = page_height(window);
        let Screen::Reading(r) = &mut self.screen else { return };
        let layout = r.layout.borrow().clone();
        if layout.height <= px(0.) {
            return;
        }
        if layout.height != r.height {
            if !r.pending {
                r.anchor = r.anchor_now(pages);
            }
            r.height = layout.height;
            r.pending = true;
        }
        if !r.pending {
            return;
        }
        let target = layout.height * r.anchor;
        match pages {
            PageMode::Paged => {
                let tops = paginate(&layout, page_h);
                r.page = tops.iter().rposition(|t| *t <= target + px(0.5)).unwrap_or(0);
                r.pending = false;
            }
            PageMode::Scroll => {
                // The scrolled column has to be laid out before it can be scrolled: straight after
                // a switch from pages it has no size yet, and scrolling it then would lose the place.
                let (view, max) = (r.scroll.bounds().size.height, r.scroll.max_offset().y);
                if view <= px(0.) || (max <= px(0.) && layout.height > view) {
                    cx.on_next_frame(window, |_, _, cx| cx.notify());
                    return;
                }
                r.scroll.set_offset(point(px(0.), -target.min(max)));
                r.pending = false;
            }
        }
        cx.notify();
    }

    /// After the page mode changes, the view goes back on the same place in the new mode.
    pub(crate) fn pages_changed(&mut self, before: PageMode) {
        let Screen::Reading(r) = &mut self.screen else { return };
        r.anchor = r.anchor_now(before);
        r.pending = true;
        r.scroll.set_offset(point(px(0.), px(0.)));
    }

    pub(crate) fn render_reading(&self, r: &Reading, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = self.theme();
        let c = t.c;
        let m = self.metrics();
        let pages = self.settings.reading.pages;
        let page_h = page_height(window);
        let chapters = r.book.chapters.len();
        let at = r.position(pages);
        let percent = if chapters == 0 { 0. } else { ((at.chapter as f32 + at.fraction) / chapters as f32 * 100.).round() };
        let top = div()
            .w_full()
            .h(TOP_BAR)
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .pl(self.corner_width())
            .pr(m.gutter)
            .text_size(m.ui)
            .child(ghost_button(&c, "back-to-library", "‹ Library").on_click(cx.listener(|this, _, _, cx| this.close_book(cx))).test_support())
            .child(div().flex_1().min_w_0().text_color(c.muted).truncate().child(r.book.title.clone()))
            .child(
                outlined_button(&c, "contents", "Contents")
                    .when(r.contents, |b| b.bg(c.segment))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_contents(cx)))
                    .test_support(),
            );
        let page_note = match pages {
            PageMode::Paged => {
                let count = r.pages(page_h).len();
                format!(" · page {} of {count}", r.page.min(count.saturating_sub(1)) + 1)
            }
            PageMode::Scroll => String::new(),
        };
        let bottom = div()
            .id("reading-progress")
            .w_full()
            .h(BOTTOM_LINE)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .px(m.gutter)
            .text_size(px(12.))
            .text_color(c.muted)
            .child(div().truncate().child(format!("{} · {percent}%{page_note}", r.chapter_title(r.chapter))))
            .test_support();
        let column = self.render_chapter(r, &t, cx);
        let body = match pages {
            PageMode::Scroll => div()
                .id("reading-body")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&r.scroll)
                .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.wheeled(event, cx)))
                .child(div().w_full().flex().justify_center().px(m.gutter).pt(MARGIN).pb(px(64.)).child(column))
                .test_support()
                .into_any_element(),
            PageMode::Paged => {
                let tops = r.pages(page_h);
                let page = r.page.min(tops.len() - 1);
                let start = tops[page];
                let end = tops.get(page + 1).copied().unwrap_or_else(|| r.layout.borrow().height.max(start + page_h));
                let zone = |id: &'static str| div().id(id).absolute().top_0().bottom_0().w(relative(0.3)).cursor_pointer();
                div()
                    .id("reading-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .px(m.gutter)
                    .pt(MARGIN)
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| this.swiped(event, window, cx)))
                    .child(
                    div()
                        .id("page")
                        .relative()
                        .w_full()
                        .max_w(t.body.column)
                        .h(page_h)
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .h((end - start).min(page_h))
                                .overflow_hidden()
                                .child(div().absolute().left_0().right_0().top(-start).child(column)),
                        )
                        .child(zone("page-back").left_0().on_click(cx.listener(|this, _, window, cx| this.page_back(window, cx))).test_support())
                        .child(zone("page-forward").right_0().on_click(cx.listener(|this, _, window, cx| this.page_forward(window, cx))).test_support())
                        .test_support(),
                )
                .into_any_element()
            }
        };
        div()
            .id("reading")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(top)
            .child(body)
            .child(bottom)
            .when(r.contents, |screen| screen.child(self.render_contents(r, &c, window.viewport_size().width, cx)))
    }

    /// The contents list, under the Contents button.
    fn render_contents(&self, r: &Reading, c: &Colors, window_width: Pixels, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .id("contents-list")
            .absolute()
            .top(TOP_BAR)
            .right(self.metrics().gutter)
            .w(px(340.))
            .max_w(window_width - self.metrics().gutter * 2.)
            .max_h(px(480.))
            .overflow_y_scroll()
            .p(px(4.))
            .bg(c.surface)
            .border_1()
            .border_color(c.hairline)
            .rounded(px(10.))
            .shadow_md()
            .occlude()
            .text_size(px(13.))
            .children((0..r.book.chapters.len()).map(|at| {
                let on = at == r.chapter;
                div()
                    .id(("chapter", at))
                    .min_h(self.metrics().control.min(px(44.)))
                    .items_center()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(6.))
                    .flex()
                    .gap_2()
                    .when(on, |row| row.bg(c.segment).font_weight(FontWeight::SEMIBOLD))
                    .hover(|row| row.bg(c.segment))
                    .cursor_pointer()
                    .child(div().flex_1().min_w_0().truncate().child(r.chapter_title(at)))
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_chapter(at, 0., cx)))
                    .test_support()
            }))
            .test_support()
    }

    fn render_chapter(&self, r: &Reading, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (c, body): (Colors, Body) = (t.c, t.body);
        let size = body.size;
        let line = size * body.line;
        let gap = size * 0.8;
        let chapter = &r.book.chapters[r.chapter];
        let mut lines: Vec<Option<Pixels>> = vec![];
        let mut children: Vec<AnyElement> = vec![];
        if let Some(prev) = r.chapter.checked_sub(1) {
            lines.push(None);
            children.push(
                div()
                    .id("prev-chapter")
                    .text_size(px(13.))
                    .font_family(crate::SANS)
                    .text_color(c.muted)
                    .cursor_pointer()
                    .child(format!("‹ {}", r.chapter_title(prev)))
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_chapter(prev, 1., cx)))
                    .test_support()
                    .into_any_element(),
            );
        }
        if let Some(title) = chapter.title.as_deref() {
            lines.push(Some(size * 1.7 * 1.2));
            children.push(
                div()
                    .id("chapter-title")
                    .text_size(size * 1.7)
                    .line_height(relative(1.2))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(display_title(title))
                    .test_support()
                    .into_any_element(),
            );
        }
        for block in &chapter.blocks {
            let (element, step) = match block {
                Block::Paragraph(text) => (div().child(text.clone()), Some(line)),
                Block::Heading { level, text } => {
                    let size = size * heading_scale(*level);
                    (div().text_size(size).line_height(relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(text.clone()), Some(size * 1.3))
                }
                Block::Quote(text) => (div().pl(px(18.)).border_l_2().border_color(c.hairline).child(text.clone()), Some(line)),
                Block::Break => (div().w_full().flex().justify_center().text_color(c.muted).child("*   *   *"), None),
            };
            lines.push(step);
            children.push(element.into_any_element());
        }
        lines.push(None);
        children.push(match r.chapter + 1 < r.book.chapters.len() {
            true => {
                let next = r.chapter + 1;
                div()
                    .id("next-chapter")
                    .mt(gap)
                    .pt(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(c.hairline)
                    .cursor_pointer()
                    .child(div().font_family(crate::SANS).text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(c.muted).child("Next"))
                    .child(div().text_size(px(20.)).line_height(relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(r.chapter_title(next)))
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_chapter(next, 0., cx)))
                    .test_support()
                    .into_any_element()
            }
            false => div().id("the-end").mt(gap).w_full().flex().justify_center().italic().text_color(c.muted).child("The end").test_support().into_any_element(),
        });
        let layout = r.layout.clone();
        let view = cx.entity_id();
        div()
            .on_children_prepainted(move |bounds, window, cx| {
                let Some(first) = bounds.first().map(|b| b.top()) else { return };
                let pieces: Vec<Piece> =
                    bounds.iter().zip(&lines).map(|(b, line)| Piece { top: b.top() - first, bottom: b.bottom() - first, line: *line }).collect();
                let height = pieces.last().map(|p| p.bottom).unwrap_or_default();
                let next = Layout { pieces, height };
                if *layout.borrow() != next {
                    *layout.borrow_mut() = next;
                    // Draw again with the new layout, once this frame is done: a redraw asked for
                    // while drawing is dropped on iPhone.
                    window.refresh();
                    cx.defer(move |cx| cx.notify(view));
                }
            })
            .id("reading-column")
            .w_full()
            .max_w(body.column)
            .flex()
            .flex_col()
            .gap(gap)
            .font_family(body.face)
            .text_size(size)
            .line_height(relative(body.line))
            .text_color(c.ink)
            .children(children)
            .test_support()
            .into_any_element()
    }
}

/// How tall a page is in this window: the reading area between the bars, less its margins.
fn page_height(window: &Window) -> Pixels {
    (window.viewport_size().height - TOP_BAR - BOTTOM_LINE - MARGIN * 2.).max(px(120.))
}

/// A chapter title as a reader would set it: "CHAPTER I." becomes "Chapter I", and roman numerals
/// stay in capitals.
pub(crate) fn display_title(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('.').trim();
    let letters: Vec<char> = t.chars().filter(|c| c.is_alphabetic()).collect();
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    let shouting = letters.len() > 3 && upper * 4 >= letters.len() * 3;
    if !shouting {
        return t.to_string();
    }
    t.split(' ')
        .map(|w| {
            let roman = !w.is_empty() && w.chars().all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M' | '.' | ','));
            if roman || w.chars().any(char::is_lowercase) {
                return w.to_string();
            }
            let mut cs = w.chars();
            match cs.next() {
                Some(first) => first.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

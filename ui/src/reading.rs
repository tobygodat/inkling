//! The Reading screen: one chapter at a time, reflowed in the writer's own look (`Theme`), in a
//! column that scrolls. The place in each book is kept with `Library::set_position`, as a chapter
//! and how far through it, and restored when the book opens again.

use crate::app::{Inkling, Screen, ghost_button};
use crate::theme::Theme;
use gpui_kit::*;
use inkling_books::{Block, Book, Position};
use std::time::Duration;

/// How long after the last scroll the place is saved, as the page autosaves.
const SAVE_AFTER: Duration = Duration::from_millis(500);

/// Open while `Inkling::screen` is `Screen::Reading(_)`.
pub(crate) struct Reading {
    pub book: Book,
    pub chapter: usize,
    pub scroll: ScrollHandle,
    /// How far through the chapter to scroll once it is laid out, when the book opens.
    restore: Option<f32>,
}

impl Reading {
    pub fn new(book: Book, at: Position) -> Self {
        let chapter = at.chapter.min(book.chapters.len().saturating_sub(1));
        let fraction = if chapter == at.chapter { at.fraction.clamp(0., 1.) } else { 0. };
        Reading { book, chapter, scroll: ScrollHandle::new(), restore: (fraction > 0.).then_some(fraction) }
    }

    /// Where the reader is: the chapter, and how far its column has scrolled.
    pub fn position(&self) -> Position {
        Position { chapter: self.chapter, fraction: self.restore.unwrap_or_else(|| fraction(&self.scroll)) }
    }

    fn chapter_title(&self, at: usize) -> String {
        self.book.chapters.get(at).and_then(|c| c.title.as_deref()).map(display_title).unwrap_or_else(|| format!("Chapter {}", at + 1))
    }
}

fn fraction(scroll: &ScrollHandle) -> f32 {
    let max = scroll.max_offset().y;
    if max <= px(0.) { 0. } else { (-scroll.offset().y / max).clamp(0., 1.) }
}

impl Inkling {
    /// The open book and where the reader is in it.
    pub fn reading(&self) -> Option<(&Book, Position)> {
        match &self.screen {
            Screen::Reading(r) => Some((&r.book, r.position())),
            _ => None,
        }
    }

    /// Back to the library, keeping the place.
    pub fn close_book(&mut self, cx: &mut Context<Self>) {
        self.save_position();
        self.screen = Screen::Library(None);
        cx.notify();
    }

    pub fn go_to_chapter(&mut self, at: usize, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &mut self.screen else { return };
        if at >= r.book.chapters.len() {
            return;
        }
        r.chapter = at;
        r.restore = None;
        r.scroll.set_offset(point(px(0.), px(0.)));
        self.saves += 1;
        self.save_position();
        cx.notify();
    }

    /// Writes the open book's place to the library, if it moved.
    pub(crate) fn save_position(&mut self) {
        let Screen::Reading(r) = &self.screen else { return };
        let (id, at) = (r.book.id.clone(), r.position());
        if let Ok(library) = &mut self.books.library
            && let Err(e) = library.set_position(&id, at)
        {
            self.books.errors = vec![format!("Couldn't keep your place ({e}).")];
        }
    }

    /// Saves the place 500 ms after the last scroll. The timer holds the scroll handle, so it
    /// still saves when the writer has already gone to another screen.
    fn scrolled(&mut self, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &self.screen else { return };
        self.saves += 1;
        let (save, id, chapter, scroll) = (self.saves, r.book.id.clone(), r.chapter, r.scroll.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let _ = this.update(cx, |this, _| {
                if this.saves == save
                    && let Ok(library) = &mut this.books.library
                {
                    let _ = library.set_position(&id, Position { chapter, fraction: fraction(&scroll) });
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Scrolls a book that just opened to its place, once its chapter has been laid out.
    pub(crate) fn restore_place(&mut self, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &mut self.screen else { return };
        let Some(at) = r.restore else { return };
        let max = r.scroll.max_offset().y;
        if max > px(0.) {
            r.scroll.set_offset(point(px(0.), -max * at));
            r.restore = None;
            cx.notify();
        }
    }

    pub(crate) fn render_reading(&self, r: &Reading, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = self.theme();
        let c = t.c;
        let m = self.metrics();
        let chapters = r.book.chapters.len();
        let at = r.position();
        let percent = if chapters == 0 { 0. } else { ((at.chapter as f32 + at.fraction) / chapters as f32 * 100.).round() };
        let top = div()
            .w_full()
            .h(px(56.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .pl(px(56.))
            .pr(m.gutter)
            .text_size(m.ui)
            .child(ghost_button(&c, "back-to-library", "‹ Library").on_click(cx.listener(|this, _, _, cx| this.close_book(cx))).test_support())
            .child(div().flex_1().min_w_0().text_color(c.muted).truncate().child(r.book.title.clone()));
        let bottom = div()
            .id("reading-progress")
            .w_full()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .text_color(c.muted)
            .child(format!("Chapter {} of {chapters} · {percent}%", r.chapter + 1))
            .test_support();
        let body = div()
            .id("reading-body")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&r.scroll)
            .on_scroll(cx.listener(|this, _, _, cx| this.scrolled(cx)))
            .child(div().w_full().flex().justify_center().px(m.gutter).pt(px(24.)).pb(px(64.)).child(self.render_chapter(r, &t, cx)))
            .test_support();
        div().id("reading").size_full().flex().flex_col().child(top).child(body).child(bottom)
    }

    fn render_chapter(&self, r: &Reading, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (c, body) = (t.c, t.body);
        let size = body.size;
        let gap = size * 0.8;
        let chapter = &r.book.chapters[r.chapter];
        let link = |id: &'static str, label: String| {
            div().id(id).text_size(px(13.)).font_family(crate::SANS).text_color(c.muted).cursor_pointer().child(label)
        };
        let prev = r.chapter.checked_sub(1).map(|prev| {
            link("prev-chapter", format!("‹ {}", r.chapter_title(prev)))
                .mb(gap)
                .on_click(cx.listener(move |this, _, _, cx| this.go_to_chapter(prev, cx)))
                .test_support()
        });
        let title = chapter.title.as_deref().map(|title| {
            div()
                .id("chapter-title")
                .text_size(size * 1.7)
                .line_height(relative(1.2))
                .font_weight(FontWeight::SEMIBOLD)
                .mb(gap)
                .child(display_title(title))
                .test_support()
        });
        let blocks = chapter.blocks.iter().map(|block| match block {
            Block::Paragraph(text) => div().child(text.clone()),
            Block::Heading { level, text } => div()
                .mt(gap * 0.5)
                .text_size(size * heading_scale(*level))
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text.clone()),
            Block::Quote(text) => div().pl(px(18.)).border_l_2().border_color(c.hairline).child(text.clone()),
            Block::Break => div().w_full().flex().justify_center().text_color(c.muted).child("*   *   *"),
        });
        let end = match r.chapter + 1 < r.book.chapters.len() {
            true => {
                let next = r.chapter + 1;
                div()
                    .id("next-chapter")
                    .mt(gap * 2.)
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
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_chapter(next, cx)))
                    .test_support()
            }
            false => div().id("the-end").mt(gap * 2.).w_full().flex().justify_center().italic().text_color(c.muted).child("The end").test_support(),
        };
        div()
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
            .children(prev)
            .children(title)
            .children(blocks)
            .child(end)
            .test_support()
    }
}

fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.5,
        2 => 1.3,
        _ => 1.12,
    }
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

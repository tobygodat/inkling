//! A passage of a book: selecting it, highlighting it and quoting it into the open document.
//!
//! A selection is a range of the chapter's `Laid::text` (see `chapter`). On the Mac a drag selects
//! and a double click selects a word; on iPhone pressing and holding a paragraph selects it. While
//! there is a selection a pill offers Highlight and Quote: beside the selection on the Mac, at the
//! bottom of the screen on iPhone.

use crate::FormFactor;
use crate::app::{Inkling, Screen};
use crate::chapter::Mark;
use crate::icons::Icon;
use crate::reading::Reading;
use crate::theme::Colors;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_books::Highlight;
use inkling_books::highlights::{Run, quote};
use std::ops::Range;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a finger rests on a paragraph before it is selected.
const HOLD: Duration = Duration::from_millis(500);
/// How far a finger may move and still be holding still.
const STILL: Pixels = px(10.);
/// How far the mouse moves before a press becomes a drag.
const DRAG: Pixels = px(3.);

/// A press on the book's text that may become a drag (Mac) or a hold (iPhone).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Press {
    /// Where it landed, in `Laid::text`.
    pub at: usize,
    pub point: Point<Pixels>,
    /// The block it landed on.
    pub block: usize,
    pub dragging: bool,
    /// Tells a hold's timer whether this is still the press it was started for.
    pub token: u64,
}

/// What the contents popover lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Chapters,
    Highlights,
}

impl Reading {
    /// The selection in `Laid::text`, without the spaces at its ends, if it holds any text.
    pub(crate) fn selection(&self) -> Option<Range<usize>> {
        let (a, b) = self.sel?;
        let text = &self.laid.text;
        let range = a.min(b).min(text.len())..a.max(b).min(text.len());
        let words = text.get(range.clone())?;
        let start = range.start + (words.len() - words.trim_start().len());
        let end = range.end - (words.len() - words.trim_end().len());
        (start < end).then_some(start..end)
    }

    /// The highlights of the chapter on screen whose words are found, as ranges of `Laid::text`,
    /// each with its index in `highlights`.
    pub(crate) fn located(&self) -> Vec<(Range<usize>, usize)> {
        self.highlights
            .iter()
            .enumerate()
            .filter(|(_, h)| h.chapter == self.chapter)
            .filter_map(|(i, h)| Some((h.locate(&self.laid.text, &self.laid.map)?, i)))
            .collect()
    }

    /// Whether the selection lies inside a highlight, so the highlighter shows as chosen.
    pub(crate) fn chosen(&self) -> Option<usize> {
        let sel = self.selection()?;
        self.located().into_iter().find(|(r, _)| r.start <= sel.start && sel.end <= r.end).map(|(_, i)| i)
    }

    /// The tints to draw on block `i`: highlights, then the selection over them.
    pub(crate) fn tints(&self, i: usize, c: &Colors) -> Vec<(Range<usize>, Hsla)> {
        let Some(block) = self.laid.blocks.get(i) else { return vec![] };
        let span = block.at..block.at + block.text.text.len();
        let local = |r: &Range<usize>| {
            let (start, end) = (r.start.max(span.start), r.end.min(span.end));
            (start < end).then(|| start - span.start..end - span.start)
        };
        let mut out: Vec<(Range<usize>, Hsla)> = self.located().iter().filter_map(|(r, _)| Some((local(r)?, c.highlight.into()))).collect();
        if let Some(sel) = self.selection().as_ref().and_then(local) {
            out.push((sel, c.selection.into()));
        }
        out
    }

    /// The selection's paragraphs as runs of text with their italics and bold, without footnote
    /// markers.
    pub(crate) fn runs(&self, range: Range<usize>) -> Vec<Vec<Run>> {
        let mut out = vec![];
        for block in &self.laid.blocks {
            let text = &block.text;
            let (start, end) = (range.start.max(block.at), range.end.min(block.at + text.text.len()));
            if start >= end {
                continue;
            }
            let local = start - block.at..end - block.at;
            let mut cuts = vec![local.start, local.end];
            for (r, _) in &text.marks {
                cuts.extend([r.start, r.end].into_iter().filter(|c| local.contains(c)));
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut runs: Vec<Run> = vec![];
            for w in cuts.windows(2) {
                let piece = w[0]..w[1];
                let has = |f: fn(&Mark) -> bool| text.marks.iter().any(|(r, m)| f(m) && r.start <= piece.start && piece.end <= r.end);
                if has(|m| matches!(m, Mark::Note(_))) {
                    continue;
                }
                let (em, strong) = (has(|m| *m == Mark::Em), has(|m| *m == Mark::Strong));
                match runs.last_mut() {
                    Some((t, e, s)) if *e == em && *s == strong => t.push_str(&text.text[piece]),
                    _ => runs.push((text.text[piece].to_string(), em, strong)),
                }
            }
            if !runs.is_empty() {
                out.push(runs);
            }
        }
        out
    }

    /// The place in `Laid::text` under a point in the window: in the block whose lines it is on,
    /// or the nearest block above or below it.
    pub(crate) fn hit(&self, point: Point<Pixels>) -> Option<(usize, usize)> {
        let drawn = self.painted.borrow();
        let off = |layout: &TextLayout| {
            let b = layout.bounds();
            if point.y < b.top() {
                b.top() - point.y
            } else if point.y > b.bottom() {
                point.y - b.bottom()
            } else {
                px(0.)
            }
        };
        let (i, (layout, _)) = drawn.iter().min_by(|(_, (a, _)), (_, (b, _))| off(a).partial_cmp(&off(b)).unwrap_or(std::cmp::Ordering::Equal))?;
        let ix = layout.index_for_position(point).unwrap_or_else(|ix| ix);
        let block = self.laid.blocks.get(*i)?;
        Some((*i, block.at + ix.min(block.text.text.len())))
    }

    /// The word around `at` in `Laid::text`.
    fn word_at(&self, at: usize) -> Range<usize> {
        let text = &self.laid.text;
        let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-';
        let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(i, _)| i);
        let end = text[at..].char_indices().find(|(_, c)| !is_word(*c)).map_or(text.len(), |(i, _)| at + i);
        start..end
    }

    /// Clears the selection and any press. True if there was a selection.
    pub(crate) fn deselect(&mut self) -> bool {
        self.press = None;
        self.sel.take().is_some()
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Inkling {
    fn book_open_mut(&mut self) -> Option<&mut Reading> {
        match &mut self.screen {
            Screen::Reading(r) => Some(r),
            _ => None,
        }
    }

    /// A press on block `block` of the text.
    pub(crate) fn press_text(&mut self, block: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let phone = self.form == FormFactor::Phone;
        let Some(r) = self.book_open_mut() else { return };
        let Some((_, at)) = r.hit(event.position) else { return };
        let token = r.press.map_or(0, |p| p.token) + 1;
        if !phone && event.click_count == 2 {
            r.sel = Some((r.word_at(at).start, r.word_at(at).end));
            r.held = true;
            r.press = None;
            cx.notify();
            return;
        }
        r.held = false;
        r.press = Some(Press { at, point: event.position, block, dragging: false, token });
        if phone {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HOLD).await;
                let _ = this.update(cx, |this, cx| this.held_still(token, cx));
            })
            .detach();
        }
    }

    /// A finger has rested on a paragraph: it is selected, whole.
    fn held_still(&mut self, token: u64, cx: &mut Context<Self>) {
        let Some(r) = self.book_open_mut() else { return };
        let Some(press) = r.press.filter(|p| p.token == token && !p.dragging) else { return };
        let Some(block) = r.laid.blocks.get(press.block) else { return };
        r.sel = Some((block.at, block.at + block.text.text.len()));
        r.held = true;
        r.press = None;
        cx.notify();
    }

    /// The mouse or a finger moving over the book.
    pub(crate) fn move_over_text(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let phone = self.form == FormFactor::Phone;
        let Some(r) = self.book_open_mut() else { return };
        let Some(mut press) = r.press else { return };
        let d = event.position - press.point;
        let beyond = |reach: Pixels| d.x.abs() > reach || d.y.abs() > reach;
        if phone {
            if beyond(STILL) {
                r.press = None;
            }
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            r.press = None;
            return;
        }
        if !press.dragging && !beyond(DRAG) {
            return;
        }
        press.dragging = true;
        r.press = Some(press);
        if let Some((_, to)) = r.hit(event.position) {
            r.sel = Some((press.at, to));
            r.held = true;
        }
        cx.notify();
    }

    pub(crate) fn release_text(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.book_open_mut()
            && r.press.take().is_some_and(|p| p.dragging)
        {
            cx.notify();
        }
    }

    /// A click on the book that ends a drag or a hold, or closes a selection, does nothing else.
    pub(crate) fn swallow_click(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(r) = self.book_open_mut() else { return false };
        if std::mem::take(&mut r.held) {
            return true;
        }
        let had = r.deselect();
        if had {
            cx.notify();
        }
        had
    }

    /// Highlight in the pill: highlights the selection, or removes the highlight it lies in. A
    /// selection that overlaps highlights joins them into one.
    pub fn toggle_highlight(&mut self, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &mut self.screen else { return };
        let Some(sel) = r.selection() else { return };
        if let Some(i) = r.chosen() {
            r.highlights.remove(i);
        } else {
            let overlapping: Vec<(Range<usize>, usize)> = r.located().into_iter().filter(|(g, _)| g.start < sel.end && sel.start < g.end).collect();
            let start = overlapping.iter().map(|(g, _)| g.start).fold(sel.start, usize::min);
            let end = overlapping.iter().map(|(g, _)| g.end).fold(sel.end, usize::max);
            let mut gone: Vec<usize> = overlapping.iter().map(|(_, i)| *i).collect();
            gone.sort_unstable_by(|a, b| b.cmp(a));
            for i in gone {
                r.highlights.remove(i);
            }
            let made = now_millis();
            let words = r.laid.text[start..end].to_string();
            r.highlights.push(Highlight { chapter: r.chapter, start: r.laid.map[start], end: r.laid.map[end], words, made });
            r.highlights.sort_by_key(|h| (h.chapter, h.start, h.made));
            r.sel = Some((start, end));
        }
        let (id, all) = (r.book.id.clone(), r.highlights.clone());
        if let Ok(library) = &self.books.library
            && let Err(e) = library.set_highlights(&id, &all)
        {
            self.books.errors = vec![format!("Couldn't keep your highlights ({e}).")];
        }
        cx.notify();
    }

    /// Quote in the pill: the passage goes into the open document as a block quote, with the
    /// author and title under it, after the line that holds the caret or at the end, as one undo
    /// step. The book stays on screen.
    pub fn quote_selection(&mut self, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &self.screen else { return };
        // The review keeps byte offsets into the document, so nothing else may change it meanwhile.
        if self.revision.review().is_some() || self.revision.is_busy() {
            self.books.errors = vec!["Finish the review on the page before quoting.".into()];
            cx.notify();
            return;
        }
        let Some(sel) = r.selection() else { return };
        let block = quote(&r.runs(sel), r.book.author.as_deref(), &r.book.title);
        let text = self.page.value(cx);
        let (at, insert) = inkling_core::place_block(&text, self.page.placed_caret(cx), &block);
        self.page.replace(at..at, &insert, cx);
        let caret = at + insert.len();
        self.page.set_selection(caret..caret, cx);
        cx.notify();
    }

    /// Cmd-C with a passage selected.
    pub(crate) fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        let Screen::Reading(r) = &self.screen else { return false };
        let Some(sel) = r.selection() else { return false };
        cx.write_to_clipboard(ClipboardItem::new_string(r.laid.text[sel].to_string()));
        true
    }

    /// A highlight chosen in Contents: its chapter, at the block that holds it.
    pub fn go_to_highlight(&mut self, i: usize, cx: &mut Context<Self>) {
        let Screen::Reading(r) = &self.screen else { return };
        let Some(h) = r.highlights.get(i) else { return };
        let (chapter, at) = (h.chapter, h.start);
        self.go_to_place(chapter, at, cx);
    }

    pub(crate) fn show_contents_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if let Some(r) = self.book_open_mut() {
            r.tab = tab;
            cx.notify();
        }
    }

    /// The pill: beside the selection's end on the Mac, at the bottom of the screen on iPhone.
    pub(crate) fn render_pill(&self, r: &Reading, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Not while a drag is still selecting: it would sit under the pointer.
        if r.press.is_some_and(|p| p.dragging) {
            return None;
        }
        let sel = r.selection()?;
        let c = self.theme().c;
        let phone = self.form == FormFactor::Phone;
        let size = if phone { px(44.) } else { px(28.) };
        let chosen = r.chosen().is_some();
        let button = |id: &'static str, icon: Icon, label: &'static str, on: bool| {
            div()
                .id(id)
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |b| b.bg(c.segment))
                .hover(|b| b.bg(c.segment))
                .aria_label(label)
                .aria_selected(on)
                .child(icon.draw(if phone { px(20.) } else { px(15.) }, c.ink))
                .when(!phone, |b| b.tooltip(move |window, cx| Tooltip::new(label).build(window, cx)))
        };
        let pill = div()
            .id("pill")
            .flex()
            .gap(px(2.))
            .p(px(3.))
            .bg(c.surface)
            .border_1()
            .border_color(c.hairline)
            .rounded(if phone { px(14.) } else { px(9.) })
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(button("pill-highlight", Icon::Highlighter, "Highlight", chosen).on_click(cx.listener(|this, _, _, cx| this.toggle_highlight(cx))).test_support())
            .child(button("pill-quote", Icon::Quote, "Quote", false).on_click(cx.listener(|this, _, _, cx| this.quote_selection(cx))).test_support())
            .test_support();
        if phone {
            return Some(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(56.))
                    .flex()
                    .justify_center()
                    .child(pill)
                    .into_any_element(),
            );
        }
        let drawn = r.painted.borrow();
        let end = sel.end;
        let (layout, at) = r.laid.blocks.iter().enumerate().rev().find(|(_, b)| b.at <= end && end <= b.at + b.text.text.len()).and_then(|(i, b)| Some((drawn.get(&i)?.0.clone(), end - b.at)))?;
        let p = layout.position_for_index(at)?;
        let width = size * 2. + px(10.);
        let x = (p.x + px(6.)).min(window.viewport_size().width - width - px(8.));
        let y = p.y + (layout.line_height() - size - px(8.)) / 2.;
        Some(deferred(anchored().position(point(x, y)).child(pill)).with_priority(1).into_any_element())
    }

    /// The Highlights tab of the contents popover, in reading order.
    pub(crate) fn render_highlight_rows(&self, r: &Reading, c: &Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if r.highlights.is_empty() {
            return vec![div().id("no-highlights").px(px(10.)).py(px(10.)).text_color(c.muted).child("No highlights yet").test_support().into_any_element()];
        }
        r.highlights
            .iter()
            .enumerate()
            .map(|(i, h)| {
                div()
                    .id(("highlight", i))
                    .min_h(self.metrics().control.min(px(44.)))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .hover(|row| row.bg(c.segment))
                    .cursor_pointer()
                    .font_family(self.theme().body.face)
                    .line_height(relative(1.4))
                    .line_clamp(3)
                    .child(h.words.replace('\n', " "))
                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_highlight(i, cx)))
                    .test_support()
                    .into_any_element()
            })
            .collect()
    }

    /// The selected passage's words, for the headless proof.
    pub fn reading_selection(&self) -> Option<String> {
        let Screen::Reading(r) = &self.screen else { return None };
        Some(r.laid.text[r.selection()?].to_string())
    }

    /// The open book's highlights and how many of them are drawn in the chapter on screen.
    pub fn reading_highlights(&self) -> Option<(Vec<Highlight>, usize)> {
        let Screen::Reading(r) = &self.screen else { return None };
        Some((r.highlights.clone(), r.located().len()))
    }

    /// Whether the pill shows its highlighter as chosen.
    pub fn highlighter_chosen(&self) -> bool {
        matches!(&self.screen, Screen::Reading(r) if r.chosen().is_some())
    }

    /// The point in the window over byte `offset` of the first `needle` in the chapter on
    /// screen, half a line down: for the headless proof, which drags and clicks there.
    pub fn reading_point(&self, needle: &str, offset: usize) -> Option<Point<Pixels>> {
        let Screen::Reading(r) = &self.screen else { return None };
        let at = r.laid.text.find(needle)? + offset;
        let (i, block) = r.laid.blocks.iter().enumerate().rev().find(|(_, b)| b.at <= at)?;
        let drawn = r.painted.borrow();
        let (layout, _) = drawn.get(&i)?;
        let p = layout.position_for_index(at - block.at)?;
        Some(point(p.x + px(1.), p.y + layout.line_height() / 2.))
    }
}

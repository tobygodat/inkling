//! The page during review: the draft with each change drawn inline. On Mac every change carries
//! its own controls. On iPhone a tap selects a change and a sheet below the page acts on it.

use crate::app::{Inkling, ghost_button};
use crate::inline::{InlineText, ROOM, room_for, width_of};
use crate::theme::{Body, Colors};
use crate::{FormFactor, SANS};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{Change, ChangeId, Decision, Segment};
use std::ops::Range;

const PENDING_CONTROLS: f32 = 70.;
const MORE_BUTTON: f32 = 56.;
/// Where the page's text sits inside the page: left, right and top. The review matches it, so
/// nothing moves when a review opens.
fn text_inset(form: FormFactor, body: &Body) -> (Pixels, Pixels, Pixels) {
    match form {
        FormFactor::Mac => {
            let gutter = form.metrics().gutter;
            (gutter, gutter, body.size)
        }
        FormFactor::Phone => (px(10.), px(20.), px(8.)),
    }
}

fn removed(c: &Colors) -> HighlightStyle {
    HighlightStyle {
        color: Some(c.removed_text.into()),
        background_color: Some(c.removed_fill.into()),
        strikethrough: Some(StrikethroughStyle { thickness: px(1.5), color: Some(c.removed_line.into()) }),
        ..Default::default()
    }
}

fn added(c: &Colors) -> HighlightStyle {
    HighlightStyle {
        color: Some(c.added_text.into()),
        background_color: Some(c.added_fill.into()),
        ..Default::default()
    }
}

fn kept(c: &Colors) -> HighlightStyle {
    HighlightStyle {
        color: Some(c.added_text.into()),
        underline: Some(UnderlineStyle { thickness: px(2.), color: Some(c.added_line.into()), wavy: false }),
        ..Default::default()
    }
}

fn selected(c: &Colors) -> HighlightStyle {
    HighlightStyle {
        underline: Some(UnderlineStyle { thickness: px(2.), color: Some(c.ink.into()), wavy: false }),
        ..Default::default()
    }
}

/// The review as one run of text, with the styles, the controls and where each change sits.
#[derive(Default)]
struct Composed {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    controls: Vec<(usize, AnyElement)>,
    changes: Vec<(Range<usize>, ChangeId)>,
}

impl Composed {
    fn push(&mut self, s: &str, style: Option<HighlightStyle>) -> Range<usize> {
        let range = self.text.len()..self.text.len() + s.len();
        self.text.push_str(s);
        if let Some(style) = style.filter(|_| !s.is_empty()) {
            self.highlights.push((range.clone(), style));
        }
        range
    }

    /// Reserves room for a control of `width` after the text so far and places it there.
    fn control(&mut self, width: Pixels, control: AnyElement, font: &Font, size: Pixels, window: &Window) {
        let at = self.text.len();
        let room: String = std::iter::repeat_n(ROOM, room_for(width, font, size, window)).collect();
        self.text.push_str(&room);
        self.controls.push((at, control));
    }
}

impl Inkling {
    pub(crate) fn render_review(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        let c = &t.c;
        let font = font(t.body.face);
        if self.form == FormFactor::Phone && self.selected.is_none() {
            self.selected = self.revision.review().and_then(|r| r.next_pending(None));
        }
        let Some(review) = self.revision.review() else { return div().into_any_element() };
        let next = review.next_pending(None);
        let mut out = Composed::default();
        for segment in review.segments() {
            let change = match segment {
                Segment::Unchanged(text) => {
                    out.push(text, None);
                    continue;
                }
                Segment::Change(change) => change,
            };
            let start = out.text.len();
            let tapped = self.form == FormFactor::Phone && self.selected == Some(change.id);
            let mark = |style: HighlightStyle| if tapped { HighlightStyle { underline: selected(c).underline, ..style } } else { style };
            match change.decision {
                Decision::Pending => {
                    out.push(change.removed, Some(mark(removed(c))));
                    if !change.removed.is_empty() && !change.added.is_empty() {
                        out.push(" ", Some(mark(HighlightStyle::default())));
                    }
                    out.push(change.added, Some(mark(added(c))));
                }
                Decision::Accepted => {
                    out.push(change.added, Some(mark(kept(c))));
                }
                Decision::Rejected => {
                    out.push(change.removed, Some(mark(HighlightStyle::default())));
                }
            }
            let range = start..out.text.len();
            let ordinal = out.changes.len();
            out.changes.push((range, change.id));
            if self.form == FormFactor::Mac {
                let (width, control) = self.change_controls(&change, ordinal, next == Some(change.id), window, cx);
                out.control(width, control, &font, t.body.size, window);
            }
        }

        let styled = StyledText::new(out.text).with_highlights(out.highlights);
        let layout = styled.layout().clone();
        let changes = out.changes;
        let (left, right, top) = text_inset(self.form, &t.body);
        div()
            .id("review")
            .size_full()
            .overflow_y_scroll()
            .font_family(t.body.face)
            .text_size(t.body.size)
            .line_height(relative(t.body.line))
            .text_color(c.ink)
            .child(div().pl(left).pr(right).pt(top).child(InlineText::new(styled, out.controls)))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if this.form != FormFactor::Phone {
                    return;
                }
                let Ok(ix) = layout.index_for_position(event.position()) else { return };
                if let Some((_, id)) = changes.iter().find(|(range, _)| range.start <= ix && ix < range.end) {
                    this.select(Some(*id), cx);
                }
            }))
            .test_support()
            .into_any_element()
    }

    /// The controls after one change on Mac, and the room they take in the line. The accept
    /// button is filled on the change Tab would accept.
    fn change_controls(
        &self,
        change: &Change<'_>,
        key: usize,
        next: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Pixels, AnyElement) {
        let id = change.id;
        let c = self.theme().c;
        match change.decision {
            Decision::Pending => {
                let width = PENDING_CONTROLS + if change.more > 0 { MORE_BUTTON + 2. } else { 0. };
                let mut cluster = div()
                    .flex()
                    .gap(px(2.))
                    .p(px(2.))
                    .bg(c.surface)
                    .border_1()
                    .border_color(c.hairline)
                    .rounded(px(8.))
                    .child(
                        icon_button(("accept", key), "✓", c.added_text, next.then_some(c.added_fill), self.metrics().control)
                            .on_click(cx.listener(move |this, _, window, cx| this.accept(id, window, cx)))
                            .test_support(),
                    )
                    .child(
                        icon_button(("reject", key), "✕", c.removed_text, None, self.metrics().control)
                            .on_click(cx.listener(move |this, _, window, cx| this.reject(id, window, cx)))
                            .test_support(),
                    );
                if change.more > 0 {
                    cluster = cluster.child(
                        div()
                            .id(("more", key))
                            .w(px(MORE_BUTTON))
                            .h(self.metrics().control)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .font_family(SANS)
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c.muted)
                            .cursor_pointer()
                            .child(format!("{} more", change.more))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_wordings(id, cx)))
                            .test_support(),
                    );
                }
                let mut root = div().relative().pl(px(6.)).child(cluster);
                if self.wordings_open == Some(id) {
                    root = root.child(deferred(self.wordings_menu(change, cx)).with_priority(1));
                }
                (px(width), root.into_any_element())
            }
            decision => {
                let (mark, ink) = match decision {
                    Decision::Accepted => ("✓ kept", c.added_count),
                    _ => ("✕ rejected", c.removed_text),
                };
                let ui = Font { weight: FontWeight::MEDIUM, ..font(SANS) };
                let width = 4. + 6. + 8. + f32::from(width_of(mark, &ui, px(11.), window) + width_of("Undo", &ui, px(11.), window));
                let control = div()
                    .pl(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .font_family(SANS)
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .whitespace_nowrap()
                    .child(div().text_color(ink).child(mark))
                    .child(
                        div()
                            .id(("undo", key))
                            .px(px(4.))
                            .rounded(px(4.))
                            .text_color(c.muted)
                            .underline()
                            .cursor_pointer()
                            .child("Undo")
                            .on_click(cx.listener(move |this, _, window, cx| this.undo(id, window, cx)))
                            .test_support(),
                    );
                (px(width), control.into_any_element())
            }
        }
    }

    /// Every wording of a change, the chosen one ticked. A menu under the change on Mac, a
    /// list inside the sheet on iPhone.
    fn wordings_menu(&self, change: &Change<'_>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = change.id;
        let phone = self.form == FormFactor::Phone;
        let c = self.theme().c;
        div()
            .when(!phone, |menu| menu.absolute().top(px(36.)).left(px(6.)).min_w(px(180.)).shadow_md())
            .p(px(4.))
            .flex()
            .flex_col()
            .bg(c.surface)
            .border_1()
            .border_color(c.hairline)
            .rounded(px(8.))
            .font_family(SANS)
            .text_size(if phone { px(15.) } else { px(13.) })
            .children(change.wordings.iter().enumerate().map(|(i, wording)| {
                let chosen = i == change.chosen;
                div()
                    .id(("wording", i))
                    .h(if phone { self.metrics().control } else { px(30.) })
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(c.added_fill).text_color(c.added_text))
                    .child(div().w(px(12.)).child(if chosen { "✓" } else { "" }))
                    .child(wording.clone())
                    .on_click(cx.listener(move |this, _, window, cx| this.choose(id, i, window, cx)))
                    .test_support()
            }))
    }

    /// iPhone: the change that was tapped, with Keep and Use.
    pub(crate) fn render_sheet(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.form != FormFactor::Phone {
            return None;
        }
        let review = self.revision.review()?;
        let change = review.change(self.selected?)?;
        let m = self.metrics();
        let c = self.theme().c;
        let total = review.stats().total;
        let index = review.changes().position(|c| c.id == change.id).unwrap_or(0);
        let id = change.id;
        let step = move |delta: isize| {
            move |this: &mut Inkling, _: &ClickEvent, _: &mut Window, cx: &mut Context<Inkling>| {
                let Some(review) = this.revision.review() else { return };
                let ids: Vec<ChangeId> = review.changes().map(|c| c.id).collect();
                let at = ids.iter().position(|c| *c == id).unwrap_or(0) as isize;
                let next = (at + delta).rem_euclid(ids.len() as isize) as usize;
                this.select(Some(ids[next]), cx);
            }
        };
        let quote = |s: &str| format!("\u{201c}{}\u{201d}", s.trim());
        let (removed, added) = (change.removed.trim(), change.added.trim());
        let keep = if removed.is_empty() { "✕ Leave it out".to_string() } else { format!("✕ Keep {}", quote(removed)) };
        let take = if added.is_empty() { format!("✓ Cut {}", quote(removed)) } else { format!("✓ Use {}", quote(added)) };
        let sheet_button = |id: &'static str, label: String, ink: Rgba, fill: Rgba| {
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .h(px(48.))
                .px_3()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .border_1()
                .border_color(c.hairline)
                .bg(fill)
                .text_color(ink)
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().min_w_0().truncate().child(label))
        };
        Some(
            div()
                .w_full()
                .px(m.gutter)
                .pt(px(14.))
                .pb(px(12.))
                .flex()
                .flex_col()
                .gap_3()
                .bg(c.surface)
                .border_t_1()
                .border_color(c.hairline)
                .rounded_t(px(18.))
                .text_size(m.ui)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .pl_2()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.muted)
                                .child(format!("Change {} of {total}", index + 1)),
                        )
                        .child(
                            div()
                                .flex()
                                .child(icon_button("previous", "‹", c.ink, None, m.control).on_click(cx.listener(step(-1))).test_support())
                                .child(icon_button("next", "›", c.ink, None, m.control).on_click(cx.listener(step(1))).test_support()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            sheet_button("keep", keep, c.removed_text, c.surface)
                                .on_click(cx.listener(move |this, _, window, cx| this.reject(id, window, cx)))
                                .test_support(),
                        )
                        .child(
                            sheet_button("use", take, c.added_text, c.added_fill)
                                .on_click(cx.listener(move |this, _, window, cx| this.accept(id, window, cx)))
                                .test_support(),
                        ),
                )
                .when(change.more > 0, |sheet| {
                    let more = if change.more == 1 { "1 more word".to_string() } else { format!("{} more words", change.more) };
                    sheet.child(
                        ghost_button(&c, "more-words", "")
                            .child(more)
                            .h(m.control)
                            .justify_center()
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_wordings(id, cx)))
                            .test_support(),
                    )
                })
                .when(self.wordings_open == Some(id), |sheet| sheet.child(self.wordings_menu(&change, cx)))
                .when(change.decision != Decision::Pending, |sheet| {
                    sheet.child(
                        ghost_button(&c, "undo", "Undo")
                            .h(m.control)
                            .justify_center()
                            .on_click(cx.listener(move |this, _, window, cx| this.undo(id, window, cx)))
                            .test_support(),
                    )
                })
                .into_any_element(),
        )
    }
}

fn icon_button(id: impl Into<ElementId>, glyph: &'static str, ink: Rgba, fill: Option<Rgba>, size: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when_some(fill, |b, fill| b.bg(fill))
        .font_family(SANS)
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ink)
        .cursor_pointer()
        .child(glyph)
}

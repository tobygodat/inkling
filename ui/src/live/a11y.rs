//! What assistive technology reads of the page: one multi-line text input whose value is the
//! source. The source is also split into text runs, because AccessKit reports a selection as a
//! place in a run, and each image the page draws gets a node with its alt text.

use super::*;
use gpui_kit::accesskit::{Node, Rect, TextPosition, TextSelection};

/// AccessKit gives word starts as one byte each, so a run holds at most this many characters.
const RUN_CHARS: usize = 255;

/// Read from the editor at render, while accessibility is active.
pub(super) struct Reading {
    pub source: String,
    pub selection: Selection,
    /// Each local image drawn: its leaf key, the drawn byte of its atom, and its alt text.
    pub images: Vec<(u64, usize, SharedString)>,
    pub frame: Rc<RefCell<Frame>>,
    pub scale: f32,
}

/// One text run: a line of the source, or a part of a long one.
struct Run {
    range: Range<usize>,
    lengths: Vec<u8>,
    word_starts: Vec<u8>,
}

/// The source as runs, each ending at a line break or after `RUN_CHARS` characters. A source
/// that is empty or ends in a line break ends with an empty run, where the caret can be.
fn runs(source: &str) -> Vec<Run> {
    let mut runs = vec![];
    let mut run = Run { range: 0..0, lengths: vec![], word_starts: vec![] };
    let mut prev: Option<char> = None;
    for (at, c) in source.char_indices() {
        let starts_word = !c.is_whitespace() && prev.is_none_or(char::is_whitespace);
        if starts_word && (!run.lengths.is_empty() || prev.is_some()) {
            run.word_starts.push(run.lengths.len() as u8);
        }
        run.lengths.push(c.len_utf8() as u8);
        run.range.end = at + c.len_utf8();
        prev = (c != '\n').then_some(c);
        if c == '\n' || run.lengths.len() == RUN_CHARS {
            let end = run.range.end;
            runs.push(std::mem::replace(&mut run, Run { range: end..end, lengths: vec![], word_starts: vec![] }));
        }
    }
    if !run.lengths.is_empty() || source.is_empty() || source.ends_with('\n') {
        runs.push(run);
    }
    runs
}

/// The run holding source byte `at`, and the character index there.
fn position(runs: &[Run], source: &str, at: usize) -> (usize, usize) {
    let ix = runs.iter().position(|r| r.range.contains(&at)).unwrap_or(runs.len() - 1);
    let run = &runs[ix];
    let at = at.clamp(run.range.start, run.range.end);
    (ix, source[run.range.start..at].chars().count())
}

/// Where the selection's anchor and head are reported: each a text run and a character in it.
fn reported(source: &str, runs: &[Run], selection: Selection) -> [(usize, usize); 2] {
    [position(runs, source, selection.anchor), position(runs, source, selection.head)]
}

impl LiveEditor {
    /// The selection as the page reports it to assistive technology, for the headless proof.
    pub fn reported_selection(&self) -> [(usize, usize); 2] {
        let source = self.draft.source();
        reported(source, &runs(source), self.draft.selection())
    }
}

impl Reading {
    pub(super) fn write(&self, builder: &mut A11ySubtreeBuilder) {
        let runs = runs(&self.source);
        let ids: Vec<_> = (0..runs.len()).map(|ix| builder.synthetic_node_id(("run", ix))).collect();
        for (run, &id) in runs.iter().zip(&ids) {
            let mut node = Node::new(Role::TextRun);
            node.set_value(&self.source[run.range.clone()]);
            node.set_character_lengths(run.lengths.clone());
            node.set_word_starts(run.word_starts.clone());
            builder.push_child(id, node);
        }
        let [anchor, focus] = reported(&self.source, &runs, self.selection).map(|(ix, character_index)| TextPosition { node: ids[ix], character_index });
        let selection = TextSelection { anchor, focus };
        builder.parent_node().set_text_selection(selection);

        let frame = self.frame.borrow();
        for (n, (key, index, alt)) in self.images.iter().enumerate() {
            let Some(g) = frame.geometry.get(key) else { continue };
            let (from, to) = (g.position_for_index(*index), g.position_for_index(index + ATOM.len()));
            let s = f64::from(self.scale);
            let mut node = Node::new(Role::Image);
            node.set_label(alt.as_ref());
            node.set_bounds(Rect {
                x0: f64::from(from.x.as_f32()) * s,
                y0: f64::from(from.y.as_f32()) * s,
                x1: f64::from(to.x.as_f32()) * s,
                y1: f64::from((from.y + g.caret_height()).as_f32()) * s,
            });
            builder.push_child(builder.synthetic_node_id(("image", n)), node);
        }
    }
}

/// Draws `inner` unchanged and adds the reading's nodes under its accessibility node.
pub(super) struct Accessible<E> {
    pub inner: E,
    pub reading: Option<Reading>,
}

impl<E: Element> IntoElement for Accessible<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for Accessible<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn request_layout(&mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner.prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(id, inspector, bounds, layout, prepaint, window, cx)
    }

    fn a11y_role(&self) -> Option<Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut Node) {
        self.inner.write_a11y_info(node)
    }

    fn a11y_synthetic_children(&mut self, prepaint: &mut Self::PrepaintState, builder: &mut A11ySubtreeBuilder) {
        self.inner.a11y_synthetic_children(prepaint, builder);
        if let Some(reading) = &self.reading {
            reading.write(builder);
        }
    }
}

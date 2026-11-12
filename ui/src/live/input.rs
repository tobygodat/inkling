//! Keys, movement and the platform text input (IME) for the live editor. Movement by character,
//! word, line, block and document; cut, copy and paste carry Markdown source.

use super::*;

gpui_kit::actions!(
    live_editor,
    [
        Left, Right, Up, Down, WordLeft, WordRight, LineStart, LineEnd, BlockUp, BlockDown, DocStart, DocEnd,
        SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight, SelectLineStart,
        SelectLineEnd, SelectBlockUp, SelectBlockDown, SelectDocStart, SelectDocEnd, SelectAll,
        Backspace, Delete, DeleteWordLeft, Enter, Copy, Cut, Paste, Undo, Redo
    ]
);

const CONTEXT: &str = "LiveEditor";
/// Stands for a block that draws no text, counting down from here by block index.
const EMPTY_BLOCK: u64 = u64::MAX - 1;

/// Binds the editor's keys. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("alt-up", BlockUp, c),
        KeyBinding::new("alt-down", BlockDown, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("cmd-shift-left", SelectLineStart, c),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
        KeyBinding::new("alt-shift-up", SelectBlockUp, c),
        KeyBinding::new("alt-shift-down", SelectBlockDown, c),
        KeyBinding::new("cmd-shift-up", SelectDocStart, c),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordLeft, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        // The iPhone's edit menu and Paste control send these.
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
    ]);
}

/// Where a movement takes the caret.
#[derive(Clone, Copy)]
enum Motion {
    Char(bool),
    Word(bool),
    Line(bool),
    Vertical(bool),
    Block(bool),
    Doc(bool),
}

pub(super) fn actions(page: Stateful<Div>, cx: &mut Context<LiveEditor>) -> Stateful<Div> {
    macro_rules! motion {
        ($page:expr, $($action:ident => $motion:expr, $extend:expr;)*) => {
            $page$(.on_action(cx.listener(|e, _: &$action, w, cx| e.go($motion, $extend, w, cx))))*
        };
    }
    let page = motion!(page,
        Left => Motion::Char(false), false;
        Right => Motion::Char(true), false;
        Up => Motion::Vertical(false), false;
        Down => Motion::Vertical(true), false;
        WordLeft => Motion::Word(false), false;
        WordRight => Motion::Word(true), false;
        LineStart => Motion::Line(false), false;
        LineEnd => Motion::Line(true), false;
        BlockUp => Motion::Block(false), false;
        BlockDown => Motion::Block(true), false;
        DocStart => Motion::Doc(false), false;
        DocEnd => Motion::Doc(true), false;
        SelectLeft => Motion::Char(false), true;
        SelectRight => Motion::Char(true), true;
        SelectUp => Motion::Vertical(false), true;
        SelectDown => Motion::Vertical(true), true;
        SelectWordLeft => Motion::Word(false), true;
        SelectWordRight => Motion::Word(true), true;
        SelectLineStart => Motion::Line(false), true;
        SelectLineEnd => Motion::Line(true), true;
        SelectBlockUp => Motion::Block(false), true;
        SelectBlockDown => Motion::Block(true), true;
        SelectDocStart => Motion::Doc(false), true;
        SelectDocEnd => Motion::Doc(true), true;
    );
    page.on_action(cx.listener(|e, _: &SelectAll, _, cx| {
        let len = e.draft.source().len();
        e.set_selection(0..len, cx);
    }))
    .on_action(cx.listener(|e, _: &Backspace, _, cx| e.delete(Motion::Char(false), cx)))
    .on_action(cx.listener(|e, _: &Delete, _, cx| e.delete(Motion::Char(true), cx)))
    .on_action(cx.listener(|e, _: &DeleteWordLeft, _, cx| e.delete(Motion::Word(false), cx)))
    .on_action(cx.listener(|e, _: &Enter, _, cx| {
        let sel = e.draft.selection().range();
        e.edit(sel, "\n", cx);
    }))
    .on_action(cx.listener(|e, _: &Copy, _, cx| e.copy(cx)))
    .on_action(cx.listener(|e, _: &Cut, _, cx| {
        e.copy(cx);
        let sel = e.draft.selection().range();
        e.replace(sel, "", cx);
    }))
    .on_action(cx.listener(|e, _: &Paste, _, cx| {
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            let sel = e.draft.selection().range();
            e.replace(sel, &text, cx);
        }
    }))
    .on_action(cx.listener(|e, _: &Undo, _, cx| {
        if e.draft.undo() {
            e.source_changed(cx);
        }
    }))
    .on_action(cx.listener(|e, _: &Redo, _, cx| {
        if e.draft.redo() {
            e.source_changed(cx);
        }
    }))
}

impl LiveEditor {
    fn copy(&self, cx: &mut Context<Self>) {
        let sel = self.draft.selection().range();
        if !sel.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.draft.source()[sel].to_string()));
        }
    }

    fn delete(&mut self, motion: Motion, cx: &mut Context<Self>) {
        let sel = self.draft.selection();
        let range = if sel.is_empty() {
            let to = self.target(motion, sel.head).unwrap_or(sel.head);
            sel.head.min(to)..sel.head.max(to)
        } else {
            sel.range()
        };
        self.edit(range, "", cx);
    }

    fn go(&mut self, motion: Motion, extend: bool, _: &mut Window, cx: &mut Context<Self>) {
        let sel = self.draft.selection();
        let to = match motion {
            Motion::Char(forward) if !extend && !sel.is_empty() => {
                if forward {
                    sel.range().end
                } else {
                    sel.range().start
                }
            }
            _ => match self.target(motion, sel.head) {
                Some(to) => to,
                None => return,
            },
        };
        if !matches!(motion, Motion::Vertical(_)) {
            self.goal_x = None;
        }
        self.draft.seal();
        self.marked = None;
        self.draft.set_selection(if extend { sel.anchor } else { to }, to);
        self.moved(cx);
    }

    fn target(&mut self, motion: Motion, head: usize) -> Option<usize> {
        let d = &self.draft;
        Some(match motion {
            Motion::Char(true) => d.next_grapheme(head),
            Motion::Char(false) => d.prev_grapheme(head),
            Motion::Word(true) => d.word_right(head),
            Motion::Word(false) => d.word_left(head),
            Motion::Doc(forward) => if forward { d.source().len() } else { 0 },
            Motion::Block(forward) => {
                let ix = d.block_at(head)?;
                let blocks = d.blocks();
                let here = &blocks[ix].range;
                if forward {
                    if head < here.end { here.end } else { blocks.get(ix + 1).map_or(d.source().len(), |b| b.range.end) }
                } else if head > here.start {
                    here.start
                } else {
                    ix.checked_sub(1).map_or(0, |p| blocks[p].range.start)
                }
            }
            Motion::Line(end) => {
                let (key, _) = self.locate(head)?;
                let (p, h) = self.caret_at(head)?;
                let frame = self.frame.borrow();
                let g = frame.geometry.get(&key)?;
                let x = if end { g.bounds().right() + px(10_000.) } else { g.bounds().left() - px(10_000.) };
                let index = g.index_for_position(point(x, p.y + h / 2.));
                drop(frame);
                self.to_source(key, index)?
            }
            Motion::Vertical(down) => self.vertical(down, head)?,
        })
    }

    /// The source byte one visual line up or down at the remembered x, across block edges.
    fn vertical(&mut self, down: bool, head: usize) -> Option<usize> {
        let (p, h) = self.caret_at(head)?;
        let x = *self.goal_x.get_or_insert(p.x);
        let (key, _) = self.locate(head)?;
        let frame = self.frame.borrow();
        let line = frame.geometry.get(&key)?.line_height();
        let y = if down { p.y + h / 2. + line } else { p.y + h / 2. - line };
        let candidates = frame.geometry.iter().map(|(k, g)| (*k, g.bounds())).chain(
            frame
                .blocks
                .iter()
                .enumerate()
                .filter(|(i, _)| self.views.get(*i).is_some_and(|v| v.drawn && v.keys.is_empty()) && Some(*i) != self.active)
                .map(|(i, b)| (EMPTY_BLOCK - i as u64, *b)),
        );
        let ahead = |b: &Bounds<Pixels>| if down { b.bottom() > y } else { b.top() < y };
        let (key, bounds) = candidates
            .filter(|(_, b)| ahead(b))
            .min_by(|a, b| distance(a.1, point(x, y)).partial_cmp(&distance(b.1, point(x, y))).unwrap_or(std::cmp::Ordering::Equal))?;
        if let Some(g) = frame.geometry.get(&key) {
            let lh = g.line_height();
            let y = y.clamp(bounds.top() + lh / 2., (bounds.bottom() - lh / 2.).max(bounds.top() + lh / 2.));
            let index = g.index_for_position(point(x, y));
            drop(frame);
            return self.to_source(key, index);
        }
        let block = (EMPTY_BLOCK - key) as usize;
        Some(self.draft.blocks().get(block)?.range.start)
    }
}

impl LiveEditor {
    /// The source range a platform edit replaces: the one it names, else the marked text, else
    /// the selection.
    fn input_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16.map(|r| self.draft.from_utf16(r)).or(self.marked.clone()).unwrap_or(self.draft.selection().range())
    }
}

impl EntityInputHandler for LiveEditor {
    fn text_for_range(&mut self, range: Range<usize>, adjusted: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r = self.draft.from_utf16(range);
        *adjusted = Some(self.draft.to_utf16(r.clone()));
        Some(self.draft.source()[r].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let sel = self.draft.selection();
        Some(UTF16Selection { range: self.draft.to_utf16(sel.range()), reversed: sel.head < sel.anchor })
    }

    /// The platform's own selection UI moved the selection: a handle on iPhone. The end that
    /// moved becomes the caret, so a dragged start handle stays in view.
    fn set_selected_text_range(&mut self, range: Range<usize>, _: &mut Window, cx: &mut Context<Self>) {
        let was = self.draft.selection().range();
        let r = self.draft.from_utf16(range);
        let start_moved = r.end == was.end && r.start != was.start && !r.is_empty();
        self.draft.seal();
        if start_moved {
            self.draft.set_selection(r.end, r.start);
        } else {
            self.draft.set_selection(r.start, r.end);
        }
        self.goal_x = None;
        // A caret is brought into view. A range is not: its moved end is under the finger,
        // and Select All leaves the page where it is.
        if r.is_empty() {
            self.moved(cx);
        } else {
            cx.notify();
        }
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|m| self.draft.to_utf16(m))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.input_range(range);
        self.edit(r, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        new_selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let r = self.input_range(range);
        self.edit(r.clone(), text, cx);
        let marked = r.start..r.start + text.len();
        if let Some(sel) = new_selection {
            let base = self.draft.to_utf16(marked.start..marked.start).start;
            let sel = self.draft.from_utf16(base + sel.start..base + sel.end);
            self.draft.set_selection(sel.start.min(marked.end), sel.end.min(marked.end));
        }
        self.marked = (!text.is_empty()).then_some(marked);
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let r = self.draft.from_utf16(range);
        self.line_rects(r.clone()).into_iter().next().or_else(|| self.caret_bounds(r.start))
    }

    fn selection_bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Vec<Bounds<Pixels>> {
        self.line_rects(self.draft.from_utf16(range))
    }

    fn character_index_for_point(&mut self, at: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        self.pressed(at).map(|s| self.draft.to_utf16(s..s).start)
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.draft.len_utf16())
    }

    fn text_input_configuration(&mut self, _: &mut Window, _: &mut Context<Self>) -> TextInputConfiguration {
        TextInputConfiguration {
            autocorrect: true,
            autocapitalize: Autocapitalize::Sentences,
            suggestions: true,
            input_action: TextInputAction::Enter,
        }
    }
}

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*,
    px, relative, size,
};
use std::ops::Range;

actions!(
    editor,
    [
        Backspace, Delete, Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown,
        SelectAll, Home, End, Copy, Cut, Paste, Undo, Redo, Newline
    ]
);

pub const CONTEXT: &str = "Editor";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-left", Home, c),
        KeyBinding::new("cmd-right", End, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("enter", Newline, c),
    ]);
}

pub enum EditorEvent {
    Edited,
    Submit,
}

#[derive(Clone)]
pub struct Look {
    pub font: Font,
    pub size: Pixels,
    pub line_height: Pixels,
    pub ink: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
}

/// Text drawn into the editor that isn't in the document: ghost text, a proposal, a placeholder.
/// Built by the owner from its own state on every render, never stored here.
#[derive(Clone)]
pub struct Overlay {
    pub at: usize,
    pub text: SharedString,
    pub color: Hsla,
    pub dim: Range<usize>,
}

struct Snapshot {
    text: String,
    selected: Range<usize>,
}

struct Line {
    start: usize,
    y: Pixels,
    wrapped: WrappedLine,
}

/// Maps document offsets to the shaped text, which may hold an insertion or be masked.
#[derive(Clone, Copy, Default)]
struct Map {
    insert: Option<(usize, usize)>,
    masked: bool,
}

const BULLET: char = '•';

impl Map {
    fn to_display(self, text: &str, ix: usize) -> usize {
        if self.masked {
            return text[..ix].chars().count() * BULLET.len_utf8();
        }
        match self.insert {
            Some((at, len)) if ix > at => ix + len,
            _ => ix,
        }
    }

    fn to_doc(self, text: &str, ix: usize) -> usize {
        if self.masked {
            let n = ix / BULLET.len_utf8();
            return text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
        }
        match self.insert {
            Some((at, len)) if ix > at => ix.saturating_sub(len).max(at),
            _ => ix,
        }
    }
}

pub struct Editor {
    pub focus: FocusHandle,
    pub text: String,
    pub selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: bool,
    single_line: bool,
    masked: bool,
    pub placeholder: SharedString,
    pub look: Look,
    lines: Vec<Line>,
    map: Map,
    bounds: Option<Bounds<Pixels>>,
    selecting: bool,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Editor {
    pub fn new(text: String, look: Look, cx: &mut Context<Self>) -> Self {
        let end = text.len();
        Editor {
            focus: cx.focus_handle(),
            text,
            selected: end..end,
            reversed: false,
            marked: None,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
            single_line: false,
            masked: false,
            placeholder: SharedString::default(),
            look,
            lines: Vec::new(),
            map: Map::default(),
            bounds: None,
            selecting: false,
        }
    }

    pub fn single_line(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.single_line = true;
        self.placeholder = placeholder.into();
        self
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn cursor(&self) -> usize {
        if self.reversed { self.selected.start } else { self.selected.end }
    }

    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.selected = text.len()..text.len();
        self.text = text;
        self.undo.clear();
        self.redo.clear();
        cx.notify();
    }

    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.selected = range;
        self.reversed = false;
        self.typing = false;
        cx.notify();
    }

    /// Edits the text through `f`, which returns where the cursor goes. One undo step.
    pub fn edit_with(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut String) -> Option<usize>) {
        let before = Snapshot { text: self.text.clone(), selected: self.selected.clone() };
        if let Some(cursor) = f(&mut self.text) {
            self.undo.push(before);
            self.redo.clear();
            self.typing = false;
            self.selected = cursor..cursor;
            self.marked = None;
            cx.emit(EditorEvent::Edited);
            cx.notify();
        }
    }

    fn replace(&mut self, range: Range<usize>, new: &str, cx: &mut Context<Self>) {
        let typing = range.is_empty() && new.chars().count() == 1 && new != "\n" && new != " ";
        if !(typing && self.typing) {
            self.undo.push(Snapshot { text: self.text.clone(), selected: self.selected.clone() });
            self.redo.clear();
        }
        self.typing = typing;
        self.text.replace_range(range.clone(), new);
        let end = range.start + new.len();
        self.selected = end..end;
        self.reversed = false;
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn move_to(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = ix..ix;
        self.reversed = false;
        self.typing = false;
        cx.notify();
    }

    fn select_to(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.reversed { self.selected.start = ix } else { self.selected.end = ix }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        self.typing = false;
        cx.notify();
    }

    fn prev(&self, ix: usize) -> usize {
        self.text[..ix].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self, ix: usize) -> usize {
        self.text[ix..].chars().next().map_or(ix, |c| ix + c.len_utf8())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let ix = if self.selected.is_empty() { self.prev(self.cursor()) } else { self.selected.start };
        self.move_to(ix, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let ix = if self.selected.is_empty() { self.next(self.cursor()) } else { self.selected.end };
        self.move_to(ix, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next(self.cursor()), cx);
    }

    fn vertical(&self, rows: f32) -> usize {
        let Some(p) = self.position(self.cursor()) else { return self.cursor() };
        let target = point(p.x, p.y + self.look.line_height * (rows + 0.5));
        self.index_at(target)
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical(-1.), cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.vertical(1.), cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(-1.), cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.vertical(1.), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let ix = self.text[..self.cursor()].rfind('\n').map_or(0, |i| i + 1);
        self.move_to(ix, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let ix = self.text[c..].find('\n').map_or(self.text.len(), |i| c + i);
        self.move_to(ix, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select(0..self.text.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected.is_empty() { self.prev(self.cursor())..self.cursor() } else { self.selected.clone() };
        if !range.is_empty() {
            self.typing = false;
            self.replace(range, "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected.is_empty() { self.cursor()..self.next(self.cursor()) } else { self.selected.clone() };
        if !range.is_empty() {
            self.typing = false;
            self.replace(range, "", cx);
        }
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            cx.emit(EditorEvent::Submit);
        } else {
            self.replace(self.selected.clone(), "\n", cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.text[self.selected.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() && !self.masked {
            self.copy(&Copy, window, cx);
            self.typing = false;
            self.replace(self.selected.clone(), "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.single_line { text.replace('\n', " ") } else { text };
            self.typing = false;
            self.replace(self.selected.clone(), &text, cx);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.step(true, cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.step(false, cx);
    }

    fn step(&mut self, back: bool, cx: &mut Context<Self>) {
        let (from, to) = if back { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some(snap) = from.pop() else { return };
        to.push(Snapshot {
            text: std::mem::replace(&mut self.text, snap.text),
            selected: std::mem::replace(&mut self.selected, snap.selected),
        });
        self.typing = false;
        self.marked = None;
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let ix = self.index_at(self.local(e.position));
        if e.modifiers.shift { self.select_to(ix, cx) } else { self.move_to(ix, cx) }
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && e.pressed_button == Some(MouseButton::Left) {
            self.select_to(self.index_at(self.local(e.position)), cx);
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn local(&self, p: Point<Pixels>) -> Point<Pixels> {
        self.bounds.map_or(p, |b| p - b.origin)
    }

    /// Document offset nearest a point in the editor's own coordinates.
    fn index_at(&self, p: Point<Pixels>) -> usize {
        let lh = self.look.line_height;
        let Some(first) = self.lines.first() else { return 0 };
        if p.y < first.y {
            return 0;
        }
        let line = self.lines.iter().rev().find(|l| p.y >= l.y).unwrap_or(first);
        let local = point(p.x.max(px(0.)), (p.y - line.y).min(line.wrapped.size(lh).height - lh * 0.5));
        let ix = match line.wrapped.closest_index_for_position(local, lh) {
            Ok(ix) | Err(ix) => ix,
        };
        self.map.to_doc(&self.text, line.start + ix).min(self.text.len())
    }

    /// Where a document offset is drawn, in the editor's own coordinates.
    fn position(&self, ix: usize) -> Option<Point<Pixels>> {
        let d = self.map.to_display(&self.text, ix);
        let line = self.lines.iter().rev().find(|l| l.start <= d)?;
        let p = line.wrapped.position_for_index(d - line.start, self.look.line_height)?;
        Some(point(p.x, p.y + line.y))
    }

    fn utf16_to(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.text.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn to_utf16(&self, offset: usize) -> usize {
        self.text[..offset].chars().map(char::len_utf16).sum()
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.to_utf16(r.start)..self.to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.utf16_to(r.start)..self.utf16_to(r.end)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, r: Range<usize>, adjusted: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let range = self.range_from_utf16(&r);
        adjusted.replace(self.range_to_utf16(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected), reversed: self.reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, new: &str, _: &mut Window, cx: &mut Context<Self>) {
        let new = if self.single_line { new.replace('\n', " ") } else { new.to_string() };
        let range = r.as_ref().map(|r| self.range_from_utf16(r)).or(self.marked.clone()).unwrap_or(self.selected.clone());
        self.marked = None;
        self.replace(range, &new, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        new: &str,
        new_selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = r.as_ref().map(|r| self.range_from_utf16(r)).or(self.marked.clone()).unwrap_or(self.selected.clone());
        self.typing = false;
        self.replace(range.clone(), new, cx);
        self.marked = (!new.is_empty()).then(|| range.start..range.start + new.len());
        if let Some(sel) = new_selected {
            let sel = self.utf16_to(self.to_utf16(range.start) + sel.start)..self.utf16_to(self.to_utf16(range.start) + sel.end);
            self.selected = sel;
        }
    }

    fn bounds_for_range(&mut self, r: Range<usize>, element: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&r);
        let p = self.position(range.start)?;
        Some(Bounds::new(element.origin + p, size(px(2.), self.look.line_height)))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.to_utf16(self.index_at(self.local(p))))
    }
}

/// The editor as an element tree: key context, focus, actions and mouse, around the text.
pub fn view(editor: &Entity<Editor>, overlay: Option<Overlay>, window: &Window, cx: &App) -> impl IntoElement {
    let e = editor.read(cx);
    div()
        .key_context(CONTEXT)
        .track_focus(&e.focus)
        .w_full()
        .cursor_text()
        .on_action(window.listener_for(editor, Editor::backspace))
        .on_action(window.listener_for(editor, Editor::delete))
        .on_action(window.listener_for(editor, Editor::left))
        .on_action(window.listener_for(editor, Editor::right))
        .on_action(window.listener_for(editor, Editor::up))
        .on_action(window.listener_for(editor, Editor::down))
        .on_action(window.listener_for(editor, Editor::select_left))
        .on_action(window.listener_for(editor, Editor::select_right))
        .on_action(window.listener_for(editor, Editor::select_up))
        .on_action(window.listener_for(editor, Editor::select_down))
        .on_action(window.listener_for(editor, Editor::select_all))
        .on_action(window.listener_for(editor, Editor::home))
        .on_action(window.listener_for(editor, Editor::end))
        .on_action(window.listener_for(editor, Editor::copy))
        .on_action(window.listener_for(editor, Editor::cut))
        .on_action(window.listener_for(editor, Editor::paste))
        .on_action(window.listener_for(editor, Editor::undo))
        .on_action(window.listener_for(editor, Editor::redo))
        .on_action(window.listener_for(editor, Editor::newline))
        .on_mouse_down(MouseButton::Left, window.listener_for(editor, Editor::mouse_down))
        .on_mouse_move(window.listener_for(editor, Editor::mouse_move))
        .on_mouse_up(MouseButton::Left, window.listener_for(editor, Editor::mouse_up))
        .on_mouse_up_out(MouseButton::Left, window.listener_for(editor, Editor::mouse_up))
        .child(TextElement { editor: editor.clone(), overlay })
}

struct TextElement {
    editor: Entity<Editor>,
    overlay: Option<Overlay>,
}

struct Shaped {
    text: SharedString,
    runs: Vec<TextRun>,
    map: Map,
}

impl TextElement {
    fn shape(&self, cx: &App) -> Shaped {
        let e = self.editor.read(cx);
        let look = &e.look;
        let overlay = self.overlay.clone().or_else(|| {
            (e.text.is_empty() && !e.placeholder.is_empty()).then(|| Overlay {
                at: 0,
                text: e.placeholder.clone(),
                color: look.muted,
                dim: 0..0,
            })
        });
        let (mut text, map) = if e.masked {
            (e.text.chars().map(|_| BULLET).collect::<String>(), Map { insert: None, masked: true })
        } else {
            (e.text.clone(), Map::default())
        };
        let mut map = map;
        if let Some(o) = &overlay
            && !e.masked
        {
            text.insert_str(o.at, &o.text);
            map.insert = Some((o.at, o.text.len()));
        }
        if e.masked && text.is_empty() && !e.placeholder.is_empty() {
            text = e.placeholder.to_string();
            map = Map { insert: Some((0, text.len())), masked: false };
        }

        let d = |ix: usize| map.to_display(&e.text, ix);
        let sel = d(e.selected.start)..d(e.selected.end);
        let marked = e.marked.as_ref().map(|m| d(m.start)..d(m.end));
        let dim = overlay.as_ref().map(|o| d(o.dim.start)..d(o.dim.end)).unwrap_or(0..0);
        let ins = map.insert.map(|(at, len)| at..at + len).unwrap_or(0..0);

        let mut cuts = vec![0, text.len(), sel.start, sel.end, dim.start, dim.end, ins.start, ins.end];
        if let Some(m) = &marked {
            cuts.extend([m.start, m.end]);
        }
        cuts.retain(|&c| c <= text.len());
        cuts.sort_unstable();
        cuts.dedup();
        let inside = |r: &Range<usize>, a: usize| r.start <= a && a < r.end;
        let runs = cuts
            .windows(2)
            .map(|w| {
                let a = w[0];
                let color = match &overlay {
                    Some(o) if inside(&ins, a) => o.color,
                    _ if inside(&dim, a) => look.muted,
                    _ => look.ink,
                };
                TextRun {
                    len: w[1] - a,
                    font: look.font.clone(),
                    color,
                    background_color: inside(&sel, a).then_some(look.selection),
                    underline: marked.as_ref().filter(|m| inside(m, a)).map(|_| UnderlineStyle {
                        thickness: px(1.),
                        color: Some(look.ink),
                        wavy: false,
                    }),
                    strikethrough: None,
                }
            })
            .collect();
        Shaped { text: text.into(), runs, map }
    }
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = Shaped;
    type PrepaintState = Option<Bounds<Pixels>>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Shaped) {
        let shaped = self.shape(cx);
        let look = self.editor.read(cx).look.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let (text, runs) = (shaped.text.clone(), shaped.runs.clone());
        let id = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let rows = window
                .text_system()
                .shape_text(text.clone(), look.size, &runs, width, None)
                .map(|lines| lines.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>())
                .unwrap_or(1)
                .max(1);
            size(width.unwrap_or(px(0.)), look.line_height * rows as f32)
        });
        (id, shaped)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, shaped: &mut Shaped, window: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        let look = self.editor.read(cx).look.clone();
        let wrapped = window
            .text_system()
            .shape_text(shaped.text.clone(), look.size, &shaped.runs, Some(bounds.size.width), None)
            .unwrap_or_default();
        let mut lines = Vec::new();
        let (mut start, mut y) = (0, px(0.));
        for w in wrapped {
            let len = w.len();
            let height = w.size(look.line_height).height;
            lines.push(Line { start, y, wrapped: w });
            start += len + 1;
            y += height;
        }
        let map = shaped.map;
        self.editor.update(cx, |e, _| {
            e.lines = lines;
            e.map = map;
            e.bounds = Some(bounds);
            let caret = e.position(e.cursor()).unwrap_or_default();
            (e.selected.is_empty() && e.focus.is_focused(window))
                .then(|| Bounds::new(bounds.origin + caret, size(px(2.), look.line_height)))
        })
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Shaped, caret: &mut Option<Bounds<Pixels>>, window: &mut Window, cx: &mut App) {
        let focus = self.editor.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        let (lh, accent) = {
            let look = &self.editor.read(cx).look;
            (look.line_height, look.accent)
        };
        let lines = self.editor.update(cx, |e, _| std::mem::take(&mut e.lines));
        for line in &lines {
            let origin = bounds.origin + point(px(0.), line.y);
            let _ = line.wrapped.paint_background(origin, lh, TextAlign::Left, Some(bounds), window, cx);
            let _ = line.wrapped.paint(origin, lh, TextAlign::Left, Some(bounds), window, cx);
        }
        self.editor.update(cx, |e, _| e.lines = lines);
        if let Some(c) = caret.take() {
            window.paint_quad(fill(c, accent));
        }
    }
}

//! Draws the blocks, collects each text leaf's geometry, then paints the caret and marked text
//! and keeps the caret in view.

use super::*;
use gpui_kit::base::text::TextViewStyle;
use gpui_kit::base::text::editor_host::{FlowPosition, RenderContext};
use gpui_kit::prelude::FluentBuilder as _;

impl LiveEditor {
    fn style(&self) -> TextViewStyle {
        let c = &self.theme.c;
        let dark = Hsla::from(c.ground).l < 0.5;
        TextViewStyle::default()
            .with_foreground(c.ink.into())
            .with_muted_foreground(c.muted.into())
            .with_link(c.link.into())
            .with_selection(c.selection.into())
            .with_code_background(c.segment.into())
            .with_border(c.hairline.into())
            .with_dark(dark)
    }

    /// Selection backgrounds per leaf key, in each leaf's drawn bytes.
    fn decorations(&self, leaves: &HashMap<u64, LeafRef>) -> HashMap<u64, Vec<(Range<usize>, Hsla)>> {
        let sel = self.draft.selection().range();
        let mut out = HashMap::new();
        if sel.is_empty() {
            return out;
        }
        let color: Hsla = self.theme.c.selection.into();
        for (&key, &r) in leaves {
            let Some(view) = self.leaf_for(r) else { continue };
            let range = match view.leaf {
                Some(leaf) => {
                    let lr = leaf.source_range();
                    if sel.end <= lr.start || sel.start >= lr.end {
                        continue;
                    }
                    leaf.to_drawn(sel.start.max(lr.start))..leaf.to_drawn(sel.end.min(lr.end))
                }
                None => {
                    let (s, e) = (sel.start.max(view.start), sel.end.min(view.start + view.len));
                    if s >= e {
                        continue;
                    }
                    s - view.start..e - view.start
                }
            };
            if !range.is_empty() {
                out.insert(key, vec![(range, color)]);
            }
        }
        out
    }

    fn image_source(&self) -> impl Fn(&SharedUri) -> ImageSource + Send + Sync + 'static {
        let dir = self.image_dir.clone();
        move |uri: &SharedUri| {
            let url = uri.as_ref();
            let path = PathBuf::from(url.strip_prefix("file://").unwrap_or(url));
            match (&dir, is_remote(url)) {
                (_, true) => ImageSource::from(PathBuf::new()),
                (Some(dir), false) if path.is_relative() => ImageSource::from(dir.join(path)),
                _ => ImageSource::from(path),
            }
        }
    }
}

impl Render for LiveEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let head = self.draft.selection().head;
        // On iPhone the page is being read until it has focus: every block is drawn.
        let editing = !self.form.system_selection() || self.focus.is_focused(window);
        if !self.dragging {
            self.active = self.draft.block_at(head).filter(|_| editing);
        }
        let shown = self.active.map_or(0..self.draft.source().len(), |ix| self.source_range(ix));
        let shown_text = &self.draft.source()[shown.clone()];
        let mut leaves = HashMap::new();
        if self.active.is_some() || self.views.is_empty() {
            leaves.insert(SOURCE_KEY, LeafRef::Source { start: shown.start, len: shown.len() });
        }
        for (ix, view) in self.views.iter().enumerate() {
            if Some(ix) == self.active {
                continue;
            }
            if view.drawn {
                for (leaf, &key) in view.keys.iter().enumerate() {
                    leaves.insert(key, LeafRef::Mapped { block: ix, leaf });
                }
            } else {
                let range = self.draft.blocks()[ix].range.clone();
                leaves.insert(view.id << LEAF_BITS, LeafRef::Source { start: range.start, len: range.len() });
            }
        }
        let active_node = match self.active.and_then(|ix| self.views.get(ix)) {
            Some(view) if self.options.markdown => source_block(&view.node, shown_text),
            _ => BlockNode::Paragraph(source_paragraph(shown_text, SOURCE_KEY)),
        };
        let muted_color: Hsla = self.theme.c.muted.into();
        let muted: Vec<(Range<usize>, Hsla)> = self
            .active
            .map(|ix| self.muted(ix))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| {
                let (s, e) = (r.start.max(shown.start), r.end.min(shown.end));
                (s < e).then(|| (s - shown.start..e - shown.start, muted_color))
            })
            .collect();
        let rcx = RenderContext::new(self.style())
            .with_body_font_size(self.theme.body.size)
            .with_markdown_extensions(self.extensions.clone())
            .with_image_source(self.image_source())
            .on_link_click(|_, _, _, _| {})
            .with_foreground_ranges(SOURCE_KEY, muted);
        let mut nodes: Vec<(ElementId, &BlockNode)> = self
            .views
            .iter()
            .enumerate()
            .map(|(ix, v)| {
                if Some(ix) == self.active { (("live-source", v.id).into(), &active_node) } else { (("live-block", v.id).into(), &v.node) }
            })
            .collect();
        if nodes.is_empty() {
            nodes.push((("live-source", 0u64).into(), &active_node));
        }
        let flat: Vec<&BlockNode> = nodes.iter().map(|n| n.1).collect();
        let children: Vec<AnyElement> = nodes
            .iter()
            .enumerate()
            .map(|(ix, (id, node))| node.render_with_id(id.clone(), FlowPosition::at(&flat, ix, ix + 1 == flat.len()), &rcx, window, cx))
            .collect();
        let system_selection = self.form.system_selection() && self.focus.is_focused(window);
        let decorations = if system_selection { HashMap::new() } else { self.decorations(&leaves) };
        {
            let mut frame = self.frame.borrow_mut();
            frame.revision = self.revision;
            frame.leaves = leaves;
        }
        if let Ok(mut painted) = self.maths.painted.lock() {
            painted.clear();
        }
        let frame = self.frame.clone();
        let body = &self.theme.body;
        let column = div().w_full().children(children).on_children_prepainted(move |bounds, _, _| frame.borrow_mut().blocks = bounds);
        let placeholder = self.placeholder.clone().filter(|_| self.draft.source().is_empty());
        let reading = window.is_a11y_active().then(|| a11y::Reading {
            source: self.draft.source().to_string(),
            selection: self.draft.selection(),
            images: self.views.iter().enumerate().filter(|(ix, _)| Some(*ix) != self.active).flat_map(|(_, v)| v.images.iter().cloned()).collect(),
            frame: self.frame.clone(),
            scale: window.scale_factor(),
        });
        let editor = cx.entity();
        let page = div()
            .id("live-editor")
            .key_context("LiveEditor")
            .role(Role::MultilineTextInput)
            .when_some(self.placeholder.clone(), |page, label| page.aria_label(label.clone()).aria_placeholder(label))
            .when_some(reading.as_ref(), |page, r| page.aria_value(r.source.clone()))
            .on_a11y_action(AccessibleAction::SetValue, move |data, _, cx| {
                if let Some(accesskit::ActionData::Value(value)) = data {
                    editor.update(cx, |e, cx| e.replace(0..e.value().len(), &value[..], cx));
                }
            })
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(self.theme.c.ground)
            .text_color(self.theme.c.ink)
            .font_family(body.face)
            .text_size(body.size)
            .line_height(relative(body.line))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .px(self.form.metrics().gutter)
                    .py(body.size)
                    .child(
                        div()
                            .relative()
                            .w(body.column)
                            .max_w_full()
                            .child(Host { child: Some(column.into_any_element()), decorations, editor: cx.entity() })
                            .children(placeholder.map(|text| div().absolute().top_0().left_0().text_color(self.theme.c.muted).child(text))),
                    ),
            );
        a11y::Accessible { inner: input::actions(page, cx), reading }
    }
}

impl LiveEditor {
    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = (if e.click_count > 1 { self.pressed(e.position) } else { self.hit(e.position) }) else { return };
        self.focus.focus(window, cx);
        if self.form == FormFactor::Phone {
            window.request_virtual_keyboard();
        }
        // A tap inside the selection asks UIKit for its menu and leaves the selection alone.
        let sel = self.draft.selection().range();
        if self.form.system_selection() && e.click_count == 1 && sel.start < at && at < sel.end {
            return;
        }
        self.draft.seal();
        if e.click_count == 1 {
            self.clicked = Some((at, e.position, Instant::now()));
        }
        let picked = match e.click_count {
            1 => at..at,
            2 => self.draft.word_at(at),
            _ => self.draft.line_at(at),
        };
        let anchor = if e.modifiers.shift { self.draft.selection().anchor } else { picked.start };
        self.draft.set_selection(anchor, picked.end);
        self.marked = None;
        self.goal_x = None;
        self.dragging = true;
        cx.notify();
    }

    /// Whether the point is on one of the system's selection handles.
    fn on_system_handle(&self, at: Point<Pixels>) -> bool {
        let sel = self.draft.selection().range();
        if sel.is_empty() {
            return false;
        }
        let reach = self.form.metrics().control / 2.;
        [sel.start, sel.end].into_iter().filter_map(|end| self.caret_bounds(end)).any(|caret| {
            (at.x - caret.left()).abs() <= reach && at.y >= caret.top() - reach && at.y <= caret.bottom() + reach
        })
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging || e.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(at) = self.hit(e.position) {
            let anchor = self.draft.selection().anchor;
            self.draft.set_selection(anchor, at);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging = false;
        self.reveal = true;
        cx.notify();
    }
}

/// The painted caret's width: a hairline that still reads at body size.
const CARET_WIDTH: Pixels = px(1.5);

/// Prepaints the blocks inside a geometry scope, then paints the caret and marked text and
/// registers the editor for text input.
struct Host {
    child: Option<AnyElement>,
    decorations: HashMap<u64, Vec<(Range<usize>, Hsla)>>,
    editor: Entity<LiveEditor>,
}

impl IntoElement for Host {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Host {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().unwrap_or_else(|| div().into_any_element());
        (child.request_layout(window, cx), child)
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, child: &mut AnyElement, window: &mut Window, cx: &mut App) {
        let decorations = std::mem::take(&mut self.decorations);
        let (_, geometry) = eh::collect_leaf_geometry(decorations, || child.prepaint(window, cx));
        self.editor.read(cx).frame.borrow_mut().geometry = geometry;
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, child: &mut AnyElement, _: &mut (), window: &mut Window, cx: &mut App) {
        child.paint(window, cx);
        let editor = self.editor.read(cx);
        let ink = editor.theme.c.ink;
        if let Some(marked) = editor.marked.clone() {
            for line in underline(editor, marked) {
                window.paint_quad(fill(line, ink));
            }
        }
        let caret = editor.caret_at(editor.draft.selection().head);
        if let Some((p, h)) = caret {
            editor.frame.borrow_mut().caret = Some(Bounds::new(p, size(px(2.), h)));
        }
        // A selection shows as its highlight alone, as on the rest of the Mac: no caret beside it.
        if editor.focus.is_focused(window)
            && !editor.form.system_selection()
            && editor.draft.selection().is_empty()
            && let Some((p, h)) = caret
        {
            window.paint_quad(fill(Bounds::new(p, size(CARET_WIDTH, h)), ink).corner_radii(CARET_WIDTH / 2.));
        }
        let reveal = editor.reveal;
        let view = editor.scroll.bounds();
        if reveal && view.size.height > px(0.) && let Some((p, h)) = caret {
            let margin = editor.theme.body.size * 2.;
            let offset = editor.scroll.offset();
            let dy = if p.y + h + margin > view.bottom() {
                view.bottom() - (p.y + h + margin)
            } else if p.y - margin < view.top() {
                view.top() - (p.y - margin)
            } else {
                px(0.)
            };
            let y = (offset.y + dy).clamp(-editor.scroll.max_offset().y, px(0.));
            if (y - offset.y).abs() > px(0.5) {
                editor.scroll.set_offset(point(offset.x, y));
                window.refresh();
            }
        }
        let focus = editor.focus.clone();
        // The platform's selection UI is clipped to these bounds: the page that shows, not
        // the whole column.
        let shown = if view.size.height > px(0.) { view } else { bounds };
        window.handle_input(&focus, ElementInputHandler::new(shown, self.editor.clone()), cx);
        if editor.form.system_selection() {
            // A touch on a selection handle is UIKit's to drag. Claiming it keeps it from
            // scrolling the page or placing the caret.
            let page = self.editor.clone();
            window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                if phase.bubble() && event.phase == TouchPhase::Started && page.read(cx).on_system_handle(event.start_position) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            });
            // A long press is UIKit's too: it moves the caret under a loupe. Claiming it keeps
            // the finger's moves out of GPUI's pan, so the page does not scroll under it.
            let page = self.editor.clone();
            window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                if event.phase == TouchPhase::Started {
                    if window.default_prevented() || !shown.contains(&event.start_position) {
                        return;
                    }
                    window.capture_long_press(&page);
                } else if !window.has_long_press_capture(&page) {
                    return;
                }
                window.prevent_default();
                cx.stop_propagation();
            });
        }
        if reveal && caret.is_some() {
            self.editor.update(cx, |e, _| e.reveal = false);
        }
    }
}

/// One underline per visual line of the marked range.
pub(super) fn underline(editor: &LiveEditor, marked: Range<usize>) -> Vec<Bounds<Pixels>> {
    let mut lines: Vec<Bounds<Pixels>> = vec![];
    let source = editor.draft.source();
    let stops = source[marked.clone()].char_indices().map(|(i, _)| marked.start + i).chain([marked.end]);
    let mut prev: Option<(Point<Pixels>, Pixels)> = None;
    for at in stops {
        let Some((p, h)) = editor.caret_at(at) else { continue };
        if let Some((q, qh)) = prev
            && (q.y - p.y).abs() < px(1.)
        {
            let y = q.y + qh;
            match lines.last_mut() {
                Some(l) if (l.top() - y).abs() < px(0.5) && l.right() >= q.x - px(0.5) => l.size.width = p.x - l.left(),
                _ => lines.push(Bounds::new(point(q.x, y), size(p.x - q.x, px(1.5)))),
            }
        }
        prev = Some((p, h));
    }
    lines
}

//! The live editor: one editor over the Markdown source that draws every block as GPUI Kit
//! renders it, except the block holding the caret, which shows its source with the syntax muted.
//! The draft stays plain Markdown; `Draft` owns it and every change.

mod element;
mod input;
mod maths;

pub use input::bind_keys;

use crate::FormFactor;
use crate::theme::Theme;
use gpui_kit::base::text::editor_host::{self as eh, BlockNode, CodeBlock, InlineNode, LeafGeometry, Paragraph, Table};
use gpui_kit::base::text::{MarkdownExtensions, MarkdownNode};
use gpui_kit::*;
use inkling_core::Appearance;
use inkling_core::draft::{ATOM, Claim, Draft, Selection};
use maths::{Maths, MathsPlugin, REMOTE_IMAGE, RemoteImage};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What the editor draws. With both off, every block is its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub markdown: bool,
    pub maths: bool,
}

/// Emitted when the source changes: an edit, an undo or a redo.
pub struct Edited;

/// The leaf key of the block holding the caret.
const SOURCE_KEY: u64 = u64::MAX;
/// Leaf keys are a view's id shifted left by this, plus the leaf's index.
const LEAF_BITS: u32 = 12;

/// Where a drawn leaf's bytes come from.
#[derive(Clone, Copy, Debug)]
enum LeafRef {
    /// Leaf `leaf` of block `block`'s `SourceMap`.
    Mapped { block: usize, leaf: usize },
    /// `len` bytes of source text starting at `start`.
    Source { start: usize, len: usize },
}

/// One block as it is drawn when the caret is elsewhere.
struct View {
    id: u64,
    source: String,
    node: BlockNode,
    /// The leaf keys in `node`, in `SourceMap` order. Empty for a block drawn as source.
    keys: Vec<u64>,
    drawn: bool,
}

/// The last painted frame: what each leaf key is and where it was laid out.
#[derive(Default)]
struct Frame {
    revision: u64,
    leaves: HashMap<u64, LeafRef>,
    geometry: HashMap<u64, LeafGeometry>,
    blocks: Vec<Bounds<Pixels>>,
    /// The caret as it was last painted.
    caret: Option<Bounds<Pixels>>,
}

pub struct LiveEditor {
    draft: Draft,
    options: Options,
    form: FormFactor,
    theme: Theme,
    image_dir: Option<PathBuf>,
    maths: Arc<Maths>,
    extensions: Arc<MarkdownExtensions>,
    views: Vec<View>,
    next_id: u64,
    revision: u64,
    /// The block drawn as source, kept still while a drag selects.
    active: Option<usize>,
    dragging: bool,
    /// Shown on an empty page.
    placeholder: Option<SharedString>,
    /// The last single click: the source byte it landed on, where and when.
    clicked: Option<(usize, Point<Pixels>, Instant)>,
    marked: Option<Range<usize>>,
    goal_x: Option<Pixels>,
    reveal: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
    frame: Rc<RefCell<Frame>>,
    /// Blocks whose kit text didn't match their `SourceMap`, drawn as source instead.
    mismatches: Vec<String>,
}

impl LiveEditor {
    pub fn new(
        text: &str,
        options: Options,
        form: FormFactor,
        appearance: &Appearance,
        image_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let maths = Arc::new(Maths::default());
        let theme = Theme::new(form, appearance, &[]);
        let claim = Self::claim_for(&maths, options);
        let mut editor = LiveEditor {
            draft: Draft::new(text, claim),
            options,
            form,
            theme,
            image_dir,
            extensions: Arc::default(),
            maths,
            views: vec![],
            next_id: 1,
            revision: 0,
            active: None,
            dragging: false,
            placeholder: None,
            clicked: None,
            marked: None,
            goal_x: None,
            reveal: true,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            frame: Rc::default(),
            mismatches: vec![],
        };
        editor.extensions = Arc::new(editor.build_extensions());
        editor.rebuild();
        // On iPhone the page waits for a tap, which also raises the keyboard.
        if form == FormFactor::Mac {
            editor.focus.focus(window, cx);
        }
        editor
    }

    fn claim_for(maths: &Arc<Maths>, options: Options) -> Claim {
        if options.markdown && options.maths { maths.claim() } else { Arc::new(|_, _| false) }
    }

    fn build_extensions(&self) -> MarkdownExtensions {
        let ext = MarkdownExtensions::default().plugin(RemoteImage { muted: self.theme.c.muted });
        if !(self.options.markdown && self.options.maths) {
            return ext;
        }
        let plugin = |display| MathsPlugin {
            maths: self.maths.clone(),
            claim: self.maths.claim(),
            display,
            size: self.theme.body.size,
            ink: self.theme.c.ink,
        };
        ext.plugin(plugin(false)).plugin(plugin(true))
    }

    pub fn value(&self) -> &str {
        self.draft.source()
    }

    pub fn selection(&self) -> Selection {
        self.draft.selection()
    }

    pub fn options(&self) -> Options {
        self.options
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn set_placeholder(&mut self, text: impl Into<SharedString>) {
        self.placeholder = Some(text.into());
    }

    pub fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        if options == self.options {
            return;
        }
        self.options = options;
        self.extensions = Arc::new(self.build_extensions());
        self.views.clear();
        self.draft.set_claim(Self::claim_for(&self.maths, options));
        self.changed(cx);
    }

    pub fn set_appearance(&mut self, appearance: &Appearance, cx: &mut Context<Self>) {
        self.set_theme(Theme::new(self.form, appearance, &[]), cx);
    }

    /// The app's theme, which knows the faces this device has.
    pub(crate) fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.extensions = Arc::new(self.build_extensions());
        cx.notify();
    }

    pub fn set_selection(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.draft.seal();
        self.draft.set_selection(range.start, range.end);
        self.moved(cx);
    }

    /// Replaces `range` of the source with `text`, as one undo step.
    pub fn replace(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.draft.seal();
        self.edit(range, text, cx);
        self.draft.seal();
    }

    fn edit(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.draft.replace(range, text);
        self.marked = None;
        self.source_changed(cx);
    }

    fn source_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(Edited);
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        self.goal_x = None;
        self.rebuild();
        self.moved(cx);
    }

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.reveal = true;
        cx.notify();
    }

    /// Rebuilds each block's drawn view, reusing those whose source didn't change.
    fn rebuild(&mut self) {
        let mut old: HashMap<String, Vec<View>> = HashMap::new();
        for view in self.views.drain(..) {
            old.entry(view.source.clone()).or_default().push(view);
        }
        let definitions: String = self.draft.definitions().map(|d| format!("\n\n{d}")).collect();
        for ix in 0..self.draft.blocks().len() {
            let source = &self.draft.source()[self.draft.blocks()[ix].range.clone()];
            let view = match old.get_mut(source).and_then(Vec::pop) {
                Some(view) => view,
                None => self.build_view(ix, &definitions),
            };
            self.views.push(view);
        }
    }

    fn build_view(&mut self, ix: usize, definitions: &str) -> View {
        let id = self.next_id;
        self.next_id += 1;
        let block = &self.draft.blocks()[ix];
        let source = self.draft.source()[block.range.clone()].to_string();
        let source_view = |source: String| {
            let node = BlockNode::Paragraph(source_paragraph(&source, id << LEAF_BITS));
            View { id, source, node, keys: vec![], drawn: false }
        };
        let Some(map) = block.map.as_ref().filter(|m| self.options.markdown && m.leaves.len() < (1 << LEAF_BITS) - 1)
        else {
            return source_view(source);
        };
        let parsed = eh::parse_markdown(&format!("{source}{definitions}"), self.extensions.clone()).unwrap_or_default();
        let mut nodes = parsed.into_iter().filter(|n| !matches!(n, BlockNode::Definition { .. }));
        let (Some(node), None) = (nodes.next(), nodes.next()) else {
            self.mismatches.push(source.clone());
            return source_view(source);
        };
        let mut keyer = Keyer { next: id << LEAF_BITS, keys: vec![], texts: vec![] };
        let node = keyer.block(node);
        let expected: Vec<&str> = map.leaves.iter().map(|l| l.text()).collect();
        if keyer.texts != expected {
            self.mismatches.push(source.clone());
            return source_view(source);
        }
        View { id, source, node, keys: keyer.keys, drawn: true }
    }

    /// The source range drawn for block `ix` while it holds the caret: the block, plus the blank
    /// lines after it up to the caret when the caret sits in them.
    fn source_range(&self, ix: usize) -> Range<usize> {
        let blocks = self.draft.blocks();
        let Some(b) = blocks.get(ix) else { return 0..self.draft.source().len() };
        let start = if ix == 0 { 0 } else { b.range.start };
        let region_end = blocks.get(ix + 1).map_or(self.draft.source().len(), |n| n.range.start);
        let head = self.draft.selection().head;
        let end = if (b.range.end..=region_end).contains(&head) { head.max(b.range.end) } else { b.range.end };
        start..end
    }

    fn leaf_for(&self, r: LeafRef) -> Option<LeafView<'_>> {
        match r {
            LeafRef::Mapped { block, leaf } => {
                let leaf = self.draft.blocks().get(block)?.map.as_ref()?.leaves.get(leaf)?;
                Some(LeafView { leaf: Some(leaf), start: 0, len: leaf.text().len() })
            }
            LeafRef::Source { start, len } => Some(LeafView { leaf: None, start, len }),
        }
    }

    /// The source byte for drawn byte `index` of leaf `key`.
    fn to_source(&self, key: u64, index: usize) -> Option<usize> {
        let r = *self.frame.borrow().leaves.get(&key)?;
        let view = self.leaf_for(r)?;
        Some(match view.leaf {
            Some(leaf) => leaf.to_source(index),
            None => (view.start + index).min(self.draft.source().len()),
        })
    }

    /// Where source byte `at` is drawn: its leaf key and drawn index.
    fn locate(&self, at: usize) -> Option<(u64, usize)> {
        let frame = self.frame.borrow();
        if frame.revision != self.revision {
            return None;
        }
        let mut best = None;
        for (&key, &r) in &frame.leaves {
            let Some(view) = self.leaf_for(r) else { continue };
            let range = match view.leaf {
                Some(leaf) => leaf.source_range(),
                None => view.start..view.start + view.len,
            };
            if range.contains(&at) || range.end == at {
                let index = view.leaf.map_or(at - view.start, |l| l.to_drawn(at));
                let source_leaf = key == SOURCE_KEY;
                if best.is_none() || source_leaf {
                    best = Some((key, index));
                }
            }
        }
        best
    }

    /// The top of the caret for source byte `at`, and its height.
    fn caret_at(&self, at: usize) -> Option<(Point<Pixels>, Pixels)> {
        let (key, index) = self.locate(at)?;
        let frame = self.frame.borrow();
        let g = frame.geometry.get(&key)?;
        Some((g.position_for_index(index), g.caret_height()))
    }

    /// A caret's box at source byte `at`. While an edit waits for its frame, the last caret
    /// painted: the platform's text input asks between the two and needs a finite answer.
    fn caret_bounds(&self, at: usize) -> Option<Bounds<Pixels>> {
        self.caret_at(at).map(|(p, h)| Bounds::new(p, size(px(2.), h))).or(self.frame.borrow().caret)
    }

    /// The line boxes the source range covers, top to bottom, one per visual line of each leaf.
    fn line_rects(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let frame = self.frame.borrow();
        if range.is_empty() || frame.revision != self.revision {
            return vec![];
        }
        let mut rects: Vec<Bounds<Pixels>> = vec![];
        for (key, &r) in &frame.leaves {
            let (Some(view), Some(g)) = (self.leaf_for(r), frame.geometry.get(key)) else { continue };
            let (source, text) = match view.leaf {
                Some(leaf) => (leaf.source_range(), leaf.text()),
                None => (view.start..view.start + view.len, &self.draft.source()[view.start..view.start + view.len]),
            };
            let (from, to) = (range.start.max(source.start), range.end.min(source.end));
            if from >= to {
                continue;
            }
            let drawn = view.leaf.map_or(from - view.start..to - view.start, |l| l.to_drawn(from)..l.to_drawn(to));
            let height = g.line_height();
            let mut line: Option<Bounds<Pixels>> = None;
            let stops = text.get(drawn.clone()).into_iter().flat_map(|t| t.char_indices()).map(|(i, _)| drawn.start + i).chain([drawn.end]);
            let on = |l: &Bounds<Pixels>, p: Point<Pixels>| (l.top() - p.y).abs() < px(1.) && p.x >= l.left();
            for stop in stops {
                // At a soft wrap one byte is two places: the end of the line above and the
                // start of the line below.
                let (end, start) = (g.position_for_caret(eh::Caret::upstream(stop)), g.position_for_caret(eh::Caret::new(stop)));
                if let Some(l) = line.as_mut().filter(|l| on(l, end)) {
                    l.size.width = end.x - l.left();
                }
                if !line.as_ref().is_some_and(|l| on(l, start)) {
                    rects.extend(line.replace(Bounds::new(start, size(px(0.), height))));
                }
            }
            rects.extend(line);
        }
        rects.retain(|r| r.size.width > px(0.));
        rects.sort_by(|a, b| (a.top(), a.left()).partial_cmp(&(b.top(), b.left())).unwrap_or(std::cmp::Ordering::Equal));
        rects
    }

    /// The source byte a press at `at` means. The first click can redraw its block as source
    /// and move the text, so a press that follows it at once in the same place means the byte
    /// the first one landed on: the second click of a double click, and UIKit's reading of it.
    fn pressed(&self, at: Point<Pixels>) -> Option<usize> {
        let near = |p: Point<Pixels>| (p.x - at.x).abs() < px(24.) && (p.y - at.y).abs() < px(24.);
        match self.clicked {
            Some((byte, p, when)) if near(p) && when.elapsed() < Duration::from_millis(500) => Some(byte),
            _ => self.hit(at),
        }
    }

    /// The source byte nearest the window point: the closest leaf, or the start of a block that
    /// draws no text, such as a display formula or a rule.
    fn hit(&self, at: Point<Pixels>) -> Option<usize> {
        let frame = self.frame.borrow();
        if frame.revision != self.revision {
            return None;
        }
        let leaf = frame.geometry.iter().map(|(k, g)| (distance(g.bounds(), at), Some(*k), g)).min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let empty_block = frame
            .blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| self.views.get(*i).is_some_and(|v| v.drawn && v.keys.is_empty()) && Some(*i) != self.active)
            .map(|(i, b)| (distance(*b, at), i))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        match (leaf, empty_block) {
            (Some((d, _, _)), Some((e, i))) if e < d => Some(self.draft.blocks()[i].range.start),
            (Some((_, Some(key), g)), _) => {
                let index = g.index_for_position(at);
                drop(frame);
                self.to_source(key, index)
            }
            (_, Some((_, i))) => Some(self.draft.blocks()[i].range.start),
            _ => None,
        }
    }

    /// For the headless proof.
    pub fn snapshot(&self) -> Snapshot {
        let frame = self.frame.borrow();
        let mut leaves = vec![];
        for (&key, &r) in &frame.leaves {
            let Some(view) = self.leaf_for(r) else { continue };
            let (block, source) = match (r, view.leaf) {
                (LeafRef::Mapped { block, .. }, Some(leaf)) => (block, leaf.source_range()),
                (LeafRef::Source { start, .. }, _) => (self.draft.block_at(start).unwrap_or(0), start..start + view.len),
                _ => continue,
            };
            let text = match view.leaf {
                Some(leaf) => leaf.text().to_string(),
                None => self.draft.source()[source.clone()].to_string(),
            };
            let geometry = frame.geometry.get(&key);
            leaves.push(SnapLeaf {
                key,
                block,
                source,
                text,
                is_source: view.leaf.is_none(),
                bounds: geometry.map(LeafGeometry::bounds),
                line_height: geometry.map(LeafGeometry::line_height),
            });
        }
        leaves.sort_by_key(|l| (l.block, l.source.start));
        let active = self.active;
        let muted = active.map(|ix| self.muted(ix)).unwrap_or_default();
        Snapshot {
            leaves,
            active,
            blocks: frame.blocks.clone(),
            muted,
            formulas: self.maths.painted.lock().map(|p| p.clone()).unwrap_or_default(),
            mismatches: self.mismatches.clone(),
            marked: self.marked.clone(),
            caret: self.caret_at(self.draft.selection().head),
            underlines: self.marked.clone().map(|m| element::underline(self, m)).unwrap_or_default(),
            ink: self.theme.c.ink,
            ground: self.theme.c.ground,
        }
    }

    /// Where source byte `at` is drawn: the top of a caret there and its height.
    pub fn point_for(&self, at: usize) -> Option<(Point<Pixels>, Pixels)> {
        self.caret_at(at)
    }

    /// The top of a caret before drawn byte `index` of leaf `key`, from the last frame.
    pub fn drawn_point(&self, key: u64, index: usize) -> Option<Point<Pixels>> {
        self.frame.borrow().geometry.get(&key).map(|g| g.position_for_index(index))
    }

    /// The source bytes of the caret's block drawn muted: its Markdown syntax.
    fn muted(&self, ix: usize) -> Vec<Range<usize>> {
        let Some(block) = self.draft.blocks().get(ix) else { return vec![] };
        match (&block.map, self.options.markdown) {
            (Some(map), true) => map.markers(block.range.clone()).into_iter().filter(|r| !self.draft.source()[r.clone()].trim().is_empty()).collect(),
            _ => vec![],
        }
    }
}

impl EventEmitter<Edited> for LiveEditor {}

/// What a headless test can read about the last frame.
#[derive(Debug)]
pub struct Snapshot {
    pub leaves: Vec<SnapLeaf>,
    pub active: Option<usize>,
    pub blocks: Vec<Bounds<Pixels>>,
    /// Source ranges drawn muted in the caret's block.
    pub muted: Vec<Range<usize>>,
    /// Each formula drawn as an image last frame: its TeX and its bounds.
    pub formulas: Vec<(String, Bounds<Pixels>)>,
    pub mismatches: Vec<String>,
    pub marked: Option<Range<usize>>,
    pub caret: Option<(Point<Pixels>, Pixels)>,
    /// The underline drawn under marked text, one per visual line.
    pub underlines: Vec<Bounds<Pixels>>,
    pub ink: Rgba,
    pub ground: Rgba,
}

#[derive(Debug)]
pub struct SnapLeaf {
    pub key: u64,
    pub block: usize,
    pub source: Range<usize>,
    pub text: String,
    /// Drawn as the source itself rather than rendered.
    pub is_source: bool,
    pub bounds: Option<Bounds<Pixels>>,
    pub line_height: Option<Pixels>,
}

struct LeafView<'a> {
    leaf: Option<&'a inkling_core::draft::Leaf>,
    start: usize,
    len: usize,
}

fn source_paragraph(text: &str, key: u64) -> Paragraph {
    let mut p = Paragraph::default();
    p.push(InlineNode::new(text.to_string()));
    p.with_leaf_key(key)
}

/// `(dy, dx)` from `at` to the nearest point of `b`, so the line under the point wins.
fn distance(b: Bounds<Pixels>, at: Point<Pixels>) -> (f32, f32) {
    let gap = |lo: Pixels, hi: Pixels, v: Pixels| if v < lo { lo - v } else if v > hi { v - hi } else { px(0.) };
    (gap(b.top(), b.bottom(), at.y).as_f32(), gap(b.left(), b.right(), at.x).as_f32())
}

fn is_remote(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| scheme.len() > 1 && scheme != "file")
}

/// Keys the text leaves of a parsed block in the order `SourceMap` lists them, records the text
/// the kit will draw for each, and puts alt text in place of remote images.
struct Keyer {
    next: u64,
    keys: Vec<u64>,
    texts: Vec<String>,
}

impl Keyer {
    fn key(&mut self) -> u64 {
        let k = self.next;
        self.next += 1;
        self.keys.push(k);
        k
    }

    fn paragraph(&mut self, p: Paragraph) -> Paragraph {
        let remote = p.children().iter().any(|c| c.as_image().is_some_and(|i| is_remote(&i.url)));
        let p = if remote {
            let mut out = Paragraph::default();
            for c in p.children() {
                match c.as_image() {
                    Some(i) if is_remote(&i.url) => {
                        let alt = format!("[{}]", i.alt.as_deref().unwrap_or("image"));
                        out.push(InlineNode::custom(MarkdownNode::new(REMOTE_IMAGE, ()).text(alt)));
                    }
                    _ => out.push(c.clone()),
                }
            }
            out
        } else {
            p
        };
        let text: String = p.children().iter().map(|c| if c.as_image().is_some() || c.as_custom().is_some() { ATOM } else { c.text() }).collect();
        self.texts.push(text);
        let key = self.key();
        p.with_leaf_key(key)
    }

    fn blocks(&mut self, children: Vec<BlockNode>) -> Vec<BlockNode> {
        children.into_iter().map(|c| self.block(c)).collect()
    }

    fn block(&mut self, node: BlockNode) -> BlockNode {
        match node {
            BlockNode::Paragraph(p) => BlockNode::Paragraph(self.paragraph(p)),
            BlockNode::Heading { level, children, span } => BlockNode::Heading { level, children: self.paragraph(children), span },
            BlockNode::CodeBlock(c) => {
                self.texts.push(c.code().to_string());
                let key = self.key();
                BlockNode::CodeBlock(c.with_leaf_key(key))
            }
            BlockNode::List { children, ordered, start, span } => BlockNode::List { children: self.blocks(children), ordered, start, span },
            BlockNode::ListItem { children, spread, checked, task_marker, span } => {
                BlockNode::ListItem { children: self.blocks(children), spread, checked, task_marker, span }
            }
            BlockNode::Blockquote { children, span } => BlockNode::Blockquote { children: self.blocks(children), span },
            BlockNode::Root { children, span } => BlockNode::Root { children: self.blocks(children), span },
            BlockNode::Table(t) => {
                let rows = t.rows().map(|row| row.map(|cell| self.paragraph(cell.clone())).collect()).collect();
                BlockNode::Table(Table::new(rows, t.column_aligns().to_vec()))
            }
            other => other,
        }
    }
}

/// The caret's block as styled source: a heading keeps its level, code stays code.
fn source_block(node: &BlockNode, text: &str) -> BlockNode {
    match node {
        BlockNode::Heading { level, .. } => BlockNode::Heading { level: *level, children: source_paragraph(text, SOURCE_KEY), span: None },
        BlockNode::CodeBlock(c) => BlockNode::CodeBlock(CodeBlock::new(text.to_string().into(), c.lang(), None::<eh::Span>).with_leaf_key(SOURCE_KEY)),
        _ => BlockNode::Paragraph(source_paragraph(text, SOURCE_KEY)),
    }
}

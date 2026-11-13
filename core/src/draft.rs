//! The draft as the live editor holds it: the Markdown source, the selection, the top-level
//! blocks and the undo log. Every change goes through `Draft::replace`.
//!
//! `SourceMap` says which source bytes draw each byte of a block's drawn text. It walks the
//! `markdown` crate's tree, the parser GPUI Kit draws from, so a leaf's text is the kit's.

use markdown::ParseOptions;
use markdown::mdast::Node;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Whether a formula is drawn as maths: its TeX and whether it is a display block.
pub type Claim = Arc<dyn Fn(&str, bool) -> bool + Send + Sync>;

/// What an image or a drawn formula takes in a leaf's text, as in GPUI Kit's leaf geometry.
pub const ATOM: &str = "\u{FFFC}";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Selection { anchor: at, head: at }
    }

    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Debug, PartialEq)]
pub struct Block {
    pub range: Range<usize>,
    /// `None` when the block holds something the map can't follow, so it is drawn as source.
    pub map: Option<SourceMap>,
}

/// How one block's drawn text maps to its source: one leaf per paragraph, heading, table cell or
/// code block, in the order GPUI Kit renders them.
#[derive(Debug, Default, PartialEq)]
pub struct SourceMap {
    pub leaves: Vec<Leaf>,
}

impl SourceMap {
    /// The bytes of `block` that draw nothing: Markdown syntax such as `**`, `# `, `- `, link
    /// targets and the backslash of an escape.
    pub fn markers(&self, block: Range<usize>) -> Vec<Range<usize>> {
        let mut out = vec![];
        let mut at = block.start;
        for s in self.leaves.iter().flat_map(|l| &l.segments) {
            if at < s.source.start {
                out.push(at..s.source.start);
            }
            if s.kind == Kind::Escape {
                out.push(s.source.start..s.source.start + 1);
            }
            at = at.max(s.source.end);
        }
        if at < block.end {
            out.push(at..block.end);
        }
        out
    }

    /// Moves every source byte by `by`, for a block that an edit before it moved.
    fn shift(&mut self, by: isize) {
        let at = |b: &mut usize| *b = b.wrapping_add_signed(by);
        for leaf in &mut self.leaves {
            at(&mut leaf.at);
            for s in &mut leaf.segments {
                at(&mut s.source.start);
                at(&mut s.source.end);
            }
        }
    }
}

/// One drawn run of text and where each piece of it comes from. A segment that isn't `Same`
/// (an escape, an entity, a line ending, an image) is one caret stop.
#[derive(Debug, Default, PartialEq)]
pub struct Leaf {
    text: String,
    segments: Vec<Segment>,
    at: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment {
    drawn: Range<usize>,
    source: Range<usize>,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Drawn exactly as written.
    Same,
    /// `\*` drawn as `*`.
    Escape,
    /// `&amp;` drawn as `&`.
    Entity,
    /// A soft line break drawn as a space.
    LineEnding,
    /// A hard break drawn as a new line.
    Break,
    /// An image or a formula.
    Atom,
}

impl Leaf {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The source bytes this leaf draws from, markers between its segments included.
    pub fn source_range(&self) -> Range<usize> {
        match (self.segments.first(), self.segments.last()) {
            (Some(first), Some(last)) => first.source.start..last.source.end,
            _ => self.at..self.at,
        }
    }

    /// The source byte for drawn byte `drawn`. A caret between two segments sits at the start
    /// of the second, so typing there joins the text after it.
    pub fn to_source(&self, drawn: usize) -> usize {
        let Some(s) = self.segments.iter().find(|s| drawn < s.drawn.end) else {
            return self.segments.last().map_or(self.at, |s| s.source.end);
        };
        match s.kind {
            Kind::Same => s.source.start + drawn.saturating_sub(s.drawn.start),
            _ if drawn <= s.drawn.start => s.source.start,
            _ => s.source.end,
        }
    }

    /// The drawn byte for source byte `source`. Markers between segments draw nothing, so they
    /// take the position of the text after them.
    pub fn to_drawn(&self, source: usize) -> usize {
        let Some(s) = self.segments.iter().find(|s| source < s.source.end) else {
            return self.text.len();
        };
        match s.kind {
            _ if source <= s.source.start => s.drawn.start,
            Kind::Same => {
                let mut d = s.drawn.start + (source - s.source.start);
                while !self.text.is_char_boundary(d) {
                    d -= 1;
                }
                d
            }
            _ => s.drawn.end,
        }
    }

    fn push(&mut self, drawn: &str, source: Range<usize>, kind: Kind) {
        let start = self.text.len();
        self.text.push_str(drawn);
        let drawn = start..self.text.len();
        if kind == Kind::Same
            && let Some(last) = self.segments.last_mut()
            && last.kind == Kind::Same
            && last.drawn.end == drawn.start
            && last.source.end == source.start
        {
            last.drawn.end = drawn.end;
            last.source.end = source.end;
            return;
        }
        self.segments.push(Segment { drawn, source, kind });
    }
}

/// `&name;`, `&#35;` or `&#x23;` decoded, or `None` when it is no character reference.
fn decode_entity(source: &str) -> Option<String> {
    let inner = source.strip_prefix('&')?.strip_suffix(';')?;
    match inner.strip_prefix('#') {
        Some(number) => {
            let (digits, radix, max) = match number.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16, 6),
                None => (number, 10, 7),
            };
            let valid = !digits.is_empty() && digits.len() <= max && digits.chars().all(|c| c.is_digit(radix));
            valid.then(|| markdown::decode_numeric(digits, radix))
        }
        None => markdown::decode_named(inner, true),
    }
}

fn options(maths: bool) -> ParseOptions {
    let mut o = ParseOptions::gfm();
    o.constructs.math_text = maths;
    o.constructs.math_flow = maths;
    o
}

/// Whether inline maths at `range` of `source` is drawn as a formula. Dollars that read as prose
/// stay text: a space just inside either dollar, or a digit right after the closing one, as in
/// `$5 and $10`.
pub fn draws_inline_maths(source: &str, range: Range<usize>, tex: &str, claim: &Claim) -> bool {
    let raw = &source[range.clone()];
    let fence = raw.bytes().take_while(|b| *b == b'$').count();
    let inner = raw.get(fence..raw.len().saturating_sub(fence)).unwrap_or_default();
    let prose = inner.is_empty()
        || inner.starts_with(char::is_whitespace)
        || inner.ends_with(char::is_whitespace)
        || source[range.end..].starts_with(|c: char| c.is_ascii_digit());
    !prose && claim(tex, false)
}

/// GPUI Kit draws unclaimed inline maths as ordinary Markdown when its text holds markup. The
/// same test and parse as the kit's, so the drawn text matches.
fn reparse_as_prose(literal: &str) -> Option<Vec<Node>> {
    let markup = literal.bytes().any(|b| matches!(b, b'<' | b'*' | b'_' | b'[' | b'`' | b'~' | b'\\' | b'!' | b'&'))
        || literal.contains("://")
        || literal.contains("www.");
    if !markup {
        return None;
    }
    let Ok(Node::Root(root)) = markdown::to_mdast(literal, &options(false)) else { return None };
    let [Node::Paragraph(paragraph)] = root.children.as_slice() else { return None };
    if paragraph.children.iter().all(|c| matches!(c, Node::Text(_))) {
        return None;
    }
    Some(paragraph.children.clone())
}

fn span(node: &Node, offset: usize) -> Option<Range<usize>> {
    node.position().map(|p| offset + p.start.offset..offset + p.end.offset)
}

/// Builds the leaves of one block from the document's tree.
struct Mapper<'a> {
    source: &'a str,
    claim: &'a Claim,
    definitions: &'a HashSet<String>,
}

impl Mapper<'_> {
    fn block(&self, node: &Node) -> Option<SourceMap> {
        let mut leaves = vec![];
        self.blocks(node, &mut leaves)?;
        Some(SourceMap { leaves })
    }

    fn blocks(&self, node: &Node, leaves: &mut Vec<Leaf>) -> Option<()> {
        let range = span(node, 0)?;
        match node {
            Node::Paragraph(p) => leaves.push(self.inline_leaf(&p.children, range.start)?),
            Node::Heading(h) => leaves.push(self.inline_leaf(&h.children, range.start)?),
            Node::Code(c) => leaves.push(self.code_leaf(&c.value, range)?),
            Node::Math(m) if !(self.claim)(&m.value, true) => leaves.push(self.code_leaf(&m.value, range)?),
            Node::Math(_) | Node::ThematicBreak(_) => {}
            Node::List(_) | Node::ListItem(_) | Node::Blockquote(_) => {
                for child in node.children()? {
                    self.blocks(child, leaves)?;
                }
            }
            Node::Table(t) => {
                for row in &t.children {
                    for cell in row.children()?.iter().take(t.align.len()) {
                        leaves.push(self.inline_leaf(cell.children()?, span(cell, 0)?.start)?);
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn inline_leaf(&self, children: &[Node], at: usize) -> Option<Leaf> {
        let mut leaf = Leaf { at, ..Leaf::default() };
        for child in children {
            self.inline(child, 0, &mut leaf)?;
        }
        Some(leaf)
    }

    fn inline(&self, node: &Node, offset: usize, leaf: &mut Leaf) -> Option<()> {
        let range = span(node, offset)?;
        match node {
            Node::Text(t) => self.prose(&t.value, range, leaf)?,
            Node::Emphasis(_) | Node::Strong(_) | Node::Delete(_) | Node::Link(_) | Node::LinkReference(_) => {
                for child in node.children()? {
                    self.inline(child, offset, leaf)?;
                }
            }
            Node::InlineCode(c) => {
                let raw = &self.source[range.clone()];
                let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                self.code(&c.value, range.start + ticks..range.end - ticks, leaf)?;
            }
            Node::Break(_) => leaf.push("\n", range, Kind::Break),
            Node::Image(_) => leaf.push(ATOM, range, Kind::Atom),
            Node::ImageReference(i) if self.definitions.contains(&i.identifier) => leaf.push(ATOM, range, Kind::Atom),
            Node::ImageReference(_) => {}
            Node::InlineMath(m) => {
                if draws_inline_maths(self.source, range.clone(), &m.value, self.claim) {
                    leaf.push(ATOM, range, Kind::Atom);
                } else if let Some(children) = reparse_as_prose(&self.source[range.clone()]) {
                    for child in &children {
                        self.inline(child, range.start, leaf)?;
                    }
                } else {
                    leaf.push(&self.source[range.clone()], range, Kind::Same);
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// Aligns a text node's value with its source. Line endings draw as spaces, and the
    /// whitespace and `>` the parser drops around them draw nothing.
    fn prose(&self, value: &str, range: Range<usize>, leaf: &mut Leaf) -> Option<()> {
        let src = self.source;
        let mut at = range.start;
        let mut i = 0;
        while i < value.len() {
            let rest = &value[i..];
            let c = rest.chars().next()?;
            if c == '\n' || c == '\r' {
                i += if rest.starts_with("\r\n") { 2 } else { 1 };
                at = skip(src, at, range.end, b" \t");
                let len = line_ending(&src[at..range.end])?;
                leaf.push(" ", at..at + len, Kind::LineEnding);
                at += len;
                continue;
            }
            let (from, src_len, drawn_len, kind) = loop {
                let here = &src[at..range.end];
                if let Some(escaped) = here.strip_prefix('\\')
                    && c.is_ascii_punctuation()
                    && escaped.starts_with(c)
                {
                    break (at, 1 + c.len_utf8(), c.len_utf8(), Kind::Escape);
                }
                if here.starts_with('&')
                    && let Some(semi) = here.find(';').filter(|s| *s < 40)
                    && let Some(decoded) = decode_entity(&here[..=semi])
                    && rest.starts_with(decoded.as_str())
                {
                    break (at, semi + 1, decoded.len(), Kind::Entity);
                }
                if here.starts_with(c) {
                    break (at, c.len_utf8(), c.len_utf8(), Kind::Same);
                }
                at = skip_one(src, at, range.end, b" \t>")?;
            };
            leaf.push(&value[i..i + drawn_len], from..from + src_len, kind);
            at = from + src_len;
            i += drawn_len;
        }
        Some(())
    }

    fn code_leaf(&self, value: &str, range: Range<usize>) -> Option<Leaf> {
        let raw = &self.source[range.clone()];
        let fenced = ["```", "~~~", "$$"].iter().any(|f| raw.trim_start().starts_with(f));
        let body = if fenced { raw.find('\n').map_or(raw.len(), |n| n + 1) } else { 0 };
        let mut leaf = Leaf { at: range.start + body, ..Leaf::default() };
        self.code(value, range.start + body..range.end, &mut leaf)?;
        Some(leaf)
    }

    /// Aligns code with its source, skipping the indentation and `>` the parser strips.
    fn code(&self, value: &str, range: Range<usize>, leaf: &mut Leaf) -> Option<()> {
        let mut at = range.start;
        for c in value.chars() {
            while !self.source[at..range.end].starts_with(c) {
                at = skip_one(self.source, at, range.end, b" \t>\r")?;
            }
            let len = c.len_utf8();
            leaf.push(&self.source[at..at + len], at..at + len, Kind::Same);
            at += len;
        }
        Some(())
    }
}

fn skip(src: &str, mut at: usize, end: usize, set: &[u8]) -> usize {
    while at < end && set.contains(&src.as_bytes()[at]) {
        at += 1;
    }
    at
}

fn skip_one(src: &str, at: usize, end: usize, set: &[u8]) -> Option<usize> {
    (at < end && set.contains(&src.as_bytes()[at])).then_some(at + 1)
}

fn line_ending(s: &str) -> Option<usize> {
    if s.starts_with("\r\n") {
        Some(2)
    } else if s.starts_with(['\n', '\r']) {
        Some(1)
    } else {
        None
    }
}

fn definitions(node: &Node, out: &mut HashSet<String>) {
    if let Node::Definition(d) = node {
        out.insert(d.identifier.clone());
    }
    for child in node.children().into_iter().flatten() {
        definitions(child, out);
    }
}

/// One undo step: at `at`, `removed` was replaced by `inserted`.
#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    removed: String,
    inserted: String,
    before: Selection,
    after: Selection,
}

impl Edit {
    /// Folds `next` into this step when it continues it: typing on within a word, a run of
    /// Backspace or Delete, or replacing what this step just inserted, as marked text does.
    fn absorb(&mut self, next: &Edit) -> bool {
        let end = self.at + self.inserted.len();
        let starts_word = next.inserted.starts_with(char::is_whitespace) && !self.inserted.ends_with(char::is_whitespace);
        if next.removed.is_empty() && next.at == end && !self.inserted.is_empty() && !starts_word && !next.inserted.contains('\n') {
            self.inserted.push_str(&next.inserted);
        } else if !self.inserted.is_empty() && next.at == self.at && next.removed == self.inserted {
            self.inserted.clone_from(&next.inserted);
        } else if self.inserted.is_empty() && next.inserted.is_empty() && next.at + next.removed.len() == self.at {
            self.removed.insert_str(0, &next.removed);
            self.at = next.at;
        } else if self.inserted.is_empty() && next.inserted.is_empty() && next.at == self.at {
            self.removed.push_str(&next.removed);
        } else {
            return false;
        }
        self.after = next.after;
        true
    }
}

#[derive(Default)]
struct History {
    done: Vec<Edit>,
    undone: Vec<Edit>,
    /// Whether the next edit may fold into the last one.
    open: bool,
}

pub struct Draft {
    source: String,
    selection: Selection,
    blocks: Vec<Block>,
    definitions: Vec<Range<usize>>,
    /// Whether any link reference definition or footnote is anywhere in the source, so a
    /// reference in one block may resolve against another.
    references: bool,
    history: History,
    claim: Claim,
}

impl Draft {
    pub fn new(source: impl Into<String>, claim: Claim) -> Self {
        let mut draft = Draft { source: source.into(), selection: Selection::default(), blocks: vec![], definitions: vec![], references: false, history: History::default(), claim };
        draft.reparse();
        draft
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The top-level link reference definitions, which every block's references resolve against.
    pub fn definitions(&self) -> impl Iterator<Item = &str> {
        self.definitions.iter().map(|r| &self.source[r.clone()])
    }

    /// The block the byte belongs to: the last one starting at or before it.
    pub fn block_at(&self, at: usize) -> Option<usize> {
        self.blocks.iter().rposition(|b| b.range.start <= at).or((!self.blocks.is_empty()).then_some(0))
    }

    fn floor(&self, at: usize) -> usize {
        let mut at = at.min(self.source.len());
        while !self.source.is_char_boundary(at) {
            at -= 1;
        }
        at
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.selection = Selection { anchor: self.floor(anchor), head: self.floor(head) };
    }

    /// Replaces `range` with `text` and leaves the caret after it, as one undo step or as part
    /// of the step before it (see `Edit::absorb`).
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let range = self.floor(range.start)..self.floor(range.end.max(range.start));
        let removed = self.source[range.clone()].to_string();
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let edit = Edit {
            at: range.start,
            removed,
            inserted: text.to_string(),
            before: self.selection,
            after: Selection::caret(range.start + text.len()),
        };
        let removed = range.len();
        self.source.replace_range(range.clone(), text);
        self.selection = edit.after;
        let h = &mut self.history;
        h.undone.clear();
        let folded = h.open && h.done.last_mut().is_some_and(|last| last.absorb(&edit));
        if !folded {
            h.done.push(edit);
        }
        h.open = true;
        self.reparse_edit(range.start, removed, text.len());
    }

    /// Ends the current undo step, so the next edit starts a new one.
    pub fn seal(&mut self) {
        self.history.open = false;
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.history.done.pop() else { return false };
        self.source.replace_range(edit.at..edit.at + edit.inserted.len(), &edit.removed);
        self.selection = edit.before;
        self.history.open = false;
        self.reparse_edit(edit.at, edit.inserted.len(), edit.removed.len());
        self.history.undone.push(edit);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.history.undone.pop() else { return false };
        self.source.replace_range(edit.at..edit.at + edit.removed.len(), &edit.inserted);
        self.selection = edit.after;
        self.history.open = false;
        self.reparse_edit(edit.at, edit.removed.len(), edit.inserted.len());
        self.history.done.push(edit);
        true
    }

    /// Draws formulas with `claim` from now on.
    pub fn set_claim(&mut self, claim: Claim) {
        self.claim = claim;
        self.reparse();
    }

    pub fn prev_grapheme(&self, at: usize) -> usize {
        let mut cursor = GraphemeCursor::new(self.floor(at), self.source.len(), true);
        cursor.prev_boundary(&self.source, 0).ok().flatten().unwrap_or(0)
    }

    pub fn next_grapheme(&self, at: usize) -> usize {
        let mut cursor = GraphemeCursor::new(self.floor(at), self.source.len(), true);
        cursor.next_boundary(&self.source, 0).ok().flatten().unwrap_or(self.source.len())
    }

    /// The start of the word before `at`.
    pub fn word_left(&self, at: usize) -> usize {
        self.source[..self.floor(at)]
            .split_word_bound_indices()
            .rfind(|(_, w)| !w.trim().is_empty())
            .map_or(0, |(i, _)| i)
    }

    /// The end of the word after `at`.
    pub fn word_right(&self, at: usize) -> usize {
        let at = self.floor(at);
        self.source[at..]
            .split_word_bound_indices()
            .find(|(_, w)| !w.trim().is_empty())
            .map_or(self.source.len(), |(i, w)| at + i + w.len())
    }

    /// The line holding `at`, without its line ending.
    pub fn line_at(&self, at: usize) -> Range<usize> {
        let at = self.floor(at);
        let start = self.source[..at].rfind('\n').map_or(0, |i| i + 1);
        start..self.source[at..].find('\n').map_or(self.source.len(), |i| at + i)
    }

    /// The word a double click at `at` selects: the one under it, else the one ending there,
    /// else nothing.
    pub fn word_at(&self, at: usize) -> Range<usize> {
        let at = self.floor(at);
        let line = self.line_at(at);
        let mut words = self.source[line.clone()]
            .split_word_bound_indices()
            .filter(|(_, w)| !w.trim().is_empty())
            .map(|(i, w)| line.start + i..line.start + i + w.len());
        let mut ending = None;
        words
            .find(|w| {
                if w.end == at {
                    ending = Some(w.clone());
                }
                w.start <= at && at < w.end
            })
            .or(ending)
            .unwrap_or(at..at)
    }

    /// The source's length in UTF-16 code units, the unit the platform's text input counts in.
    pub fn len_utf16(&self) -> usize {
        self.source.encode_utf16().count()
    }

    /// A byte range of the source in UTF-16 code units.
    pub fn to_utf16(&self, range: Range<usize>) -> Range<usize> {
        let (from, to) = (self.floor(range.start), self.floor(range.end.max(range.start)));
        let start = self.source[..from].encode_utf16().count();
        start..start + self.source[from..to].encode_utf16().count()
    }

    /// A range in UTF-16 code units as bytes of the source. A unit past the end is the end, and
    /// one inside a surrogate pair is the start of its character, so no edit splits a character.
    pub fn from_utf16(&self, range: Range<usize>) -> Range<usize> {
        let at = |units: usize| {
            let mut n = 0;
            for (i, c) in self.source.char_indices() {
                n += c.len_utf16();
                if n > units {
                    return i;
                }
            }
            self.source.len()
        };
        at(range.start)..at(range.end.max(range.start))
    }

    /// Brings the blocks up to date after `removed` bytes at `at` became `inserted` bytes.
    /// Parsing the whole draft on every keystroke is most of a keystroke's time in a long one,
    /// so only the blocks around the edit are parsed again, when that is sure to give what a
    /// whole parse would.
    fn reparse_edit(&mut self, at: usize, removed: usize, inserted: usize) {
        if !self.reparse_around(at, removed, inserted) {
            self.reparse();
        }
    }

    /// Parses again the blocks the edit touched, with one untouched block on each side, and
    /// moves the blocks after them. Declines, leaving the blocks as they were, unless a blank
    /// line closes the region on each side, the last block in it cannot run on past a blank
    /// line, and no block can refer to another.
    fn reparse_around(&mut self, at: usize, removed: usize, inserted: usize) -> bool {
        let n = self.blocks.len();
        if n == 0 || self.references || self.source.contains("[^") {
            return false;
        }
        let old_end = at + removed;
        let delta = inserted as isize - removed as isize;
        let first = self.blocks.iter().position(|b| b.range.end >= at).unwrap_or(n);
        let last = self.blocks.iter().rposition(|b| b.range.start <= old_end).unwrap_or(0);
        let lo = first.min(last).saturating_sub(1);
        let hi = (first.max(last) + 1).min(n - 1);
        // From the start of the line, so a block keeps its indentation.
        let line = |at: usize| self.source[..at].rfind('\n').map_or(0, |i| i + 1);
        let start = if lo == 0 { 0 } else { line(self.blocks[lo].range.start) };
        let end = if hi == n - 1 { self.source.len() } else { self.blocks[hi].range.end.wrapping_add_signed(delta) };
        let blank = |gap: &str| gap.trim().is_empty() && gap.matches('\n').count() >= 2;
        if lo > 0 && !blank(&self.source[self.blocks[lo - 1].range.end..start]) {
            return false;
        }
        let next = self.blocks.get(hi + 1).map(|b| b.range.start.wrapping_add_signed(delta));
        if next.is_some_and(|next| !blank(&self.source[end..next])) {
            return false;
        }
        let region = &self.source[start..end];
        let Ok(Node::Root(root)) = markdown::to_mdast(region, &options(true)) else { return false };
        let mut defs = HashSet::new();
        for child in &root.children {
            definitions(child, &mut defs);
        }
        if !defs.is_empty() || region.contains("]:") {
            return false;
        }
        let closed = |n: &Node| matches!(n, Node::Paragraph(_) | Node::Heading(_) | Node::ThematicBreak(_) | Node::Table(_));
        if next.is_some() && !root.children.last().is_some_and(closed) {
            return false;
        }
        let mapper = Mapper { source: region, claim: &self.claim, definitions: &defs };
        let mut fresh = vec![];
        for child in &root.children {
            let Some(range) = span(child, start) else { continue };
            let mut map = mapper.block(child);
            if let Some(map) = map.as_mut() {
                map.shift(start as isize);
            }
            fresh.push(Block { range, map });
        }
        for block in &mut self.blocks[hi + 1..] {
            block.range = block.range.start.wrapping_add_signed(delta)..block.range.end.wrapping_add_signed(delta);
            if let Some(map) = block.map.as_mut() {
                map.shift(delta);
            }
        }
        self.blocks.splice(lo..=hi, fresh);
        true
    }

    fn reparse(&mut self) {
        self.blocks.clear();
        self.definitions.clear();
        self.references = false;
        let Ok(Node::Root(root)) = markdown::to_mdast(&self.source, &options(true)) else { return };
        let mut defs = HashSet::new();
        for child in &root.children {
            definitions(child, &mut defs);
        }
        self.references = !defs.is_empty() || root.children.iter().any(|c| matches!(c, Node::FootnoteDefinition(_)));
        let mapper = Mapper { source: &self.source, claim: &self.claim, definitions: &defs };
        for child in &root.children {
            let Some(range) = span(child, 0) else { continue };
            if matches!(child, Node::Definition(_)) {
                self.definitions.push(range.clone());
            }
            self.blocks.push(Block { range, map: mapper.block(child) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Inline Markdown and the text GPUI Kit draws for it. U+FFFC is an image or a formula.
    const INLINE: &[(&str, &str)] = &[
        ("ferry", "ferry"),
        ("café", "café"),
        ("東京", "東京"),
        ("😀", "😀"),
        (" ", " "),
        (", ", ", "),
        ("**bold**", "bold"),
        ("*it*", "it"),
        ("_it_", "it"),
        ("~~gone~~", "gone"),
        ("`co de`", "co de"),
        ("`` a`b ``", "a`b"),
        ("\\*", "*"),
        ("\\\\", "\\"),
        ("\\[", "["),
        ("&amp;", "&"),
        ("&copy;", "©"),
        ("&#35;", "#"),
        ("&#x41;", "A"),
        ("<https://a.example/x>", "https://a.example/x"),
        ("https://b.example", "https://b.example"),
        ("www.c.example", "www.c.example"),
        ("[link](https://d.example)", "link"),
        ("[**bold** link](u)", "bold link"),
        ("![alt](p.png)", "\u{FFFC}"),
        ("$x^2$", "\u{FFFC}"),
        ("$5 and $10", "$5 and $10"),
        ("$\\frac{1}{$", "$\\frac{1}{$"),
    ];

    fn claim() -> Claim {
        Arc::new(|tex: &str, _| !tex.contains("frac{1}{"))
    }

    fn inline() -> impl Strategy<Value = (String, String)> {
        prop::collection::vec(prop::sample::select(INLINE), 1..8).prop_map(|parts| {
            let md: Vec<&str> = parts.iter().map(|p| p.0).collect();
            let drawn: Vec<&str> = parts.iter().map(|p| p.1).collect();
            (format!("w {} w", md.join(" ")), format!("w {} w", drawn.join(" ")))
        })
    }

    /// A block of Markdown and the text of each leaf it draws.
    fn block() -> impl Strategy<Value = (String, Vec<String>)> {
        prop_oneof![
            inline().prop_map(|(m, d)| (m, vec![d])),
            (inline(), inline()).prop_map(|((a, x), (b, y))| (format!("{a}\n{b}"), vec![format!("{x} {y}")])),
            (inline(), inline()).prop_map(|((a, x), (b, y))| (format!("{a}  \n{b}"), vec![format!("{x}\n{y}")])),
            (1..=6usize, inline()).prop_map(|(n, (m, d))| (format!("{} {m}", "#".repeat(n)), vec![d])),
            (inline(), inline()).prop_map(|((a, x), (b, y))| (format!("- {a}\n- {b}"), vec![x, y])),
            (inline(), inline()).prop_map(|((a, x), (b, y))| (format!("> {a}\n> {b}"), vec![format!("{x} {y}")])),
            (inline(), inline()).prop_map(|((a, x), (b, y))| (format!("- {a}\n  {b}"), vec![format!("{x} {y}")])),
            Just(("```rust\nfn main() {}\n  x\n```".to_string(), vec!["fn main() {}\n  x".to_string()])),
            Just(("    indented\n    code".to_string(), vec!["indented\ncode".to_string()])),
            Just(("| a \\| b | c |\n|---|---|\n| &amp; | `x` |".to_string(), ["a | b", "c", "&", "x"].map(String::from).to_vec())),
            Just(("$$\n\\frac{1}{\n$$".to_string(), vec!["\\frac{1}{".to_string()])),
            Just(("$$\nx^2\n$$".to_string(), vec![])),
            Just(("---".to_string(), vec![])),
        ]
    }

    /// Blocks with a paragraph between each two, so that no two lists merge.
    fn document() -> impl Strategy<Value = (String, Vec<Vec<String>>)> {
        prop::collection::vec(block(), 1..6).prop_map(|blocks| {
            let source = blocks.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>().join("\n\nsep\n\n");
            let mut drawn = vec![];
            for (i, (_, d)) in blocks.into_iter().enumerate() {
                if i > 0 {
                    drawn.push(vec!["sep".to_string()]);
                }
                drawn.push(d);
            }
            (source, drawn)
        })
    }

    #[derive(Clone, Debug)]
    enum Op {
        Replace(usize, usize, String),
        Type(String),
        Backspace,
        Delete,
        Seal,
    }

    fn edits() -> impl Strategy<Value = Vec<Op>> {
        let text = prop::collection::vec(prop::sample::select(INLINE), 0..3)
            .prop_map(|parts| parts.iter().map(|(m, _)| *m).collect::<String>());
        let piece = prop_oneof![
            text,
            prop::sample::select(vec![
                "\n", "\n\n", "# ", "- ", "`", "*", "$", "\\", "&", "```", "$$\n", "> ", "    ", "1. ", "|---|\n", "\n===", "\n[r]: /x", "[^1]",
                "<div>\n", "\n\n- b"
            ])
            .prop_map(String::from)
        ];
        let op = prop_oneof![
            (any::<usize>(), 0..12usize, piece).prop_map(|(at, len, text)| Op::Replace(at, len, text)),
            prop::sample::select(vec!["a", "b", " ", "é", "😀"]).prop_map(|t| Op::Type(t.to_string())),
            Just(Op::Backspace),
            Just(Op::Delete),
            Just(Op::Seal),
        ];
        prop::collection::vec(op, 0..24)
    }

    fn boundary(s: &str, i: usize) -> usize {
        let mut i = i % (s.len() + 1);
        while !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// Every segment's source is exactly what draws its text, and every caret stop maps to
    /// source and back to itself.
    fn assert_exact(source: &str, leaf: &Leaf) {
        let mut drawn = 0;
        let mut last_source = 0;
        for s in &leaf.segments {
            assert_eq!(s.drawn.start, drawn, "segments cover the drawn text in order: {leaf:?}");
            assert!(s.source.start >= last_source, "segments move forward in the source: {leaf:?}");
            drawn = s.drawn.end;
            last_source = s.source.end;
            let (d, src) = (&leaf.text[s.drawn.clone()], &source[s.source.clone()]);
            let ok = match s.kind {
                Kind::Same => d == src,
                Kind::Escape => src.strip_prefix('\\') == Some(d),
                Kind::Entity => decode_entity(src).as_deref() == Some(d),
                Kind::LineEnding => d == " " && matches!(src, "\n" | "\r\n" | "\r"),
                Kind::Break => d == "\n" && src.ends_with('\n'),
                Kind::Atom => d == "\u{FFFC}",
            };
            assert!(ok, "{:?} segment draws {d:?} from {src:?}", s.kind);
            let stops: Vec<usize> = match s.kind {
                Kind::Same => d.char_indices().map(|(i, _)| s.drawn.start + i).collect(),
                _ => vec![s.drawn.start],
            };
            for stop in stops {
                assert_eq!(leaf.to_drawn(leaf.to_source(stop)), stop, "caret stop {stop} round-trips in {leaf:?}");
            }
        }
        assert_eq!(drawn, leaf.text.len(), "segments cover the whole drawn text: {leaf:?}");
        assert_eq!(leaf.to_drawn(leaf.to_source(drawn)), drawn, "the end round-trips");
    }

    /// Bytes and UTF-16 code units convert both ways: a range on character boundaries
    /// round-trips, and a unit inside a surrogate pair lands on the start of its character.
    /// The word at a byte holds it and no white space.
    fn assert_utf16(draft: &Draft, a: usize, b: usize) {
        let source = draft.source();
        let units: Vec<u16> = source.encode_utf16().collect();
        assert_eq!(draft.len_utf16(), units.len());
        let (a, b) = (boundary(source, a), boundary(source, b));
        let bytes = a.min(b)..a.max(b);
        let range = draft.to_utf16(bytes.clone());
        assert_eq!(String::from_utf16_lossy(&units[range.clone()]), source[bytes.clone()], "{bytes:?} is {range:?} in UTF-16");
        assert_eq!(draft.from_utf16(range), bytes);
        let (word, line) = (draft.word_at(a), draft.line_at(a));
        assert!(line.start <= word.start && word.start <= a && a <= word.end && word.end <= line.end, "the word at {a} is {word:?} in line {line:?}");
        assert!(!source[word.clone()].contains(char::is_whitespace), "{:?} is one word", &source[word]);
        for unit in 0..=units.len() + 1 {
            let at = draft.from_utf16(unit..unit).start;
            assert!(source.is_char_boundary(at));
            let back = draft.to_utf16(at..at).start;
            let low_surrogate = units.get(unit).is_some_and(|u| (0xDC00..0xE000).contains(u));
            assert_eq!(back, if low_surrogate { unit - 1 } else { unit.min(units.len()) }, "unit {unit} of {source:?}");
        }
    }

    /// The blocks an edit leaves are the ones a whole parse of the source gives.
    fn assert_reparsed(draft: &Draft) {
        let whole = Draft::new(draft.source(), claim());
        assert_eq!(draft.blocks(), whole.blocks(), "blocks after an edit of {:?}", draft.source());
        assert_eq!(draft.definitions().collect::<Vec<_>>(), whole.definitions().collect::<Vec<_>>());
    }

    fn assert_maps(draft: &Draft) {
        for block in draft.blocks() {
            if let Some(map) = &block.map {
                for leaf in &map.leaves {
                    assert_exact(draft.source(), leaf);
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn draft_maps_drawn_text_and_undo_restores((source, expected) in document(), edits in edits(), (a, b) in any::<(usize, usize)>()) {
            let mut draft = Draft::new(source.clone(), claim());
            let drawn: Vec<Vec<String>> = draft
                .blocks()
                .iter()
                .map(|b| b.map.as_ref().expect("every generated block maps").leaves.iter().map(|l| l.text.clone()).collect())
                .collect();
            prop_assert_eq!(&drawn, &expected);
            assert_maps(&draft);
            assert_utf16(&draft, a, b);

            let mut versions = vec![source.clone()];
            for op in edits {
                let caret = draft.selection().head;
                match op {
                    Op::Replace(at, len, text) => {
                        let start = boundary(draft.source(), at);
                        let end = boundary(draft.source(), start + len);
                        draft.seal();
                        draft.replace(start..end.max(start), &text);
                    }
                    Op::Type(text) => draft.replace(caret..caret, &text),
                    Op::Backspace => draft.replace(draft.prev_grapheme(caret)..caret, ""),
                    Op::Delete => draft.replace(caret..draft.next_grapheme(caret), ""),
                    Op::Seal => draft.seal(),
                }
                versions.push(draft.source().to_string());
                assert_reparsed(&draft);
                assert_maps(&draft);
                assert_utf16(&draft, a, b);
            }
            let last = draft.source().to_string();
            while draft.undo() {
                prop_assert!(versions.contains(&draft.source().to_string()), "undo passes only through versions that existed");
                assert_reparsed(&draft);
            }
            prop_assert_eq!(draft.source(), source.as_str());
            while draft.redo() {
                assert_reparsed(&draft);
            }
            prop_assert_eq!(draft.source(), last.as_str());
        }
    }
}

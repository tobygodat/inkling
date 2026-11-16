//! A chapter's Markdown as the Reading screen sets it: parsed once, with the parser GPUI Kit draws
//! the page with, into a flat list of blocks. A quote's paragraphs and a list's items are blocks of
//! their own, so a page can end between any two of them, and each block knows where it starts in
//! the chapter's Markdown.
//!
//! A selection or a highlight is a range of `Laid::text`: every block's text, one after another
//! with a newline between. `Laid::map` takes each byte of it back to the Markdown byte that draws
//! it, so a highlight is kept as a place in the chapter's Markdown (ADR 0006).

use gpui_kit::base::text::editor_host::{self as eh, BlockNode, Paragraph};
use gpui_kit::base::text::{MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::*;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

/// How a run of a block's text is set, or what it does when tapped.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mark {
    Em,
    Strong,
    /// A link and where it goes: `#<chapter>:<byte>` inside the book, anything else outside it.
    Link(String),
    /// A footnote marker, by its label.
    Note(String),
}

/// A block's text and how each part of it is set.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Text {
    pub text: SharedString,
    pub marks: Vec<(Range<usize>, Mark)>,
}

impl Text {
    /// What a tap on byte `at` of the text reaches: a footnote marker or a link.
    pub fn target(&self, at: usize) -> Option<&Mark> {
        self.marks.iter().filter(|(r, m)| r.contains(&at) && matches!(m, Mark::Link(_) | Mark::Note(_))).map(|(_, m)| m).next_back()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    /// The chapter's own heading.
    Title,
    Heading(u8),
    Paragraph,
    /// A picture inside the column, with its caption.
    Image { src: String, alt: String, caption: String },
    /// A scene break.
    Rule,
}

/// One block as it is laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    pub kind: Kind,
    pub text: Text,
    /// How many quotes it sits in.
    pub quote: usize,
    /// How many list items it sits in, and the marker it opens with when it starts one.
    pub indent: usize,
    pub marker: Option<String>,
    /// Where it starts in the chapter's Markdown, in bytes.
    pub source: usize,
    /// Where its text starts in `Laid::text`.
    pub at: usize,
}

/// A chapter, ready to lay out.
#[derive(Default)]
pub(crate) struct Laid {
    pub blocks: Vec<Block>,
    /// Every block's text, each followed by a newline.
    pub text: String,
    /// The Markdown byte that draws each byte of `text`, and one more for its end.
    pub map: Vec<usize>,
    /// Each footnote's text, by its label.
    pub notes: HashMap<String, Text>,
}

const NOTE: &str = "reading-note";
const NOTE_TEXT: &str = "reading-note-text";

/// Claims footnote markers, which GPUI Kit would otherwise set as "[1]" text.
struct NoteMarker;

impl MarkdownPlugin for NoteMarker {
    fn name(&self) -> &str {
        NOTE
    }

    fn parse(&self, node: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::FootnoteReference(note) = node else { return None };
        Some(MarkdownNode::new(NOTE, note.identifier.clone()).text(note.identifier.clone()))
    }
}

/// Claims footnote definitions, keeping each note's own Markdown to parse as a paragraph.
struct NoteText;

impl MarkdownPlugin for NoteText {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        NOTE_TEXT
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::FootnoteDefinition(def) = node else { return None };
        let source = cx.node_source(node)?;
        let body = source.split_once("]:").map(|(_, b)| b.trim()).unwrap_or_default();
        Some(MarkdownNode::new(NOTE_TEXT, (def.identifier.clone(), body.to_string())))
    }
}

fn extensions() -> Arc<MarkdownExtensions> {
    Arc::new(MarkdownExtensions::default().plugin(NoteMarker).plugin(NoteText))
}

/// Parses a chapter's Markdown into blocks.
pub(crate) fn lay(markdown: &str) -> Laid {
    let ext = extensions();
    let nodes = eh::parse_markdown(markdown, ext.clone()).unwrap_or_default();
    let mut laid = Laid::default();
    let mut maps = vec![];
    let mut walk = Walk { markdown, out: &mut laid.blocks, maps: &mut maps, notes: vec![], quote: 0, indent: 0, marker: None };
    for (i, node) in nodes.iter().enumerate() {
        walk.block(node, i == 0);
    }
    let notes = std::mem::take(&mut walk.notes);
    for (label, body) in notes {
        let text = eh::parse_markdown(&body, ext.clone())
            .unwrap_or_default()
            .iter()
            .find_map(|n| if let BlockNode::Paragraph(p) = n { Some(text(p).0) } else { None })
            .unwrap_or_default();
        laid.notes.insert(label, text);
    }
    for (block, map) in laid.blocks.iter_mut().zip(maps) {
        block.at = laid.text.len();
        laid.text.push_str(&block.text.text);
        laid.map.extend_from_slice(&map);
        laid.text.push('\n');
    }
    laid.map.push(markdown.len());
    laid
}

struct Walk<'a> {
    markdown: &'a str,
    out: &'a mut Vec<Block>,
    /// Each block's map from its text to the Markdown, as `Laid::map`, with an entry for its end.
    maps: &'a mut Vec<Vec<usize>>,
    notes: Vec<(String, String)>,
    quote: usize,
    indent: usize,
    /// The marker of the list item whose first block is next.
    marker: Option<String>,
}

impl Walk<'_> {
    fn push(&mut self, kind: Kind, text: Text, span: Range<usize>) {
        let marker = self.marker.take();
        self.maps.push(align(&text, self.markdown, span.clone()));
        self.out.push(Block { kind, text, quote: self.quote, indent: self.indent, marker, source: span.start, at: 0 });
    }

    fn block(&mut self, node: &BlockNode, first: bool) {
        let span = node.span().map_or(0..0, |s| s.start..s.end.min(self.markdown.len()));
        let source = span.start;
        match node {
            BlockNode::Heading { level, children, .. } => {
                let kind = if first && *level == 1 { Kind::Title } else { Kind::Heading(*level) };
                self.push(kind, text(children).0, span);
            }
            BlockNode::Paragraph(p) => {
                let (body, images) = text(p);
                if !body.text.trim().is_empty() {
                    self.push(Kind::Paragraph, body, span.clone());
                }
                for (src, alt, caption) in images {
                    self.push(Kind::Image { src, alt, caption }, Text::default(), source..source);
                }
            }
            BlockNode::Blockquote { children, .. } => {
                self.quote += 1;
                for c in children {
                    self.block(c, false);
                }
                self.quote -= 1;
            }
            BlockNode::List { children, ordered, start, .. } => {
                let mut n = start.unwrap_or(1);
                for item in children {
                    let BlockNode::ListItem { children, .. } = item else { continue };
                    self.marker = Some(if *ordered { format!("{n}.") } else { "•".to_string() });
                    n += 1;
                    self.indent += 1;
                    for c in children {
                        self.block(c, false);
                    }
                    self.indent -= 1;
                    self.marker = None;
                }
            }
            BlockNode::HorizontalRule { .. } => self.push(Kind::Rule, Text::default(), source..source),
            BlockNode::Custom { node, .. } if node.name() == NOTE_TEXT => {
                if let Some((label, body)) = node.data::<(String, String)>() {
                    self.notes.push((label.clone(), body.clone()));
                }
            }
            BlockNode::Root { children, .. } => {
                for c in children {
                    self.block(c, false);
                }
            }
            BlockNode::CodeBlock(code) => self.push(Kind::Paragraph, Text { text: code.code(), marks: vec![] }, span),
            _ => {}
        }
    }
}

/// A paragraph's text with its marks, and the images it holds as (path, alt, caption).
fn text(p: &Paragraph) -> (Text, Vec<(String, String, String)>) {
    let mut out = String::new();
    let mut marks: Vec<(Range<usize>, Mark)> = vec![];
    let mut images = vec![];
    for child in p.children() {
        if let Some(image) = child.as_image() {
            let alt = image.alt.clone().unwrap_or_default().to_string();
            let caption = image.title.clone().unwrap_or_default().to_string();
            images.push((image.url.to_string(), alt, caption));
            continue;
        }
        if let Some(custom) = child.as_custom() {
            if custom.name() == NOTE {
                let label = custom.as_text().to_string();
                let start = out.len();
                out.push_str(&superscript(&label));
                marks.push((start..out.len(), Mark::Note(label)));
            }
            continue;
        }
        let start = out.len();
        out.push_str(child.text());
        for (range, mark) in child.mark_runs() {
            let r = start + range.start..start + range.end;
            if mark.italic {
                marks.push((r.clone(), Mark::Em));
            }
            if mark.bold {
                marks.push((r.clone(), Mark::Strong));
            }
            if let Some(link) = &mark.link {
                marks.push((r, Mark::Link(link.url.to_string())));
            }
        }
    }
    (Text { text: out.into(), marks }, images)
}

/// Which Markdown byte in `span` draws each byte of a block's text, with one more entry for its
/// end. The text is the Markdown less its syntax, so each character is the next one like it in
/// the Markdown, past escapes, emphasis, link targets and pictures; a footnote marker is its
/// `[^label]`. A character it can't find maps to where the search started, so the map only ever
/// moves forward.
fn align(text: &Text, markdown: &str, span: Range<usize>) -> Vec<usize> {
    const REACH: usize = 400;
    let md = markdown.as_bytes();
    let end = span.end.min(markdown.len());
    let mut at = span.start.min(end);
    let mut map = Vec::with_capacity(text.text.len() + 1);
    let notes: Vec<Range<usize>> = text.marks.iter().filter(|(_, m)| matches!(m, Mark::Note(_))).map(|(r, _)| r.clone()).collect();
    let skip_to = |from: usize, needle: u8| md[from.min(end)..end].iter().position(|b| *b == needle).map(|i| from + i + 1);
    let mut chars = text.text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if let Some(note) = notes.iter().find(|r| r.start == i) {
            let found = markdown[at..end].find("[^").map(|n| at + n);
            let p = found.unwrap_or(at);
            map.extend(std::iter::repeat_n(p, note.end - i));
            at = found.and_then(|p| skip_to(p, b']')).unwrap_or(at);
            while chars.peek().is_some_and(|(j, _)| *j < note.end) {
                chars.next();
            }
            continue;
        }
        let mut q = at;
        let mut found = None;
        while q < end && q < at + REACH {
            let rest = &markdown[q..end];
            if let Some(escaped) = rest.strip_prefix('\\') {
                if escaped.starts_with(ch) {
                    found = Some(q + 1);
                    break;
                }
                q += 1 + escaped.chars().next().map_or(0, char::len_utf8);
                continue;
            }
            if rest.starts_with("](") || rest.starts_with("![") {
                q = rest.find(')').map_or(end, |n| q + n + 1);
                continue;
            }
            let c = rest.chars().next().unwrap_or(' ');
            if c == ch || (ch.is_whitespace() && c.is_whitespace()) {
                found = Some(q);
                break;
            }
            q += c.len_utf8();
        }
        let p = found.unwrap_or(at);
        map.extend((0..ch.len_utf8()).map(|k| p + k));
        if let Some(p) = found {
            at = p + markdown[p..].chars().next().map_or(1, char::len_utf8);
        }
    }
    map.push(at);
    map
}

/// A footnote marker as a book prints it, raised: "12" becomes "¹²".
fn superscript(label: &str) -> String {
    label
        .chars()
        .map(|c| match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            c => c,
        })
        .collect()
}

/// Where a link inside the book goes: a chapter and a byte offset in it.
pub(crate) fn inside(href: &str) -> Option<(usize, usize)> {
    let (chapter, at) = href.strip_prefix('#')?.split_once(':')?;
    Some((chapter.parse().ok()?, at.parse().ok()?))
}

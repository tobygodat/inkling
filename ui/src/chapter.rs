//! A chapter's Markdown as the Reading screen sets it: parsed once, with the parser GPUI Kit draws
//! the page with, into a flat list of blocks. A quote's paragraphs and a list's items are blocks of
//! their own, so a page can end between any two of them, and each block knows where it starts in
//! the chapter's Markdown.

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
}

/// A chapter, ready to lay out.
#[derive(Default)]
pub(crate) struct Laid {
    pub blocks: Vec<Block>,
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
    let mut walk = Walk { out: &mut laid.blocks, notes: vec![], quote: 0, indent: 0, marker: None };
    for (i, node) in nodes.iter().enumerate() {
        walk.block(node, i == 0);
    }
    for (label, body) in std::mem::take(&mut walk.notes) {
        let text = eh::parse_markdown(&body, ext.clone())
            .unwrap_or_default()
            .iter()
            .find_map(|n| if let BlockNode::Paragraph(p) = n { Some(text(p).0) } else { None })
            .unwrap_or_default();
        laid.notes.insert(label, text);
    }
    laid
}

struct Walk<'a> {
    out: &'a mut Vec<Block>,
    notes: Vec<(String, String)>,
    quote: usize,
    indent: usize,
    /// The marker of the list item whose first block is next.
    marker: Option<String>,
}

impl Walk<'_> {
    fn push(&mut self, kind: Kind, text: Text, source: usize) {
        let marker = self.marker.take();
        self.out.push(Block { kind, text, quote: self.quote, indent: self.indent, marker, source });
    }

    fn block(&mut self, node: &BlockNode, first: bool) {
        let source = node.span().map_or(0, |s| s.start);
        match node {
            BlockNode::Heading { level, children, .. } => {
                let kind = if first && *level == 1 { Kind::Title } else { Kind::Heading(*level) };
                self.push(kind, text(children).0, source);
            }
            BlockNode::Paragraph(p) => {
                let (body, images) = text(p);
                if !body.text.trim().is_empty() {
                    self.push(Kind::Paragraph, body, source);
                }
                for (src, alt, caption) in images {
                    self.push(Kind::Image { src, alt, caption }, Text::default(), source);
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
            BlockNode::HorizontalRule { .. } => self.push(Kind::Rule, Text::default(), source),
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
            BlockNode::CodeBlock(code) => self.push(Kind::Paragraph, Text { text: code.code(), marks: vec![] }, source),
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

//! Writing a book's text as Markdown. Every character the book printed comes back out of a
//! Markdown parser as the same character: whatever would read as syntax is escaped, and emphasis
//! is only written where it will parse as emphasis.

/// How a run of text is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub em: bool,
    pub strong: bool,
}

impl Style {
    fn delimiter(self) -> &'static str {
        match (self.em, self.strong) {
            (true, true) => "***",
            (false, true) => "**",
            (true, false) => "*",
            (false, false) => "",
        }
    }
}

/// A paragraph's content as the book set it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Inline {
    Text(String, Style),
    /// A `<br>`.
    Break,
    /// A link and its text. The destination is written as given.
    Link(String, Vec<Inline>),
    /// A footnote marker, by its label.
    Note(String),
}

impl Inline {
    pub fn text(s: impl Into<String>) -> Inline {
        Inline::Text(s.into(), Style::default())
    }

    /// The text a reader sees, with breaks as newlines.
    pub fn plain(inlines: &[Inline]) -> String {
        let mut out = String::new();
        for i in inlines {
            match i {
                Inline::Text(t, _) => out.push_str(t),
                Inline::Break => out.push('\n'),
                Inline::Link(_, children) => out.push_str(&Inline::plain(children)),
                Inline::Note(_) => {}
            }
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tok {
    Char(char, Style, Option<usize>),
    Break,
    Note(usize),
}

/// Writes inline content as one Markdown paragraph's text. Whitespace collapses as HTML's does,
/// and each line starts with no space.
pub(crate) fn inline(inlines: &[Inline]) -> String {
    let mut links: Vec<&str> = vec![];
    let mut notes: Vec<&str> = vec![];
    let mut toks = vec![];
    flatten(inlines, Style::default(), None, &mut links, &mut notes, &mut toks);
    let toks = collapse(toks);
    let escaped = escapes(&toks);
    write(&toks, &escaped, &links, &notes)
}

fn flatten<'a>(inlines: &'a [Inline], outer: Style, link: Option<usize>, links: &mut Vec<&'a str>, notes: &mut Vec<&'a str>, out: &mut Vec<Tok>) {
    for i in inlines {
        match i {
            Inline::Text(t, s) => {
                let style = Style { em: outer.em || s.em, strong: outer.strong || s.strong };
                out.extend(t.chars().map(|c| Tok::Char(c, style, link)));
            }
            Inline::Break => out.push(Tok::Break),
            Inline::Link(href, children) if link.is_none() => {
                links.push(href);
                let at = links.len() - 1;
                flatten(children, outer, Some(at), links, notes, out);
            }
            Inline::Link(_, children) => flatten(children, outer, link, links, notes, out),
            Inline::Note(label) => {
                notes.push(label);
                out.push(Tok::Note(notes.len() - 1));
            }
        }
    }
}

fn is_space(c: char) -> bool {
    c.is_whitespace() && c != '\u{a0}'
}

/// One space for each run of whitespace, none at either end of a line, and a space takes the
/// style and link of the text on both sides only when they agree.
fn collapse(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::with_capacity(toks.len());
    for t in toks {
        match t {
            Tok::Char(c, ..) if is_space(c) => {
                if matches!(out.last(), Some(Tok::Char(..) | Tok::Note(_))) && !matches!(out.last(), Some(Tok::Char(' ', ..))) {
                    out.push(Tok::Char(' ', Style::default(), None));
                }
            }
            Tok::Break => {
                if matches!(out.last(), Some(Tok::Char(' ', ..))) {
                    out.pop();
                }
                if !out.is_empty() {
                    out.push(Tok::Break);
                }
            }
            t => out.push(t),
        }
    }
    while matches!(out.last(), Some(Tok::Char(' ', ..) | Tok::Break)) {
        out.pop();
    }
    for i in 0..out.len() {
        if let Tok::Char(' ', ..) = out[i]
            && let (Some(Tok::Char(_, a, la)), Some(Tok::Char(_, b, lb))) = (out.get(i.wrapping_sub(1)).copied(), out.get(i + 1).copied())
        {
            let link = if la == lb { la } else { None };
            let style = if a == b && la == lb { a } else { Style::default() };
            out[i] = Tok::Char(' ', style, link);
        }
    }
    out
}

/// Which characters need a backslash. Escaping them all is safe in CommonMark, but the text has
/// to stay readable to an agent, so only what could start syntax is escaped.
fn escapes(toks: &[Tok]) -> Vec<bool> {
    let chars: Vec<Option<char>> = toks.iter().map(|t| if let Tok::Char(c, ..) = t { Some(*c) } else { None }).collect();
    let at = |i: usize| chars.get(i).copied().flatten();
    let mut out = vec![false; toks.len()];
    let mut line_start = true;
    for i in 0..toks.len() {
        let Some(c) = chars[i] else {
            line_start = toks[i] == Tok::Break;
            continue;
        };
        let after_note = i > 0 && matches!(toks[i - 1], Tok::Note(_));
        let before_link = matches!((toks[i], toks.get(i + 1)), (Tok::Char(_, _, a), Some(Tok::Char(_, _, b @ Some(_)))) if a != *b);
        let escape = match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '$' | '~' | '|' | '@' => true,
            '&' => at(i + 1).is_some_and(|n| n.is_ascii_alphanumeric() || n == '#'),
            ':' => after_note || (at(i + 1) == Some('/') && at(i + 2) == Some('/')),
            '.' => i >= 3 && (i - 3..i).all(|j| at(j).is_some_and(|w| w.eq_ignore_ascii_case(&'w'))),
            '(' => after_note,
            '!' => before_link || matches!(toks.get(i + 1), Some(Tok::Note(_))),
            '-' | '+' | '=' => line_start,
            _ => false,
        };
        out[i] |= escape;
        if line_start && c.is_ascii_digit() {
            let end = (i..toks.len()).find(|&j| !at(j).is_some_and(|d| d.is_ascii_digit())).unwrap_or(toks.len());
            if matches!(at(end), Some('.' | ')')) {
                out[end] = true;
            }
        }
        line_start = false;
    }
    out
}

fn is_punct(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace()
}

enum Item {
    Run { from: usize, to: usize, style: Style, link: Option<usize> },
    Break,
    Note(usize),
}

impl Item {
    fn link(&self) -> Option<usize> {
        match self {
            Item::Run { link, .. } => *link,
            _ => None,
        }
    }
}

fn write(toks: &[Tok], escaped: &[bool], links: &[&str], notes: &[&str]) -> String {
    let mut items: Vec<Item> = vec![];
    for (i, t) in toks.iter().enumerate() {
        match *t {
            Tok::Char(_, style, link) => match items.last_mut() {
                Some(Item::Run { to, style: s, link: l, .. }) if *s == style && *l == link => *to = i + 1,
                _ => items.push(Item::Run { from: i, to: i + 1, style, link }),
            },
            Tok::Break => items.push(Item::Break),
            Tok::Note(n) => items.push(Item::Note(n)),
        }
    }
    // The first character of what comes after item `i`, as the parser will see it.
    let source_char = |i: usize| -> Option<char> {
        let next = items.get(i + 1)?;
        if next.link() != items[i].link() {
            return Some(if items[i].link().is_some() { ']' } else { '[' });
        }
        Some(match *next {
            Item::Run { from, .. } if escaped[from] => '\\',
            Item::Run { from, .. } => match toks[from] {
                Tok::Char(c, ..) => c,
                _ => ' ',
            },
            Item::Break => '\\',
            Item::Note(_) => '[',
        })
    };
    let mut out = String::new();
    let mut link: Option<usize> = None;
    let mut closed_emphasis = false;
    for (n, item) in items.iter().enumerate() {
        if item.link() != link {
            if let Some(l) = link {
                out.push_str("](");
                out.push_str(&destination(links[l]));
                out.push(')');
            }
            if item.link().is_some() {
                out.push('[');
            }
            link = item.link();
            closed_emphasis = false;
        }
        match *item {
            Item::Break => {
                out.push_str("\\\n");
                closed_emphasis = false;
            }
            Item::Note(i) => {
                out.push_str("[^");
                out.push_str(notes[i]);
                out.push(']');
                closed_emphasis = false;
            }
            Item::Run { from, to, style, .. } => {
                let char_at = |i: usize| if let Tok::Char(c, ..) = toks[i] { c } else { ' ' };
                let first_punct = escaped[from] || is_punct(char_at(from));
                let last_punct = escaped[to - 1] || is_punct(char_at(to - 1));
                let before = out.chars().last();
                let opens = !first_punct || before.is_none_or(|b| b.is_whitespace() || is_punct(b));
                let closes = !last_punct || source_char(n).is_none_or(|a| a.is_whitespace() || is_punct(a));
                let keep = style != Style::default() && !closed_emphasis && opens && closes && !char_at(from).is_whitespace() && !char_at(to - 1).is_whitespace();
                let delimiter = if keep { style.delimiter() } else { "" };
                out.push_str(delimiter);
                for i in from..to {
                    if escaped[i] {
                        out.push('\\');
                    }
                    out.push(char_at(i));
                }
                out.push_str(delimiter);
                closed_emphasis = keep;
            }
        }
    }
    if let Some(l) = link {
        out.push_str("](");
        out.push_str(&destination(links[l]));
        out.push(')');
    }
    out
}

/// A link destination as Markdown takes it: spaces, angle brackets and parentheses encoded.
fn destination(href: &str) -> String {
    let mut out = String::with_capacity(href.len());
    for c in href.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '\\' => out.push_str("%5C"),
            c if c.is_control() && c != '\u{1}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Plain text escaped so that it reads back as itself: a caption, an alt text, a title.
pub(crate) fn escape(text: &str) -> String {
    inline(&[Inline::text(text)])
}

/// What a block of the document sits in.
enum Container {
    Quote { used: bool },
    Item { marker: String, used: bool, blocks: usize },
}

/// A chapter's Markdown, written block by block.
#[derive(Default)]
pub(crate) struct Doc {
    out: String,
    containers: Vec<Container>,
    /// Counters of the ordered lists open, innermost last; `None` for a bulleted one.
    lists: Vec<Option<u32>>,
    /// The next block starts a list item right after an item of one block, so no blank line
    /// goes between them and the list stays tight.
    tight: bool,
    /// Where the last block starts, when it is a scene break.
    rule: Option<usize>,
}

impl Doc {
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    fn line_prefix(&mut self) -> String {
        let mut p = String::new();
        for c in &mut self.containers {
            match c {
                Container::Quote { used } => {
                    p.push_str("> ");
                    *used = true;
                }
                Container::Item { marker, used, .. } => {
                    if *used {
                        p.extend(std::iter::repeat_n(' ', marker.len()));
                    } else {
                        p.push_str(marker);
                        *used = true;
                    }
                }
            }
        }
        p
    }

    /// Writes one block's lines, after a blank line (or none, inside a tight list). Returns where
    /// the block starts.
    fn block(&mut self, text: &str) -> usize {
        if text.is_empty() {
            return self.out.len();
        }
        if !self.out.is_empty() {
            self.out.push('\n');
            if !self.tight {
                let mut blank = String::new();
                for c in &self.containers {
                    match c {
                        Container::Quote { used: true } => blank.push_str("> "),
                        Container::Quote { used: false } | Container::Item { used: false, .. } => break,
                        Container::Item { marker, .. } => blank.extend(std::iter::repeat_n(' ', marker.len())),
                    }
                }
                self.out.push_str(blank.trim_end());
                self.out.push('\n');
            }
        }
        self.tight = false;
        self.rule = None;
        if let Some(Container::Item { blocks, .. }) = self.containers.iter_mut().rev().find(|c| matches!(c, Container::Item { .. })) {
            *blocks += 1;
        }
        let start = self.out.len();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.out.push('\n');
            }
            let prefix = self.line_prefix();
            self.out.push_str(&prefix);
            self.out.push_str(line);
        }
        start
    }

    pub fn paragraph(&mut self, inlines: &[Inline]) -> usize {
        let text = inline(inlines);
        self.block(&text)
    }

    pub fn heading(&mut self, level: u8, inlines: &[Inline]) -> usize {
        let flat: Vec<Inline> = inlines.iter().map(|i| if *i == Inline::Break { Inline::text(" ") } else { i.clone() }).collect();
        let text = inline(&flat);
        if text.is_empty() {
            return self.out.len();
        }
        self.block(&format!("{} {text}", "#".repeat(level.clamp(1, 6) as usize)))
    }

    /// A scene break. None opens the text, none follows another, and one left at the end is
    /// dropped.
    pub fn rule(&mut self) {
        if !self.out.is_empty() && self.rule.is_none() {
            let start = self.block("* * *");
            self.rule = Some(start);
        }
    }

    fn drop_trailing_rule(&mut self) {
        if let Some(start) = self.rule.take() {
            self.out.truncate(start);
            let kept = self.out.trim_end_matches(['\n', '>', ' ']).len();
            self.out.truncate(kept);
        }
    }

    /// An image on its own, with its caption as the image's title.
    pub fn image(&mut self, alt: &str, src: &str, caption: &str) -> usize {
        let title = if caption.is_empty() {
            String::new()
        } else {
            let quoted: String = caption.chars().flat_map(|c| if matches!(c, '"' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
            format!(" \"{quoted}\"")
        };
        self.block(&format!("![{}]({}{title})", escape(alt), destination(src)))
    }

    pub fn open_quote(&mut self) {
        self.containers.push(Container::Quote { used: false });
    }

    pub fn close_quote(&mut self) {
        if let Some(at) = self.containers.iter().rposition(|c| matches!(c, Container::Quote { .. })) {
            self.containers.truncate(at);
        }
    }

    pub fn open_list(&mut self, ordered: bool, start: u32) {
        self.lists.push(ordered.then_some(start));
    }

    pub fn close_list(&mut self) {
        self.lists.pop();
        self.tight = false;
    }

    pub fn open_item(&mut self) {
        let marker = match self.lists.last_mut() {
            Some(Some(n)) => {
                let m = format!("{n}. ");
                *n += 1;
                m
            }
            _ => "- ".to_string(),
        };
        self.containers.push(Container::Item { marker, used: false, blocks: 0 });
    }

    pub fn close_item(&mut self) {
        if let Some(at) = self.containers.iter().rposition(|c| matches!(c, Container::Item { .. })) {
            let blocks = match &self.containers[at] {
                Container::Item { blocks, .. } => *blocks,
                Container::Quote { .. } => 0,
            };
            self.containers.truncate(at);
            self.tight = blocks == 1;
        }
    }

    /// Footnote definitions, after the chapter's text.
    pub fn note(&mut self, label: &str, inlines: &[Inline]) {
        self.drop_trailing_rule();
        self.containers.clear();
        self.tight = false;
        let text = inline(inlines);
        self.block(&format!("[^{label}]: {text}"));
    }

    pub fn finish(mut self) -> String {
        self.drop_trailing_rule();
        if !self.out.is_empty() {
            self.out.push('\n');
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use markdown::mdast::Node;
    use proptest::prelude::*;

    /// The options GPUI Kit parses with: GFM, and maths on.
    fn parse(md: &str) -> Node {
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_text = true;
        options.constructs.math_flow = true;
        markdown::to_mdast(md, &options).unwrap()
    }

    /// The text a reader would see, and every kind of node it took to say it.
    fn read(node: &Node, text: &mut String, kinds: &mut Vec<&'static str>) {
        let kind = match node {
            Node::Root(_) => "root",
            Node::Paragraph(_) => "paragraph",
            Node::Text(t) => {
                text.push_str(&t.value);
                "text"
            }
            Node::Break(_) => {
                text.push('\n');
                "break"
            }
            Node::Emphasis(_) => "emphasis",
            Node::Strong(_) => "strong",
            Node::Link(_) => "link",
            _ => "other",
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
        for child in node.children().into_iter().flatten() {
            read(child, text, kinds);
        }
    }

    /// Whitespace as `collapse` leaves it: one space between words, and no line break at
    /// either end.
    fn expected(inlines: &[Inline]) -> String {
        let plain = Inline::plain(inlines);
        let lines: Vec<String> = plain.split('\n').map(|l| l.split(is_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")).collect();
        lines.join("\n").trim_matches('\n').to_string()
    }

    fn style() -> impl Strategy<Value = Style> {
        (any::<bool>(), any::<bool>()).prop_map(|(em, strong)| Style { em, strong })
    }

    /// Text full of what Markdown reads as syntax, as books print it.
    fn words() -> impl Strategy<Value = String> {
        proptest::string::string_regex(r#"([ a-z0-9.,;:!?'"()/\-+=#*_`~$&@<>|\[\]\\]|www\.|http://|&amp;|1\. |\* |\u{a0}|é|—|“|”){0,12}"#).unwrap()
    }

    fn inlines() -> impl Strategy<Value = Vec<Inline>> {
        let piece = prop_oneof![
            8 => (words(), style()).prop_map(|(t, s)| Inline::Text(t, s)),
            1 => Just(Inline::Break),
            1 => prop::collection::vec((words(), style()).prop_map(|(t, s)| Inline::Text(t, s)), 1..3).prop_map(|c| Inline::Link("#2:40".into(), c)),
        ];
        prop::collection::vec(piece, 0..8)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        /// Whatever the book printed, and however it set it, the Markdown reads back as the same
        /// text: no stray asterisk, no backslash, nothing turned into a heading, a list, a formula,
        /// code or a link the book didn't have.
        #[test]
        fn escaped_book_text_reads_back_as_itself(content in inlines()) {
            let md = inline(&content);
            let (mut text, mut kinds) = (String::new(), vec![]);
            read(&parse(&md), &mut text, &mut kinds);
            prop_assert_eq!(&text, &expected(&content), "{:?} wrote {:?}", content, md);
            prop_assert!(kinds.iter().all(|k| *k != "other"), "{:?} wrote {:?}, read as {:?}", content, md, kinds);
            let links = content.iter().any(|i| matches!(i, Inline::Link(_, c) if !expected(c).is_empty()));
            prop_assert!(links || !kinds.contains(&"link"), "{:?} became a link: {:?}", content, md);
        }
    }

    #[test]
    fn emphasis_is_written_where_it_reads_as_emphasis() {
        let em = Style { em: true, strong: false };
        let doc = [Inline::text("a "), Inline::Text("deliberately".into(), em), Inline::text(", to front")];
        assert_eq!(inline(&doc), "a *deliberately*, to front");
        let snug = [Inline::Text("“so”".into(), em), Inline::text("ever")];
        assert_eq!(inline(&snug), "“so”ever", "a closing * between punctuation and a letter would print, so the run stays plain");
    }

    #[test]
    fn blocks_nest_in_quotes_and_lists() {
        let mut doc = Doc::default();
        doc.heading(1, &[Inline::text("Economy")]);
        doc.open_list(true, 1);
        for item in ["One", "Two"] {
            doc.open_item();
            doc.paragraph(&[Inline::text(item)]);
            doc.close_item();
        }
        doc.close_list();
        doc.open_quote();
        doc.paragraph(&[Inline::text("A line"), Inline::Break, Inline::text("- and another")]);
        doc.paragraph(&[Inline::text("Second")]);
        doc.close_quote();
        doc.image("A pond", "images/ab/1.png", "The pond, \"in winter\"");
        doc.note("1", &[Inline::text("Written at the pond.")]);
        assert_eq!(
            doc.finish(),
            "# Economy\n\n1. One\n2. Two\n\n> A line\\\n> \\- and another\n>\n> Second\n\n![A pond](images/ab/1.png \"The pond, \\\"in winter\\\"\")\n\n[^1]: Written at the pond.\n"
        );
    }
}

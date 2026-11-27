//! EPUB 2 and 3: container.xml → OPF (metadata, manifest, spine) → XHTML → blocks, with chapters
//! split where the nav (or NCX) points, which is often in the middle of a spine file.
//!
//! Content documents go through quick-xml with end-name checks off, and entities resolve through
//! the HTML table, so `&nbsp;` and unclosed `<br>` don't stop a book from opening.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::book::{Chapter, display_title};
use crate::markdown::{Doc, Inline, Style};

#[derive(Debug)]
pub struct Parsed {
    pub title: Option<String>,
    pub author: Option<String>,
    pub chapters: Vec<Chapter>,
    /// The images the chapters show, by their file name under the book's image folder.
    pub images: Vec<(String, Vec<u8>)>,
}

pub fn parse(bytes: &[u8], image_dir: &str) -> Result<Parsed, String> {
    let mut zip = Zip::open(bytes)?;
    let container = zip.read_text("META-INF/container.xml").ok_or("it has no META-INF/container.xml")?;
    let opf_path = first_attr(&container, b"rootfile", b"full-path").ok_or("its container.xml names no package file")?;
    let opf = zip.read_text(&opf_path).ok_or_else(|| format!("its package file {opf_path} is missing"))?;
    let package = Package::parse(&opf, &dir_of(&opf_path));

    let targets = package
        .nav
        .as_ref()
        .and_then(|nav| zip.read_text(nav).map(|src| nav_targets(&src, &dir_of(nav))))
        .filter(|t| !t.is_empty())
        .or_else(|| package.ncx.as_ref().and_then(|ncx| zip.read_text(ncx).map(|src| ncx_targets(&src, &dir_of(ncx)))))
        .unwrap_or_default();

    let docs: Vec<(String, String)> = package
        .spine
        .iter()
        .filter(|doc| package.cover_page.as_ref() != Some(*doc))
        .filter_map(|doc| Some((doc.clone(), zip.read_text(doc)?)))
        .collect();
    let refs = References::scan(&docs);

    let mut notes = Walker::new(true, &refs);
    for (doc, src) in &docs {
        notes.walk(doc, src, &HashMap::new(), &mut |_| None);
    }
    let notes = notes.notes;

    let mut images: Vec<(String, Vec<u8>)> = vec![];
    let mut named: HashMap<String, Option<String>> = HashMap::new();
    let mut image = |path: &str| -> Option<String> {
        named
            .entry(path.to_string())
            .or_insert_with(|| {
                let bytes = zip.read_bytes(path)?;
                let name = format!("{}.{}", images.len() + 1, image_extension(&bytes)?);
                images.push((name.clone(), bytes));
                Some(format!("{image_dir}{name}"))
            })
            .clone()
    };
    let mut walker = Walker::new(false, &refs).with_notes(notes);
    for (doc, src) in &docs {
        let here: HashMap<&str, &str> = targets
            .iter()
            .filter(|t| &t.file == doc)
            .map(|t| (t.fragment.as_deref().unwrap_or(""), t.label.as_str()))
            .collect();
        if targets.is_empty() {
            walker.start_chapter(None);
        } else if let Some(label) = here.get("") {
            walker.start_chapter(Some(label));
        }
        walker.walk(doc, src, &here, &mut image);
    }
    let chapters = walker.finish();
    if chapters.is_empty() {
        return Err("it has no readable text".into());
    }
    Ok(Parsed { title: package.title, author: package.author, chapters, images })
}

/// A web page's article, already cleaned to HTML, as chapters: one document and no contents
/// list, so usually one chapter. Each picture is fetched once, through `fetch`, by its absolute
/// address, and kept by its file name under the article's image folder.
pub(crate) fn page(src: &str, image_dir: &str, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Parsed {
    const DOC: &str = "page.html";
    let docs = [(DOC.to_string(), src.to_string())];
    let refs = References::scan(&docs);
    let mut notes = Walker::new(true, &refs);
    notes.web = true;
    notes.walk(DOC, src, &HashMap::new(), &mut |_| None);
    let notes = notes.notes;

    let mut images: Vec<(String, Vec<u8>)> = vec![];
    let mut named: HashMap<String, Option<String>> = HashMap::new();
    let mut image = |url: &str| -> Option<String> {
        named
            .entry(url.to_string())
            .or_insert_with(|| {
                let bytes = fetch(url)?;
                let name = format!("{}.{}", images.len() + 1, image_extension(&bytes)?);
                images.push((name.clone(), bytes));
                Some(format!("{image_dir}{name}"))
            })
            .clone()
    };
    let mut walker = Walker::new(false, &refs).with_notes(notes);
    walker.web = true;
    walker.start_chapter(None);
    walker.walk(DOC, src, &HashMap::new(), &mut image);
    let chapters = walker.finish();
    Parsed { title: None, author: None, chapters, images }
}

/// The title and author an EPUB's package file names, without reading its text. `None` for a
/// file that isn't an EPUB.
pub fn metadata(bytes: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let mut zip = Zip::open(bytes).ok()?;
    let container = zip.read_text("META-INF/container.xml")?;
    let opf_path = first_attr(&container, b"rootfile", b"full-path")?;
    let package = Package::parse(&zip.read_text(&opf_path)?, &dir_of(&opf_path));
    Some((package.title, package.author))
}

/// The cover image an EPUB names, if it names one in a format the app can draw.
pub struct Cover {
    pub bytes: Vec<u8>,
    /// The file extension its bytes say it is.
    pub extension: &'static str,
}

/// Finds the book's cover: the manifest's `cover-image`, the EPUB 2 `<meta name="cover">`, an
/// image named like a cover, then the first image of a cover page.
pub fn cover(bytes: &[u8]) -> Option<Cover> {
    let mut zip = Zip::open(bytes).ok()?;
    let container = zip.read_text("META-INF/container.xml")?;
    let opf_path = first_attr(&container, b"rootfile", b"full-path")?;
    let opf = zip.read_text(&opf_path)?;
    let package = Package::parse(&opf, &dir_of(&opf_path));
    let from_page = |zip: &mut Zip, page: &str| {
        let src = zip.read_text(page)?;
        let image = events(&src).into_iter().find_map(|ev| match ev {
            Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "img" || tag == "image" => {
                attrs.into_iter().find(|(k, _)| k == "src" || k == "href").map(|(_, v)| v)
            }
            _ => None,
        })?;
        Some(resolve(&dir_of(page), &image).0)
    };
    let image = package.cover.clone().or_else(|| package.cover_page.as_deref().and_then(|page| from_page(&mut zip, page)))?;
    let bytes = zip.read_bytes(&image)?;
    let extension = image_extension(&bytes)?;
    Some(Cover { bytes, extension })
}

fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xff, 0xd8, 0xff, ..] => Some("jpg"),
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

struct Zip<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Zip<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self, String> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "it isn't a valid EPUB (not a zip archive)".to_string())?;
        Ok(Zip { archive })
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        self.read_bytes(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        let name = match self.archive.index_for_name(path) {
            Some(i) => i,
            None => (0..self.archive.len()).find(|&i| self.archive.name_for_index(i).is_some_and(|n| n.as_ref().is_ok_and(|n| n.eq_ignore_ascii_case(path))))?,
        };
        let mut file = self.archive.by_index(name).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

#[derive(Default)]
struct Package {
    title: Option<String>,
    author: Option<String>,
    spine: Vec<String>,
    nav: Option<String>,
    ncx: Option<String>,
    /// The cover image's path in the archive.
    cover: Option<String>,
    /// A page holding the cover, for books that don't name the image itself.
    cover_page: Option<String>,
}

impl Package {
    fn parse(src: &str, base: &str) -> Package {
        let mut pkg = Package::default();
        let mut manifest: HashMap<String, (String, String, String)> = HashMap::new();
        let mut spine_ids = Vec::new();
        let mut toc_id = None;
        let mut cover_meta = None;
        let mut guide_cover = None;
        let mut capture: Option<&'static str> = None;
        let mut buf = String::new();
        for ev in events(src) {
            match ev {
                Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "item" => {
                    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
                    manifest.insert(get("id"), (get("href"), get("media-type"), get("properties")));
                }
                Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "meta" => {
                    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                    if get("name").is_some_and(|n| n.eq_ignore_ascii_case("cover")) {
                        cover_meta = get("content").map(str::to_string);
                    }
                }
                Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "reference" => {
                    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                    if get("type").is_some_and(|t| t.eq_ignore_ascii_case("cover")) {
                        guide_cover = get("href").map(str::to_string);
                    }
                }
                Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "itemref" => {
                    if let Some((_, id)) = attrs.iter().find(|(n, _)| n == "idref") {
                        spine_ids.push(id.clone());
                    }
                }
                Xml::Open(tag, attrs) if tag == "spine" => {
                    toc_id = attrs.iter().find(|(n, _)| n == "toc").map(|(_, v)| v.clone());
                }
                Xml::Open(tag, _) if (tag == "title" && pkg.title.is_none()) || (tag == "creator" && pkg.author.is_none()) => {
                    capture = Some(if tag == "title" { "title" } else { "creator" });
                    buf.clear();
                }
                Xml::Text(t) if capture.is_some() => buf.push_str(&t),
                Xml::Close(tag) if capture == Some(if tag == "title" { "title" } else { "creator" }) => {
                    let value = collapse(&buf);
                    if !value.is_empty() {
                        if tag == "title" { pkg.title = Some(value) } else { pkg.author = Some(value) }
                    }
                    capture = None;
                }
                _ => {}
            }
        }
        pkg.spine = spine_ids.iter().filter_map(|id| manifest.get(id)).map(|(href, _, _)| resolve(base, href).0).collect();
        pkg.nav = manifest.values().find(|(_, _, props)| props.split_whitespace().any(|p| p == "nav")).map(|(href, _, _)| resolve(base, href).0);
        pkg.ncx = toc_id
            .and_then(|id| manifest.get(&id))
            .or_else(|| manifest.values().find(|(_, mt, _)| mt == "application/x-dtbncx+xml"))
            .map(|(href, _, _)| resolve(base, href).0);
        let is_image = |mt: &str| mt.starts_with("image/");
        let named_cover = || {
            let mut named: Vec<_> = manifest
                .iter()
                .filter(|(id, (href, mt, _))| is_image(mt) && (id.to_lowercase().contains("cover") || href.to_lowercase().contains("cover")))
                .map(|(_, (href, _, _))| href)
                .collect();
            named.sort();
            named.first().map(|href| href.to_string())
        };
        pkg.cover = manifest
            .values()
            .find(|(_, _, props)| props.split_whitespace().any(|p| p == "cover-image"))
            .map(|(href, _, _)| href.clone())
            .or_else(|| cover_meta.as_ref().and_then(|id| manifest.get(id)).filter(|(_, mt, _)| is_image(mt)).map(|(href, _, _)| href.clone()))
            .or_else(named_cover)
            .map(|href| resolve(base, &href).0);
        pkg.cover_page = guide_cover.map(|href| resolve(base, &href).0).or_else(|| pkg.spine.first().filter(|doc| doc.to_lowercase().contains("cover")).cloned());
        pkg
    }
}

/// Where a table-of-contents entry points: a spine file and, usually, an element id in it.
#[derive(Debug)]
struct Target {
    file: String,
    fragment: Option<String>,
    label: String,
}

fn push_target(out: &mut Vec<Target>, base: &str, href: &str, label: &str) {
    let (file, fragment) = resolve(base, href);
    if !out.iter().any(|t| t.file == file && t.fragment == fragment) {
        out.push(Target { file, fragment, label: collapse(label) });
    }
}

/// EPUB 3: every link inside `<nav epub:type="toc">`, nested lists flattened in reading order.
fn nav_targets(src: &str, base: &str) -> Vec<Target> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut in_toc = false;
    let mut link: Option<(String, String)> = None;
    for ev in events(src) {
        match ev {
            Xml::Open(tag, attrs) if tag == "nav" => {
                depth += 1;
                if depth == 1 {
                    in_toc = attrs.iter().any(|(n, v)| n == "type" && v.split_whitespace().any(|t| t == "toc"));
                }
            }
            Xml::Close(tag) if tag == "nav" => {
                depth = depth.saturating_sub(1);
                if depth == 0 && in_toc {
                    break;
                }
            }
            Xml::Open(tag, attrs) if tag == "a" && in_toc => {
                link = attrs.iter().find(|(n, _)| n == "href").map(|(_, v)| (v.clone(), String::new()));
            }
            Xml::Text(t) if in_toc => {
                if let Some((_, label)) = &mut link {
                    label.push_str(&t);
                }
            }
            Xml::Close(tag) if tag == "a" => {
                if let Some((href, label)) = link.take() {
                    push_target(&mut out, base, &href, &label);
                }
            }
            _ => {}
        }
    }
    out
}

/// EPUB 2: `navPoint`s in document order, each a `navLabel/text` and a `content src`.
fn ncx_targets(src: &str, base: &str) -> Vec<Target> {
    let mut out = Vec::new();
    let mut label = String::new();
    let mut in_text = false;
    for ev in events(src) {
        match ev {
            Xml::Open(tag, _) if tag == "navPoint" => label.clear(),
            Xml::Open(tag, _) if tag == "text" => in_text = true,
            Xml::Close(tag) if tag == "text" => in_text = false,
            Xml::Text(t) if in_text => label.push_str(&t),
            Xml::Open(tag, attrs) | Xml::Empty(tag, attrs) if tag == "content" => {
                if let Some((_, href)) = attrs.iter().find(|(n, _)| n == "src") {
                    push_target(&mut out, base, href, &label);
                }
            }
            _ => {}
        }
    }
    out
}

/// What every content document holds that a link can point at, read before any is written, so a
/// link can be told from a footnote marker and a target that isn't there.
#[derive(Default)]
struct References {
    /// Every element id, as (file, id).
    ids: HashSet<(String, String)>,
    files: HashSet<String>,
    /// Every target a link points at, as (file, id or "").
    linked: HashSet<(String, String)>,
    /// The targets of links marked as footnote markers.
    noterefs: HashSet<(String, String)>,
}

impl References {
    fn scan(docs: &[(String, String)]) -> References {
        let mut r = References::default();
        for (file, src) in docs {
            r.files.insert(file.clone());
            let base = dir_of(file);
            for ev in events(src) {
                let (Xml::Open(tag, attrs) | Xml::Empty(tag, attrs)) = ev else { continue };
                for (n, v) in &attrs {
                    if n == "id" || (n == "name" && tag == "a") {
                        r.ids.insert((file.clone(), v.clone()));
                    }
                }
                if tag == "a"
                    && let Some(href) = attr(&attrs, "href")
                    && !is_external(href)
                {
                    let to = target(file, &base, href);
                    if is_noteref(&attrs) {
                        r.noterefs.insert(to.clone());
                    }
                    r.linked.insert(to);
                }
            }
        }
        r
    }

    fn exists(&self, to: &(String, String)) -> bool {
        if to.1.is_empty() { self.files.contains(&to.0) } else { self.ids.contains(to) }
    }
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn has_token(attrs: &[(String, String)], name: &str, tokens: &[&str]) -> bool {
    attr(attrs, name).is_some_and(|v| v.split_whitespace().any(|t| tokens.iter().any(|w| t.eq_ignore_ascii_case(w))))
}

/// A link that leaves the book: one with a scheme.
fn is_external(href: &str) -> bool {
    href.split_once(':').is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
}

fn target(file: &str, base: &str, href: &str) -> (String, String) {
    match href.strip_prefix('#') {
        Some(id) => (file.to_string(), percent_decode(id)),
        None => {
            let (path, fragment) = resolve(base, href);
            (path, fragment.unwrap_or_default())
        }
    }
}

fn is_noteref(attrs: &[(String, String)]) -> bool {
    has_token(attrs, "type", &["noteref"])
        || has_token(attrs, "role", &["doc-noteref"])
        || has_token(attrs, "class", &["noteref", "fnanchor", "footnote-ref", "footnoteref", "footnote-link"])
}

fn is_note(attrs: &[(String, String)]) -> bool {
    has_token(attrs, "type", &["footnote", "endnote", "rearnote"])
        || has_token(attrs, "role", &["doc-footnote", "doc-endnote"])
        || has_token(attrs, "class", &["footnote", "endnote"])
}

fn is_figure(tag: &str, attrs: &[(String, String)]) -> bool {
    tag == "figure" || has_token(attrs, "role", &["figure"]) || has_token(attrs, "class", &["figcenter", "figleft", "figright", "figure"])
}

fn style_of(tag: &str, attrs: &[(String, String)]) -> Style {
    let class = attr(attrs, "class").unwrap_or_default().to_ascii_lowercase();
    let css = attr(attrs, "style").unwrap_or_default().to_ascii_lowercase().replace(' ', "");
    Style {
        em: matches!(tag, "em" | "i" | "cite" | "dfn" | "var") || class.split_whitespace().any(|c| c == "italic" || c == "ital" || c == "i") || css.contains("font-style:italic"),
        strong: matches!(tag, "strong" | "b") || class.split_whitespace().any(|c| c == "bold") || css.contains("font-weight:bold"),
    }
}

/// A back-link from a note to its marker: "[1]", "2", "↩".
fn is_backlink(text: &str) -> bool {
    let t = text.trim();
    t.contains('↩') || (t.chars().count() <= 5 && t.chars().all(|c| c.is_alphanumeric() || matches!(c, '[' | ']' | '(' | ')' | '.' | '*' | '†' | '‡' | '§')))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Quote,
    List,
    Item,
    Figure,
    /// A footnote's own element.
    Note,
    /// A footnote marker: its text is the marker, which the reader draws itself.
    NoteRef,
    Link,
}

/// One open element in a content document, and what it changed on the way in.
struct Open {
    tag: String,
    skip: bool,
    block: bool,
    heading: Option<u8>,
    style: Style,
    kind: Kind,
    /// A note's key: its own id, or the first one inside it.
    key: Option<String>,
    /// A link's place in the paragraph and its destination.
    link: Option<(usize, String)>,
}

/// One chapter as it is written.
struct Build {
    label: Option<String>,
    title: Option<String>,
    doc: Doc,
    /// Blocks besides the title.
    body: usize,
    /// Blocks of text: a chapter of only pictures, like the plates before a book's first
    /// chapter, is left out, as it was before images were kept.
    text: usize,
    /// The notes its markers point at, in order: the label of each is its place, from 1.
    notes: Vec<(String, String)>,
}

/// The footnotes of a book, read in a first pass.
#[derive(Default)]
struct Notes {
    text: HashMap<(String, String), Vec<Inline>>,
    /// Every id inside a note, to the note's key.
    alias: HashMap<(String, String), (String, String)>,
    /// Notes some link points at, so the reader shows them as notes rather than as text.
    referenced: HashSet<(String, String)>,
}

struct Walker<'r> {
    /// The first pass, which only reads the notes.
    collect: bool,
    /// A web page rather than a book: a picture's address is already absolute.
    web: bool,
    refs: &'r References,
    notes: Notes,
    file: String,
    base: String,
    chapters: Vec<Build>,
    stack: Vec<Open>,
    inlines: Vec<Inline>,
    skipping: usize,
    /// Ids read since the last block was written, waiting for where the next one starts.
    waiting: Vec<(String, String)>,
    /// Each id's chapter and byte offset, as first written.
    anchors: HashMap<(String, String), (usize, usize)>,
    /// Ids in the paragraph being read, and the note it is when one of them is a note's.
    block_ids: Vec<String>,
    block_note: Option<String>,
    /// Images waiting for the end of the paragraph they sit in.
    images: Vec<(String, String)>,
    /// Inside a figure: its images and caption.
    figure: Option<(Vec<(String, String)>, String)>,
    /// Where each link inside the book points, by its placeholder.
    links: Vec<(String, String)>,
}

/// A link's destination until every chapter is written and the offsets are known.
fn placeholder(n: usize) -> String {
    format!("\u{1}{n}\u{1}")
}

impl<'r> Walker<'r> {
    fn new(collect: bool, refs: &'r References) -> Self {
        Walker {
            collect,
            web: false,
            refs,
            notes: Notes::default(),
            file: String::new(),
            base: String::new(),
            chapters: vec![],
            stack: vec![],
            inlines: vec![],
            skipping: 0,
            waiting: vec![],
            anchors: HashMap::new(),
            block_ids: vec![],
            block_note: None,
            images: vec![],
            figure: None,
            links: vec![],
        }
    }

    /// Takes the notes the first pass read.
    fn with_notes(mut self, mut notes: Notes) -> Self {
        for (id, key) in &notes.alias {
            if self.refs.linked.contains(id) || self.refs.noterefs.contains(id) {
                notes.referenced.insert(key.clone());
            }
        }
        self.notes = notes;
        self
    }

    fn start_chapter(&mut self, label: Option<&str>) {
        self.flush();
        self.chapters.push(Build { label: label.map(str::to_owned), title: None, doc: Doc::default(), body: 0, text: 0, notes: vec![] });
        // A quote or list open across the split carries on in the new chapter.
        let kinds: Vec<(Kind, String)> = self.stack.iter().map(|o| (o.kind, o.tag.clone())).collect();
        for (kind, tag) in kinds {
            match kind {
                Kind::Quote => self.doc().open_quote(),
                Kind::List => self.doc().open_list(tag == "ol", 1),
                Kind::Item => self.doc().open_item(),
                _ => {}
            }
        }
    }

    fn doc(&mut self) -> &mut Doc {
        if self.chapters.is_empty() {
            self.chapters.push(Build { label: None, title: None, doc: Doc::default(), body: 0, text: 0, notes: vec![] });
        }
        &mut self.chapters.last_mut().expect("a chapter was just ensured").doc
    }

    fn walk(&mut self, file: &str, src: &str, targets: &HashMap<&str, &str>, image: &mut dyn FnMut(&str) -> Option<String>) {
        self.file = file.to_string();
        self.base = dir_of(file);
        self.stack.clear();
        self.skipping = 0;
        self.waiting.push((file.to_string(), String::new()));
        for ev in events(src) {
            match ev {
                Xml::Open(tag, attrs) => {
                    self.check_target(&attrs, targets);
                    if is_void(&tag) {
                        self.empty(&tag, &attrs, image);
                    } else {
                        self.open(tag, &attrs);
                    }
                }
                Xml::Empty(tag, attrs) => {
                    self.check_target(&attrs, targets);
                    self.empty(&tag, &attrs, image);
                }
                Xml::Close(tag) => self.close(&tag),
                Xml::Text(t) => self.text(&t),
            }
        }
        while let Some(tag) = self.stack.last().map(|o| o.tag.clone()) {
            self.close(&tag);
        }
        self.flush();
    }

    fn check_target(&mut self, attrs: &[(String, String)], targets: &HashMap<&str, &str>) {
        if self.skipping > 0 || self.collect {
            return;
        }
        let label = attrs.iter().filter(|(n, _)| n == "id" || n == "name").find_map(|(_, v)| targets.get(v.as_str()));
        if let Some(label) = label {
            let heading = self.stack.iter().rev().find_map(|o| o.heading);
            self.start_chapter(Some(label));
            // An anchor inside a heading starts the chapter at that heading.
            if heading.is_some() {
                self.inlines.clear();
            }
        }
    }

    /// Records the element's ids: anchors for links, and the key of the note they are in.
    fn ids(&mut self, tag: &str, attrs: &[(String, String)], block: bool) {
        for (n, v) in attrs {
            if !(n == "id" || (n == "name" && tag == "a")) {
                continue;
            }
            let id = (self.file.clone(), v.clone());
            self.waiting.push(id.clone());
            self.block_ids.push(v.clone());
            if let Some(note) = self.stack.iter_mut().rev().find(|o| o.kind == Kind::Note)
                && note.key.is_none()
            {
                note.key = Some(v.clone());
            } else if self.refs.noterefs.contains(&id) && !self.stack.iter().any(|o| o.kind == Kind::Note) {
                if block && let Some(top) = self.stack.last_mut().filter(|o| o.kind == Kind::Plain) {
                    top.kind = Kind::Note;
                    top.key = Some(v.clone());
                } else {
                    self.block_note = Some(v.clone());
                }
            }
        }
    }

    fn style(&self) -> Style {
        self.stack.iter().fold(Style::default(), |s, o| Style { em: s.em || o.style.em, strong: s.strong || o.style.strong })
    }

    fn text(&mut self, t: &str) {
        if self.skipping > 0 {
            return;
        }
        let style = self.style();
        // Text inside a link stays apart from the text before it.
        let floor = self.stack.iter().filter_map(|o| o.link.as_ref().map(|l| l.0)).max().unwrap_or(0);
        let len = self.inlines.len();
        match self.inlines.last_mut() {
            Some(Inline::Text(last, s)) if *s == style && len > floor => last.push_str(t),
            _ => self.inlines.push(Inline::Text(t.to_string(), style)),
        }
    }

    fn open(&mut self, tag: String, attrs: &[(String, String)]) {
        // A caption outside a figure belongs to an image the book doesn't place here.
        let stray_caption = self.figure.is_none() && has_token(attrs, "class", &["caption"]);
        let skip = self.skipping > 0 || skipped(&tag, attrs) || stray_caption;
        let heading = heading_level(&tag);
        let in_list = self.stack.last().is_some_and(|o| o.kind == Kind::List);
        let block = !skip && (is_block(&tag) || heading.is_some());
        if block {
            self.flush();
        }
        if !skip && matches!(tag.as_str(), "td" | "th") {
            self.text(" ");
        }
        let in_link = self.stack.iter().any(|o| matches!(o.kind, Kind::Link | Kind::NoteRef));
        let style = style_of(&tag, attrs);
        let mut open = Open { tag, skip, block, heading, style, kind: Kind::Plain, key: None, link: None };
        if !skip {
            open.kind = match open.tag.as_str() {
                "blockquote" => Kind::Quote,
                "ul" | "ol" => Kind::List,
                "li" if in_list => Kind::Item,
                "a" if !in_link => self.link(attrs, &mut open),
                t if is_figure(t, attrs) && self.figure.is_none() => Kind::Figure,
                _ if is_note(attrs) && !self.stack.iter().any(|o| o.kind == Kind::Note) => Kind::Note,
                _ => Kind::Plain,
            };
        }
        if open.kind == Kind::NoteRef {
            open.skip = true;
        }
        if open.skip {
            self.skipping += 1;
        }
        if !self.collect {
            match open.kind {
                Kind::Quote => self.doc().open_quote(),
                Kind::List => {
                    let start = attr(attrs, "start").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                    self.doc().open_list(open.tag == "ol", start);
                }
                Kind::Item => self.doc().open_item(),
                Kind::Figure => self.figure = Some((vec![], String::new())),
                _ => {}
            }
        } else if open.kind == Kind::Figure {
            self.figure = Some((vec![], String::new()));
        }
        let tag = open.tag.clone();
        self.stack.push(open);
        if !skip {
            self.ids(&tag, attrs, block);
        }
    }

    /// What an `<a>` is: a footnote marker, a link in or out of the book, or neither.
    fn link(&mut self, attrs: &[(String, String)], open: &mut Open) -> Kind {
        let Some(href) = attr(attrs, "href") else { return Kind::Plain };
        if is_external(href) {
            let web = href.starts_with("http://") || href.starts_with("https://") || href.starts_with("mailto:");
            open.link = web.then(|| (self.inlines.len(), href.to_string()));
            return if web { Kind::Link } else { Kind::Plain };
        }
        let to = target(&self.file, &self.base, href);
        if self.collect {
            open.link = Some((self.inlines.len(), String::new()));
            return Kind::Link;
        }
        if let Some(key) = self.notes.alias.get(&to).filter(|k| self.notes.referenced.contains(*k)).cloned() {
            self.doc();
            let notes = &mut self.chapters.last_mut().expect("a chapter was just ensured").notes;
            let label = match notes.iter().position(|k| *k == key) {
                Some(at) => at + 1,
                None => {
                    notes.push(key);
                    notes.len()
                }
            };
            self.inlines.push(Inline::Note(label.to_string()));
            return Kind::NoteRef;
        }
        if !self.refs.exists(&to) {
            return Kind::Plain;
        }
        self.links.push(to);
        open.link = Some((self.inlines.len(), placeholder(self.links.len() - 1)));
        Kind::Link
    }

    fn close(&mut self, tag: &str) {
        let Some(at) = self.stack.iter().rposition(|o| o.tag == tag) else { return };
        while self.stack.len() > at {
            let top = self.stack.last().expect("stack is longer than `at`");
            if top.block {
                self.flush();
            }
            let open = self.stack.pop().expect("stack is longer than `at`");
            if open.skip {
                self.skipping -= 1;
            }
            match open.kind {
                Kind::Link => self.close_link(open.link),
                Kind::Quote if !self.collect => self.doc().close_quote(),
                Kind::List if !self.collect => self.doc().close_list(),
                Kind::Item if !self.collect => self.doc().close_item(),
                Kind::Figure => self.close_figure(),
                _ => {}
            }
        }
    }

    fn close_link(&mut self, link: Option<(usize, String)>) {
        let Some((start, dest)) = link else { return };
        if start > self.inlines.len() {
            return;
        }
        let children: Vec<Inline> = self.inlines.drain(start..).collect();
        let text = Inline::plain(&children);
        if self.collect {
            if is_external(&dest) {
                self.inlines.push(Inline::Link(dest, children));
            } else if !is_backlink(&text) {
                self.inlines.extend(children);
            }
        } else if text.trim().is_empty() {
            self.inlines.extend(children);
        } else {
            self.inlines.push(Inline::Link(dest, children));
        }
    }

    fn close_figure(&mut self) {
        self.flush();
        let Some((images, caption)) = self.figure.take() else { return };
        if self.collect {
            return;
        }
        let caption = collapse(&caption);
        if images.is_empty() {
            // A figure of text, such as a poem set apart.
            if !caption.is_empty() {
                self.write(None, |doc| doc.paragraph(&[Inline::text(caption)]));
            }
            return;
        }
        let last = images.len() - 1;
        for (i, (alt, src)) in images.into_iter().enumerate() {
            let caption = if i == last { caption.as_str() } else { "" };
            self.write_image(&alt, &src, caption);
        }
    }

    fn empty(&mut self, tag: &str, attrs: &[(String, String)], image: &mut dyn FnMut(&str) -> Option<String>) {
        if self.skipping > 0 || skipped(tag, attrs) {
            return;
        }
        self.ids(tag, attrs, false);
        match tag {
            "br" => self.inlines.push(Inline::Break),
            "hr" => {
                self.flush();
                self.rule();
            }
            "img" | "image" => {
                let alt = attr(attrs, "alt").unwrap_or_default().trim().to_string();
                // A drop cap is often an image whose alt is the letter.
                if (1..=2).contains(&alt.chars().count()) {
                    return self.text(&alt);
                }
                let Some(src) = attr(attrs, "src").or_else(|| attr(attrs, "href")) else { return };
                let at = if self.web { src.to_string() } else { resolve(&self.base, src).0 };
                let Some(path) = image(&at) else { return };
                match &mut self.figure {
                    Some((images, _)) => images.push((alt, path)),
                    None => self.images.push((alt, path)),
                }
            }
            _ => {}
        }
    }

    /// Writes a block into the chapter, after its title. The chapter's first heading is its
    /// title; a chapter that starts some other way takes its contents label as one.
    fn write(&mut self, title: Option<String>, block: impl FnOnce(&mut Doc) -> usize) {
        self.write_block(title, true, block);
    }

    fn write_image(&mut self, alt: &str, src: &str, caption: &str) {
        self.write_block(None, false, |doc| doc.image(alt, src, caption));
    }

    fn write_block(&mut self, title: Option<String>, text: bool, block: impl FnOnce(&mut Doc) -> usize) {
        self.doc();
        let at = self.chapters.len() - 1;
        let b = &mut self.chapters[at];
        let fresh = b.doc.is_empty() && b.title.is_none();
        if fresh && let Some(t) = title.clone().or_else(|| b.label.clone().filter(|l| !l.is_empty())) {
            b.doc.heading(1, &[Inline::text(display_title(&t))]);
            b.title = Some(t);
        }
        let start = if fresh && title.is_some() {
            0
        } else {
            b.body += 1;
            b.text += usize::from(text);
            block(&mut b.doc)
        };
        for id in self.waiting.drain(..) {
            self.anchors.entry(id).or_insert((at, start));
        }
    }

    fn rule(&mut self) {
        if !self.collect {
            self.doc();
            let b = self.chapters.last_mut().expect("a chapter was just ensured");
            if b.body > 0 {
                b.doc.rule();
            }
        }
    }

    fn flush(&mut self) {
        let inlines = std::mem::take(&mut self.inlines);
        let ids = std::mem::take(&mut self.block_ids);
        let block_note = self.block_note.take();
        if let Some((_, caption)) = &mut self.figure {
            caption.push(' ');
            caption.push_str(&Inline::plain(&inlines));
            return;
        }
        let note = self.stack.iter().rev().find(|o| o.kind == Kind::Note).and_then(|o| o.key.clone()).or(block_note);
        if self.collect {
            if let Some(key) = note {
                let key = (self.file.clone(), key);
                let text = self.notes.text.entry(key.clone()).or_default();
                if !text.is_empty() {
                    text.push(Inline::text(" "));
                }
                text.extend(inlines);
                for id in ids {
                    self.notes.alias.insert((self.file.clone(), id), key.clone());
                }
                self.notes.alias.insert(key.clone(), key);
            }
            return;
        }
        if note.is_some_and(|key| self.notes.referenced.contains(&(self.file.clone(), key))) {
            return;
        }
        let plain = collapse(&Inline::plain(&inlines).replace('\n', " "));
        if !plain.is_empty() {
            let chars = plain.chars().filter(|c| !c.is_whitespace()).count();
            let linked: usize = inlines.iter().map(|i| if let Inline::Link(_, c) = i { Inline::plain(c).chars().filter(|c| !c.is_whitespace()).count() } else { 0 }).sum();
            // A block that is mostly links is the book's own contents list.
            if linked * 10 < chars * 8 {
                let heading = self.stack.iter().rev().find_map(|o| o.heading);
                if is_scene_break(&plain) {
                    self.rule();
                } else if let Some(level) = heading {
                    if self.chapters.last().is_none_or(|b| b.doc.is_empty() && b.title.is_none()) {
                        self.write(Some(plain), |_| 0);
                    } else {
                        self.write(None, |doc| doc.heading(level.clamp(2, 6), &inlines));
                    }
                } else {
                    self.write(None, |doc| doc.paragraph(&inlines));
                }
            }
        }
        for (alt, src) in std::mem::take(&mut self.images) {
            self.write_image(&alt, &src, "");
        }
    }

    fn finish(mut self) -> Vec<Chapter> {
        self.flush();
        let mut kept: Vec<Option<usize>> = vec![];
        let mut chapters = vec![];
        for b in std::mem::take(&mut self.chapters) {
            if b.text == 0 {
                kept.push(None);
                continue;
            }
            kept.push(Some(chapters.len()));
            let mut doc = b.doc;
            for (n, key) in b.notes.iter().enumerate() {
                let text = self.notes.text.get(key).cloned().unwrap_or_default();
                doc.note(&(n + 1).to_string(), &text);
            }
            chapters.push(Chapter { title: b.title, text: doc.finish() });
        }
        // An anchor in a chapter that was left out points at the start of the next one kept.
        let place = |(at, offset): (usize, usize)| -> Option<(usize, usize)> {
            match kept.get(at).copied().flatten() {
                Some(new) => Some((new, offset)),
                None => kept.get(at..)?.iter().flatten().next().map(|&new| (new, 0)),
            }
        };
        let destinations: Vec<Option<(usize, usize)>> = self
            .links
            .iter()
            .map(|to| self.anchors.get(to).or_else(|| self.anchors.get(&(to.0.clone(), String::new()))).copied().and_then(place))
            .collect();
        resolve_links(&mut chapters, &destinations);
        chapters
    }
}

/// Puts each link's destination in place of its placeholder: `#<chapter>:<byte>`. A destination's
/// length moves the offsets after it, so this settles in a few rounds.
fn resolve_links(chapters: &mut [Chapter], targets: &[Option<(usize, usize)>]) {
    // Each chapter's placeholders: where each starts, its link and its length.
    let found: Vec<Vec<(usize, usize, usize)>> = chapters
        .iter()
        .map(|c| {
            let mut out = vec![];
            let mut i = 0;
            while let Some(at) = c.text[i..].find('\u{1}').map(|a| a + i) {
                let Some(end) = c.text[at + 1..].find('\u{1}').map(|e| e + at + 1) else { break };
                if let Ok(n) = c.text[at + 1..end].parse::<usize>() {
                    out.push((at, n, end + 1 - at));
                }
                i = end + 1;
            }
            out
        })
        .collect();
    let mut written: Vec<String> = vec![String::new(); targets.len()];
    for _ in 0..8 {
        let shifted = |chapter: usize, offset: usize| -> usize {
            let delta: isize = found[chapter].iter().filter(|(at, ..)| *at < offset).map(|&(_, n, len)| written[n].len() as isize - len as isize).sum();
            (offset as isize + delta).max(0) as usize
        };
        let next: Vec<String> = targets.iter().map(|t| t.map(|(c, o)| format!("#{c}:{}", shifted(c, o))).unwrap_or_else(|| "#0:0".into())).collect();
        if next == written {
            break;
        }
        written = next;
    }
    for (chapter, places) in chapters.iter_mut().zip(&found) {
        for &(at, n, len) in places.iter().rev() {
            chapter.text.replace_range(at..at + len, &written[n]);
        }
    }
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag.as_bytes() {
        [b'h', d @ b'1'..=b'6'] => Some(d - b'0'),
        _ => None,
    }
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div" | "blockquote" | "li" | "ul" | "ol" | "dd" | "dt" | "dl" | "pre" | "section" | "article" | "header"
            | "footer" | "aside" | "main" | "tr" | "table" | "body" | "address" | "center" | "figure" | "figcaption"
    )
}

fn is_void(tag: &str) -> bool {
    matches!(tag, "br" | "hr" | "img" | "meta" | "link" | "input" | "col" | "wbr" | "source")
}

/// Things that aren't the book's running text: page furniture and the Project Gutenberg licence
/// boilerplate.
fn skipped(tag: &str, attrs: &[(String, String)]) -> bool {
    if matches!(tag, "head" | "script" | "style" | "nav" | "svg" | "math" | "title" | "rt") {
        return true;
    }
    attrs.iter().any(|(name, value)| match name.as_str() {
        "class" => value.split_whitespace().any(|c| matches!(c, "pg-boilerplate" | "x-ebookmaker-pageno" | "pagenum" | "pageno")),
        "type" => value.split_whitespace().any(|t| t == "pagebreak"),
        "role" => value == "doc-pagebreak",
        "id" => matches!(value.as_str(), "pg-header" | "pg-footer"),
        _ => false,
    })
}

fn is_scene_break(text: &str) -> bool {
    text.chars().all(|c| matches!(c, '*' | '⁂' | '·' | '•' | '~' | '#') || c.is_whitespace()) && text.chars().any(|c| !c.is_whitespace())
}

pub(crate) fn collapse(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for word in raw.split(|c: char| c.is_whitespace() && c != '\u{a0}') {
        if !word.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
        }
    }
    out
}

fn dir_of(path: &str) -> String {
    path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
}

/// An href relative to `base`, as a normalized archive path and its fragment.
fn resolve(base: &str, href: &str) -> (String, Option<String>) {
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(percent_decode(f))),
        None => (href, None),
    };
    let path = percent_decode(path);
    let mut parts: Vec<&str> = if path.starts_with('/') { Vec::new() } else { base.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    (parts.join("/"), fragment.filter(|f| !f.is_empty()))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A forgiving XML event: names without namespace prefixes, text with every entity resolved.
enum Xml {
    Open(String, Vec<(String, String)>),
    Empty(String, Vec<(String, String)>),
    Close(String),
    Text(String),
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

fn attrs(e: &BytesStart) -> Vec<(String, String)> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .map(|a| (local(a.key.as_ref()), html_escape::decode_html_entities(&*a.value).into_owned()))
        .collect()
}

/// Every event in `src`, stopping quietly at the first error so a damaged file keeps what read.
fn events(src: &str) -> Vec<Xml> {
    let mut reader = Reader::from_str(src);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    config.trim_text(false);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => out.push(Xml::Open(local(e.name().as_ref()), attrs(&e))),
            Ok(Event::Empty(e)) => out.push(Xml::Empty(local(e.name().as_ref()), attrs(&e))),
            Ok(Event::End(e)) => out.push(Xml::Close(local(e.name().as_ref()))),
            Ok(Event::Text(t)) => out.push(Xml::Text(t.to_string())),
            Ok(Event::CData(t)) => out.push(Xml::Text(t.to_string())),
            Ok(Event::GeneralRef(r)) => {
                let name = r.to_string();
                out.push(Xml::Text(html_escape::decode_html_entities(&format!("&{name};")).into_owned()));
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    out
}

fn first_attr(src: &str, tag: &[u8], attr: &[u8]) -> Option<String> {
    let tag = String::from_utf8_lossy(tag);
    let attr = String::from_utf8_lossy(attr);
    events(src).into_iter().find_map(|ev| match ev {
        Xml::Open(t, a) | Xml::Empty(t, a) if t == tag => a.into_iter().find(|(n, _)| *n == attr).map(|(_, v)| v),
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A small EPUB built in memory: `files` are (path, contents) besides the mimetype.
    pub(crate) fn build(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("mimetype", opts.compression_method(zip::CompressionMethod::Stored)).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        for (path, body) in files {
            zip.start_file(*path, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    pub(crate) fn build_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (path, body) in files {
            zip.start_file(*path, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    pub(crate) fn sample() -> Vec<u8> {
        build(&[
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#,
            ),
            (
                "OPS/book.opf",
                r#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>A Small Book</dc:title><dc:creator>Ann Author</dc:creator></metadata>
                <manifest><item id="nav" href="nav.xhtml" properties="nav"/><item id="a" href="text/one.xhtml"/><item id="b" href="text/two%20b.xhtml"/></manifest>
                <spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#,
            ),
            (
                "OPS/nav.xhtml",
                r#"<html><body><nav epub:type="toc"><ol><li><a href="text/one.xhtml">Opening</a></li>
                <li><a href="text/one.xhtml#c2">Second</a><ol><li><a href="text/two%20b.xhtml#c3">Third</a></li></ol></li></ol></nav></body></html>"#,
            ),
            (
                "OPS/text/one.xhtml",
                r#"<html><head><title>x</title><style>p{}</style></head><body>
                <h1>Chapter&nbsp;One</h1><p>It was a  bright
                cold day&mdash;and the clocks<br>were striking.</p>
                <div class="caption"><p>An image caption</p></div>
                <p>* * *</p><blockquote><p>A quoted line.</p></blockquote>
                <h2 id="c2">Chapter Two</h2><p><img alt="T" src="t.png"/>he second &amp; last &#8220;part&#8221;.</p></body></html>"#,
            ),
            ("OPS/text/two b.xhtml", r#"<html><body><p>Before the anchor.</p><h2><a id="c3"/>Three</h2><p>Third text.</p><hr/></body></html>"#),
        ])
    }

    #[test]
    fn splits_chapters_at_nav_anchors_and_reads_blocks() {
        let p = parse(&sample(), "images/x/").unwrap();
        assert_eq!(p.title.as_deref(), Some("A Small Book"));
        assert_eq!(p.author.as_deref(), Some("Ann Author"));
        let titles: Vec<_> = p.chapters.iter().map(|c| c.title.as_deref().unwrap()).collect();
        assert_eq!(titles, ["Chapter\u{a0}One", "Chapter Two", "Three"]);
        assert_eq!(p.chapters[0].text, "# Chapter\u{a0}One\n\nIt was a bright cold day—and the clocks\\\nwere striking.\n\n* * *\n\n> A quoted line.\n");
        assert_eq!(p.chapters[1].text, "# Chapter Two\n\nThe second & last “part”.\n\nBefore the anchor.\n");
        assert_eq!(p.chapters[2].text, "# Three\n\nThird text.\n", "no scene break at a chapter's end");
    }

    #[test]
    fn falls_back_to_one_chapter_per_spine_file_without_a_toc() {
        let epub = build(&[
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#),
            ("c.opf", r#"<package><manifest><item id="a" href="a.html"/><item id="b" href="b.html"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#),
            ("a.html", "<html><body><h1>First</h1><p>One &hellip; &copy;</body></html>"),
            ("b.html", "<html><body><p>Two</p></body></html>"),
        ]);
        let p = parse(&epub, "").unwrap();
        assert_eq!(p.title, None);
        assert_eq!(p.chapters.len(), 2);
        assert_eq!(p.chapters[0].title.as_deref(), Some("First"));
        assert_eq!(p.chapters[0].text, "# First\n\nOne … ©\n");
        assert_eq!(p.chapters[1].title, None);
        assert_eq!(p.chapters[1].text, "Two\n");
    }

    /// An EPUB 3 chapter with what a book sets besides plain text, and its notes in a file of
    /// their own.
    pub(crate) fn walden() -> Vec<u8> {
        let png: &[u8] = &[0x89, b'P', b'N', b'G', 13, 10];
        build_bytes(&[
            ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#),
            (
                "OPS/book.opf",
                br#"<package><metadata><dc:title>Walden</dc:title></metadata><manifest><item id="nav" href="nav.xhtml" properties="nav"/>
                <item id="a" href="text/a.xhtml"/><item id="n" href="text/notes.xhtml"/><item id="p" href="img/pond.png" media-type="image/png"/></manifest>
                <spine><itemref idref="a"/><itemref idref="n"/></spine></package>"#,
            ),
            ("OPS/nav.xhtml", br#"<html><body><nav epub:type="toc"><ol><li><a href="text/a.xhtml">Economy</a></li><li><a href="text/notes.xhtml">Notes</a></li></ol></nav></body></html>"#),
            (
                "OPS/text/a.xhtml",
                br##"<html><body><h1>Economy</h1>
                <p>I went to the woods because I wished to live <em>deliberately</em>, to front only the essential facts of life.<a epub:type="noteref" href="notes.xhtml#n1" id="r1">1</a></p>
                <figure><img src="../img/pond.png" alt="The pond"/><figcaption>The pond in <i>winter</i>.</figcaption></figure>
                <ul><li>Simplicity,</li><li>simplicity!</li></ul>
                <ol start="3"><li>one</li><li>two</li></ol>
                <p># 1. *Not* a heading: 50_000 $ and $5 &amp; more</p>
                <p>See <a href="#lived">below</a>, <a href="https://example.com/a b">the web</a> and <a href="missing.xhtml">nowhere</a>.</p>
                <blockquote><p>Quoted</p></blockquote>
                <h2 id="lived">Where I Lived</h2><p>Text.</p></body></html>"##,
            ),
            (
                "OPS/text/notes.xhtml",
                br#"<html><body><h1>Notes</h1><aside epub:type="footnote" id="n1"><p><a href="a.xhtml#r1">1</a> Written at the pond in the spring of <b>1846</b>.</p></aside></body></html>"#,
            ),
            ("OPS/img/pond.png", png),
        ])
    }

    #[test]
    fn keeps_emphasis_images_lists_notes_and_links() {
        let p = parse(&walden(), "images/w/").unwrap();
        assert_eq!(p.chapters.len(), 1, "the notes file is only notes, so it is no chapter of its own");
        assert_eq!(p.images, [("1.png".to_string(), vec![0x89, b'P', b'N', b'G', 13, 10])]);
        let text = &p.chapters[0].text;
        let lived = text.find("## Where I Lived").expect("the second heading");
        assert_eq!(
            *text,
            format!(
                "# Economy\n\n\
                 I went to the woods because I wished to live *deliberately*, to front only the essential facts of life.[^1]\n\n\
                 ![The pond](images/w/1.png \"The pond in winter.\")\n\n\
                 - Simplicity,\n- simplicity!\n\n\
                 3. one\n4. two\n\n\
                 \\# 1. \\*Not\\* a heading: 50\\_000 \\$ and \\$5 & more\n\n\
                 See [below](#0:{lived}), [the web](https://example.com/a%20b) and nowhere.\n\n\
                 > Quoted\n\n\
                 ## Where I Lived\n\n\
                 Text.\n\n\
                 [^1]: Written at the pond in the spring of **1846**.\n"
            )
        );
    }

    fn opf(manifest: &str, rest: &str) -> String {
        format!(r#"<package xmlns:dc="dc"><metadata><dc:title>T</dc:title>{rest}</metadata><manifest>{manifest}</manifest><spine><itemref idref="a"/></spine></package>"#)
    }

    fn with_cover(opf: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut put = |name: &str, body: &[u8]| {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, body).unwrap();
        };
        put("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="o/b.opf"/></rootfiles></container>"#);
        put("o/b.opf", opf.as_bytes());
        for (name, body) in extra {
            put(name, body);
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn finds_the_cover_each_way_a_book_can_name_it() {
        let png: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2];
        let text = r#"<item id="a" href="a.html" media-type="application/xhtml+xml"/>"#;
        let cases = [
            ("EPUB 3 property", opf(&format!(r#"{text}<item id="x" href="i/c.png" media-type="image/png" properties="cover-image"/><item id="y" href="i/d.png" media-type="image/png"/>"#), "")),
            ("EPUB 2 meta", opf(&format!(r#"{text}<item id="pic" href="i/c.png" media-type="image/png"/><item id="y" href="i/d.png" media-type="image/png"/>"#), r#"<meta name="cover" content="pic"/>"#)),
            ("named like a cover", opf(&format!(r#"{text}<item id="y" href="i/cover.png" media-type="image/png"/><item id="z" href="i/d.png" media-type="image/png"/>"#), "")),
        ];
        for (how, opf) in cases {
            let epub = with_cover(&opf, &[("o/i/c.png", png), ("o/i/cover.png", png), ("o/i/d.png", b"not this")]);
            let found = cover(&epub).unwrap_or_else(|| panic!("{how}: no cover"));
            assert_eq!((found.bytes.as_slice(), found.extension), (png, "png"), "{how}");
        }
    }

    #[test]
    fn finds_the_image_on_a_cover_page_and_ignores_books_without_one() {
        let png: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2];
        let opf_guide = r#"<package><manifest><item id="a" href="a.html" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine><guide><reference type="cover" href="front.html"/></guide></package>"#;
        let epub = with_cover(opf_guide, &[("o/front.html", br#"<html><body><img src="i/f.png"/></body></html>"#), ("o/i/f.png", png)]);
        assert_eq!(cover(&epub).expect("the cover page's image").bytes, png);

        let none = with_cover(&opf(r#"<item id="a" href="a.html" media-type="application/xhtml+xml"/>"#, ""), &[]);
        assert!(cover(&none).is_none());
    }

    #[test]
    fn rejects_files_that_are_not_epubs() {
        assert!(parse(b"not a zip", "").is_err());
        assert!(parse(&build(&[("x.txt", "hi")]), "").unwrap_err().contains("container.xml"));
    }

    #[test]
    fn resolves_relative_hrefs() {
        assert_eq!(resolve("OPS/text", "../img/a%20b.png#x"), ("OPS/img/a b.png".into(), Some("x".into())));
        assert_eq!(resolve("", "a.html"), ("a.html".into(), None));
    }
}

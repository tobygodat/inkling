use serde::{Deserialize, Serialize};

/// Which parser wrote a stored book. A book written by an older one is parsed again from its
/// original file, kept in `originals/`, when there is one. Bump it when a parse would come out
/// meaningfully different.
pub const PARSER: u32 = 1;

/// A book's identity: a hex digest of the imported file's bytes, so importing the same file
/// twice finds the existing book.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BookId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Epub,
    Pdf,
    /// A web page saved by its address: one chapter, with [`Source`] saying where it came from.
    Article,
}

/// Where an article came from, as the page said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The address it was saved from.
    pub url: String,
    /// The site's own name, such as "The Atlantic".
    #[serde(default)]
    pub site: Option<String>,
    /// When it was published, as the page wrote it (usually ISO 8601).
    #[serde(default)]
    pub published: Option<String>,
    /// The page's own summary of itself.
    #[serde(default)]
    pub excerpt: Option<String>,
}

/// One imported book, reduced to Markdown the reader reflows. EPUB and PDF both parse into this;
/// nothing downstream knows which it came from except `format`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub id: BookId,
    pub title: String,
    pub author: Option<String>,
    pub format: Format,
    /// The [`PARSER`] that wrote it: 0 for a book stored before chapters were Markdown.
    #[serde(default)]
    pub parser: u32,
    pub chapters: Vec<Chapter>,
    /// Where an article came from. `None` for a book, and left out of the stored file then. Boxed,
    /// so a book carried around whole by the reading view doesn't grow for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Box<Source>>,
}

/// One chapter. A place in it is a byte offset into `text`, on a char boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct Chapter {
    /// What the contents list calls it, as the book wrote it.
    pub title: Option<String>,
    /// The chapter as Markdown: CommonMark with GFM footnotes. It opens with `# <title>` when the
    /// chapter has a title. Book text that would read as Markdown is escaped (`markdown::escape`).
    /// An image is `![alt](images/<book id>/<n>.<ext> "caption")`, relative to the library; a
    /// link within the book is `#<chapter>:<byte>`.
    pub text: String,
}

impl Chapter {
    /// What the reader calls the chapter at `index`: its title as a reader would set it, else its
    /// first heading, else "Chapter n". A title that is only a number takes the name from the
    /// heading right after it, as in "1 · The Challenge of the Future".
    pub fn label(&self, index: usize) -> String {
        let heading = |line: &str| line.strip_prefix('#').map(|h| plain(h.trim_start_matches('#')));
        let first = self.text.lines().find_map(heading);
        let Some(title) = self.title.as_deref().map(display_title).or(first).filter(|t| !t.is_empty()) else {
            return format!("Chapter {}", index + 1);
        };
        let mut blocks = self.text.lines().filter(|l| !l.trim().is_empty()).map(heading);
        if is_number(&title)
            && blocks.next().flatten().is_some()
            && let Some(name) = blocks.next().flatten().filter(|n| !n.is_empty() && !is_number(n))
        {
            return format!("{title} · {name}");
        }
        title
    }
}

/// A heading's words without the Markdown's escapes and emphasis, set as a reader would.
fn plain(heading: &str) -> String {
    let mut out = String::new();
    let mut chars = heading.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '*' | '_' => {}
            c => out.push(c),
        }
    }
    display_title(&out)
}

/// Whether a title is only a number, such as "1", "IV" or "Chapter 3", and names nothing.
fn is_number(title: &str) -> bool {
    let words: Vec<&str> = title.split_whitespace().collect();
    let number = match words.as_slice() {
        [n] | [_, n] if words.len() == 1 || ["chapter", "part", "book"].contains(&words[0].to_lowercase().as_str()) => n,
        _ => return false,
    };
    number.chars().all(|c| c.is_ascii_digit()) || number.chars().all(|c| "IVXLCDMivxlcdm".contains(c))
}

/// A chapter as either version of the library wrote it: Markdown, or the blocks of a book stored
/// before chapters were Markdown, which read as the same Markdown.
#[derive(Deserialize)]
struct Stored {
    title: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    blocks: Vec<OldBlock>,
}

#[derive(Deserialize)]
enum OldBlock {
    Heading { level: u8, text: String },
    Paragraph(String),
    Quote(String),
    Break,
}

impl From<Stored> for Chapter {
    fn from(s: Stored) -> Chapter {
        if let Some(text) = s.text {
            return Chapter { title: s.title, text };
        }
        use crate::markdown::{Doc, Inline};
        let mut doc = Doc::default();
        if let Some(title) = &s.title {
            doc.heading(1, &[Inline::text(display_title(title))]);
        }
        let lines = |text: &str| -> Vec<Inline> {
            let mut out = vec![];
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    out.push(Inline::Break);
                }
                out.push(Inline::text(line));
            }
            out
        };
        for block in s.blocks {
            match block {
                OldBlock::Heading { level, text } => {
                    doc.heading(level.clamp(2, 6), &[Inline::text(text)]);
                }
                OldBlock::Paragraph(text) => {
                    doc.paragraph(&lines(&text));
                }
                OldBlock::Quote(text) => {
                    doc.open_quote();
                    doc.paragraph(&lines(&text));
                    doc.close_quote();
                }
                OldBlock::Break => {
                    doc.rule();
                }
            }
        }
        Chapter { title: s.title, text: doc.finish() }
    }
}

/// Where the reader left off: a chapter, and how far through it as a fraction in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub chapter: usize,
    pub fraction: f32,
}

impl Position {
    /// The same place in a book whose chapters are now `new`, after they were `old`: the chapter
    /// with the same title, counting repeats in order, else the same index.
    pub fn carried(self, old: &[Option<String>], new: &[Option<String>]) -> Position {
        let clamp = |chapter: usize| chapter.min(new.len().saturating_sub(1));
        let Some(title) = old.get(self.chapter).and_then(Option::as_ref) else {
            return Position { chapter: clamp(self.chapter), ..self };
        };
        let nth = old[..self.chapter].iter().filter(|t| t.as_ref() == Some(title)).count();
        match new.iter().enumerate().filter(|(_, t)| t.as_ref() == Some(title)).nth(nth) {
            Some((at, _)) => Position { chapter: at, ..self },
            None => Position { chapter: clamp(self.chapter), ..self },
        }
    }
}

/// The short words a title keeps in lower case after its first word.
const MINOR: [&str; 15] = ["a", "an", "the", "and", "but", "or", "nor", "for", "of", "in", "on", "at", "to", "by", "as"];

/// A chapter title as a reader would set it: "CHAPTER I." becomes "Chapter I", "THE IDEOLOGY OF
/// COMPETITION" becomes "The Ideology of Competition", and roman numerals stay in capitals.
pub fn display_title(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('.').trim();
    let letters: Vec<char> = t.chars().filter(|c| c.is_alphabetic()).collect();
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    let shouting = letters.len() > 3 && upper * 4 >= letters.len() * 3;
    if !shouting {
        return t.to_string();
    }
    t.split(' ')
        .enumerate()
        .map(|(i, w)| {
            let roman = !w.is_empty() && w.chars().all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M' | '.' | ','));
            if roman || w.chars().any(char::is_lowercase) {
                return w.to_string();
            }
            if let Some(numeral) = w.strip_prefix("CHAPTER").filter(|n| !n.is_empty() && n.chars().all(|c| "IVXLCDM.".contains(c))) {
                return format!("Chapter {numeral}");
            }
            if i > 0 && MINOR.contains(&w.to_lowercase().as_str()) {
                return w.to_lowercase();
            }
            let mut cs = w.chars();
            match cs.next() {
                Some(first) => first.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_stored_as_blocks_reads_as_markdown() {
        let old = r##"{"id":"ab","title":"T","author":null,"format":"Epub","chapters":[
            {"title":"CHAPTER I.","blocks":[{"Paragraph":"It is *so*\nsaid she."},{"Heading":{"level":1,"text":"# Part"}},"Break",{"Quote":"1. A line"}]},
            {"title":null,"blocks":[{"Paragraph":"Two"}]}]}"##;
        let book: Book = serde_json::from_str(old).unwrap();
        assert_eq!(book.parser, 0);
        assert_eq!(book.chapters.len(), 2, "the same chapters, so a saved place still points at the same one");
        assert_eq!(book.chapters[0].title.as_deref(), Some("CHAPTER I."));
        assert_eq!(book.chapters[0].text, "# Chapter I\n\nIt is \\*so\\*\\\nsaid she.\n\n## \\# Part\n\n* * *\n\n> 1\\. A line\n");
        assert_eq!(book.chapters[1].text, "Two\n");
        let again: Book = serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
        assert_eq!(again, book, "and is stored as Markdown from then on");
    }

    #[test]
    fn a_chapter_titled_only_by_its_number_is_labelled_with_its_name() {
        let chapter = |title: Option<&str>, text: &str| Chapter { title: title.map(str::to_owned), text: text.to_owned() };
        let numbered = chapter(Some("1"), "# 1\n\n## THE CHALLENGE OF THE FUTURE\n\nWHENEVER I INTERVIEW someone.\n");
        assert_eq!(numbered.label(2), "1 · The Challenge of the Future");
        let escaped = chapter(Some("IV"), "# IV\n\n## Party Like It’s 1999\\!\n\nText.\n");
        assert_eq!(escaped.label(0), "IV · Party Like It’s 1999!");
        assert_eq!(chapter(Some("Chapter 3"), "# Chapter 3\n\nIt was a truth.\n").label(2), "Chapter 3", "no name follows the number");
        assert_eq!(chapter(Some("7"), "# 7\n\nText.\n\n## A Later Section\n").label(0), "7", "a heading further on is not the name");
        assert_eq!(chapter(Some("Economy"), "# Economy\n\n## Notes\n").label(0), "Economy", "a named chapter keeps its name");
        assert_eq!(chapter(Some("CHAPTER I."), "# Chapter I\n\nText.\n").label(0), "Chapter I");
        assert_eq!(chapter(Some("CHAPTERXXVII."), "# ChapterXXVII\n\nText.\n").label(0), "Chapter XXVII");
        assert_eq!(chapter(None, "![A map](images/x/1.png)\n\n# SUPERINTELLIGENCE\n\nText.\n").label(0), "Superintelligence", "a heading after a picture still titles it");
        assert_eq!(chapter(None, "# THE END OF THE WORLD\n\nText.\n").label(4), "The End of the World", "the first heading when the contents gave no title");
        assert_eq!(chapter(None, "Text.\n").label(4), "Chapter 5");
    }

    #[test]
    fn a_place_follows_its_chapter_when_the_chapters_change() {
        let t = |s: &str| Some(s.to_string());
        let old = [t("One"), t("Two"), None, t("Two")];
        let new = [None, t("One"), t("Two"), t("Notes"), t("Two")];
        let at = |chapter| Position { chapter, fraction: 0.4 };
        assert_eq!(at(1).carried(&old, &new), at(2));
        assert_eq!(at(3).carried(&old, &new), at(4), "the second chapter of a repeated title");
        assert_eq!(at(2).carried(&old, &new), at(2), "an untitled chapter keeps its index");
        assert_eq!(at(3).carried(&old, &new[..2]), at(1), "clamped to the chapters there are");
    }
}

//! A web page saved by its address. Readability (`dom_smoothie`) finds the article in the page
//! and what the page says about it, and the EPUB walker writes it as Markdown (ADR 0006), so an
//! article reads, highlights, syncs and answers agents the way a book does.

use dom_smoothie::{Config, Readability, ReadabilityError};

use crate::book::{Book, BookId, Format, PARSER, Source};
use crate::epub;
use crate::import::{ImportError, Parsed, book_id, image_dir};

/// An article's identity: the hash of its address without the fragment, so saving the same page
/// twice finds the article already there.
pub fn article_id(url: &str) -> BookId {
    book_id(canonical(url).as_bytes())
}

fn canonical(url: &str) -> &str {
    let url = url.trim();
    url.split_once('#').map_or(url, |(page, _)| page)
}

/// The article in `html`, the page saved from `url`. Each picture it shows is fetched once
/// through `fetch`, by its absolute address; one that can't be fetched, or isn't a JPEG, PNG,
/// GIF or WebP, is left out, so reading never sends a request.
pub fn parse(url: &str, html: &str, fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Parsed, ImportError> {
    let url = canonical(url);
    // Readability drops every class but these, which mark footnotes and their markers.
    let classes = ["footnote", "footnotes", "endnote", "noteref", "fnanchor", "footnote-ref", "footnoteref", "footnote-link"];
    let config = Config { classes_to_preserve: classes.map(String::from).to_vec(), ..Config::default() };
    let mut readability = Readability::new(html, Some(url), Some(config)).map_err(|e| match e {
        ReadabilityError::BadDocumentURL => ImportError::BadArticle(format!("“{url}” isn’t a full web address")),
        _ => ImportError::BadArticle("Inkling found no article in it".into()),
    })?;
    let article = readability.parse().map_err(|_| ImportError::BadArticle("Inkling found no article in it".into()))?;
    let id = article_id(url);
    let title = tidy(&article.title).unwrap_or_else(|| host(url).to_string());
    // The article's title opens its one chapter, as a book's chapter opens with its own.
    let src = format!("<h1>{}</h1>{}", html_escape::encode_text(&title), &*article.content);
    let page = epub::page(&src, &format!("{}/", image_dir(&id)), fetch);
    if page.chapters.iter().all(|c| c.text.lines().filter(|l| !l.trim().is_empty()).count() < 2) {
        return Err(ImportError::BadArticle("it has no text to read".into()));
    }
    let source = Source {
        url: url.to_string(),
        site: article.site_name.as_deref().and_then(tidy),
        published: article.published_time.as_deref().and_then(tidy),
        excerpt: article.excerpt.as_deref().and_then(tidy),
    };
    let author = article.byline.as_deref().and_then(byline);
    let book = Book { id, title, author, format: Format::Article, parser: PARSER, chapters: page.chapters, source: Some(source) };
    Ok(Parsed { book, images: page.images })
}

fn tidy(s: &str) -> Option<String> {
    let s = epub::collapse(s);
    (!s.is_empty()).then_some(s)
}

/// "By Jane Doe" as "Jane Doe".
fn byline(raw: &str) -> Option<String> {
    let s = tidy(raw)?;
    let s = ["By ", "by ", "BY "].iter().find_map(|p| s.strip_prefix(p)).map_or(s.as_str(), str::trim);
    tidy(s)
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really";

    /// A page the way a site serves one: navigation, a sign-up box, the article and its comments.
    pub(crate) fn page() -> String {
        let words = "The tide came in slowly over the flats, and the birds lifted as it reached them. ".repeat(6);
        format!(
            r##"<!doctype html><html lang="en"><head>
            <title>Notes on the tide | The Shore</title>
            <meta property="og:title" content="Notes on the tide">
            <meta property="og:site_name" content="The Shore">
            <meta name="author" content="By Ana Lima">
            <meta property="article:published_time" content="2026-11-20T08:00:00Z">
            <meta name="description" content="What the flats do at dawn.">
            <script>track()</script><style>p {{ color: red }}</style>
            </head><body>
            <nav><a href="/">Home</a> <a href="/about">About</a></nav>
            <div class="newsletter-signup"><p>Subscribe to The Shore for more!</p></div>
            <article>
              <h1>Notes on the tide</h1>
              <p>{words}It is <em>never</em> the same twice, as <a href="/essays/salt">an earlier essay</a> said.<sup><a href="#fn1" class="footnote-ref">1</a></sup></p>
              <p>{words}</p>
              <h2>At dawn</h2>
              <figure><img src="/img/flats.png" alt="The flats at dawn"><figcaption>The flats, 6 a.m.</figcaption></figure>
              <ul><li>Curlews</li><li>Dunlin &amp; knot</li></ul>
              <blockquote><p>The sea is the same as it was.</p></blockquote>
              <p>{words}</p>
              <div class="footnotes"><ol><li id="fn1" class="footnote">Counted on the 19th.</li></ol></div>
            </article>
            <section id="comments"><h3>Comments</h3><p>First!</p></section>
            <footer>© The Shore</footer>
            </body></html>"##
        )
    }

    #[test]
    fn a_page_becomes_one_chapter_of_markdown_with_where_it_came_from() {
        let mut fetched = vec![];
        let mut fetch = |url: &str| {
            fetched.push(url.to_string());
            Some(PNG.to_vec())
        };
        let parsed = parse("https://shore.example/2026/tide#comments", &page(), &mut fetch).unwrap();
        let book = &parsed.book;
        assert_eq!(book.format, Format::Article);
        assert_eq!(book.id, article_id("https://shore.example/2026/tide"), "the fragment is not part of the article");
        assert_eq!(book.title, "Notes on the tide");
        assert_eq!(book.author.as_deref(), Some("Ana Lima"));
        let source = book.source.as_ref().unwrap();
        assert_eq!(source.url, "https://shore.example/2026/tide");
        assert_eq!(source.site.as_deref(), Some("The Shore"));
        assert_eq!(source.published.as_deref(), Some("2026-11-20T08:00:00Z"));
        assert_eq!(source.excerpt.as_deref(), Some("What the flats do at dawn."));

        assert_eq!(book.chapters.len(), 1);
        let text = &book.chapters[0].text;
        assert!(text.starts_with("# Notes on the tide\n\n"), "{text}");
        assert_eq!(text.matches("Notes on the tide").count(), 1, "the title is written once: {text}");
        assert!(text.contains("It is *never* the same twice"), "{text}");
        assert!(text.contains("[an earlier essay](https://shore.example/essays/salt)"), "a link out is absolute: {text}");
        assert!(text.contains("[^1]") && text.contains("[^1]: Counted on the 19th."), "the footnote is a note: {text}");
        assert!(text.contains("## At dawn"), "{text}");
        let image = format!("![The flats at dawn]({}/1.png \"The flats, 6 a.m.\")", image_dir(&book.id));
        assert!(text.contains(&image), "the picture is local, with its caption: {text}");
        assert!(text.contains("- Curlews\n- Dunlin & knot"), "{text}");
        assert!(text.contains("> The sea is the same as it was."), "{text}");
        for furniture in ["Home", "Subscribe", "First!", "©", "track()", "color: red"] {
            assert!(!text.contains(furniture), "{furniture:?} is page furniture: {text}");
        }
        assert_eq!(fetched, ["https://shore.example/img/flats.png"], "each picture is fetched once, by its full address");
        assert_eq!(parsed.images, [("1.png".to_string(), PNG.to_vec())]);
    }

    #[test]
    fn a_picture_that_cant_be_fetched_is_left_out() {
        let parsed = parse("https://shore.example/2026/tide", &page(), &mut |_| None).unwrap();
        assert!(!parsed.book.chapters[0].text.contains("!["));
        assert!(parsed.images.is_empty());
    }

    #[test]
    fn a_page_with_nothing_to_read_or_no_address_is_refused_in_plain_words() {
        let empty = parse("https://shore.example/", "<html><body><nav>Home</nav></body></html>", &mut |_| None).unwrap_err();
        assert!(matches!(empty, ImportError::BadArticle(_)), "{empty:?}");
        let relative = parse("/2026/tide", &page(), &mut |_| None).unwrap_err();
        assert_eq!(relative.to_string(), "This page couldn’t be saved as an article: “/2026/tide” isn’t a full web address.");
    }
}

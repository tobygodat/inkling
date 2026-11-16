//! `cargo run -p inkling-books --example walden -- <out.epub>` writes the small EPUB the snap
//! scripts add: a chapter of Walden with italics, a picture with its caption, a footnote, a list,
//! a quote, a link to the next chapter, and text that looks like Markdown.

use std::io::Write;

const PAGE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Where I Lived</title></head><body>
<h1>Where I Lived, and What I Lived For</h1>
<p>I went to the woods because I wished to live <em>deliberately</em>, to front only the essential facts of life, and see if I could not learn what it had to teach.<a epub:type="noteref" href="notes.xhtml#n1" id="r1">1</a></p>
<figure><img src="pond.png" alt="Walden Pond"/><figcaption>The pond in winter, from the first edition.</figcaption></figure>
<p>Simplicity, simplicity, simplicity! I say, let your affairs be as two or three, and not a hundred or a thousand; instead of a million count half a dozen, and keep your accounts on your thumb-nail.</p>
<ul><li>Two or three affairs,</li><li>half a dozen accounts,</li><li>and one thumb-nail.</li></ul>
<blockquote><p>Our life is frittered away by detail.</p></blockquote>
<p># 1. *Not* a heading: a line that starts as Markdown would, with 50_000 and $5 in it.</p>
<p>The <a href="economy.xhtml">next chapter</a> counts the cost, and <a href="https://www.gutenberg.org/ebooks/205">Project Gutenberg</a> has the rest.</p>
</body></html>"##;

const NEXT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Economy</title></head><body>
<h1>Economy</h1>
<p>When I wrote the following pages, or rather the bulk of them, I lived alone, in the woods, a mile from any neighbor, in a house which I had built myself, on the shore of Walden Pond, in Concord, Massachusetts, and earned my living by the labor of my hands only.</p>
</body></html>"#;

const NOTES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Notes</title></head><body>
<aside epub:type="footnote" id="n1"><p><a href="lived.xhtml#r1">1</a> Written at the pond in the spring of 1846.</p></aside>
</body></html>"#;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="id">inkling-walden-fixture</dc:identifier><dc:title>Walden</dc:title><dc:creator>Henry David Thoreau</dc:creator><dc:language>en</dc:language></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="lived" href="lived.xhtml" media-type="application/xhtml+xml"/><item id="economy" href="economy.xhtml" media-type="application/xhtml+xml"/>
<item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/><item id="pond" href="pond.png" media-type="image/png"/></manifest>
<spine><itemref idref="lived"/><itemref idref="economy"/><itemref idref="notes"/></spine></package>"#;

const NAV: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body>
<nav epub:type="toc"><ol><li><a href="lived.xhtml">Where I Lived</a></li><li><a href="economy.xhtml">Economy</a></li></ol></nav></body></html>"#;

fn main() {
    let out = std::env::args().nth(1).expect("the EPUB to write");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out).expect("a file to write"));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, bytes: &[u8]| {
        zip.start_file(name, stored).expect("a zip entry");
        zip.write_all(bytes).expect("written");
    };
    put("mimetype", b"application/epub+zip");
    put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
    put("OPS/book.opf", OPF.as_bytes());
    put("OPS/nav.xhtml", NAV.as_bytes());
    put("OPS/lived.xhtml", PAGE.as_bytes());
    put("OPS/economy.xhtml", NEXT.as_bytes());
    put("OPS/notes.xhtml", NOTES.as_bytes());
    put("OPS/pond.png", include_bytes!("walden/pond.png"));
    zip.finish().expect("the EPUB is written");
}

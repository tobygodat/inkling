import Foundation
import Testing

@testable import InklingUI

/// A chapter as `inkling-books` writes one (ADR 0006), with every kind of block in it.
let chapter = """
# Notes on the tide

The tide came in *slowly* over the flats,\\
and the birds lifted[^1] as it reached [them](https://shore.example/birds).

## The flats

> Nothing moves out there
> but the water.
>
> And the light.

- Mud
- Sand, which
  shifts
  - Shells
1. First
2. Second

![The flats at dusk](images/abc/1.jpg "Low water, October")

* * *

```
let tide = in
```

[^1]: Gulls, mostly.
"""

@Test func everyKindOfBlockInAChapterComesThroughInOrder() {
    let blocks = Blocks.parse(chapter)
    #expect(blocks.map(\.kind) == [
        .heading(level: 1),
        .paragraph,
        .heading(level: 2),
        .quote,
        .item(marker: "•", depth: 0),
        .item(marker: "•", depth: 0),
        .item(marker: "•", depth: 1),
        .item(marker: "1.", depth: 0),
        .item(marker: "2.", depth: 0),
        .image(path: "images/abc/1.jpg", alt: "The flats at dusk", caption: "Low water, October"),
        .rule,
        .code,
        .note(label: "1"),
    ])
    #expect(blocks[0].text == "Notes on the tide")
    #expect(blocks[1].text == "The tide came in *slowly* over the flats,\nand the birds lifted[^1] as it reached [them](https://shore.example/birds).")
    #expect(blocks[3].text == "Nothing moves out there but the water.\n\nAnd the light.")
    #expect(blocks[5].text == "Sand, which shifts")
    #expect(blocks[11].text == "let tide = in")
    #expect(blocks[12].text == "Gulls, mostly.")
}

@Test func eachBlockStartsAtItsOwnByteInTheChapter() {
    let bytes = Array(chapter.utf8)
    for block in Blocks.parse(chapter) {
        let rest = String(decoding: bytes[block.start...], as: UTF8.self)
        let opening: String
        switch block.kind {
        case .heading(let level): opening = String(repeating: "#", count: level) + " "
        case .quote: opening = "> "
        case .item(let marker, _): opening = marker == "•" ? "- " : marker + " "
        case .image: opening = "!["
        case .rule: opening = "* * *"
        case .code: opening = "```"
        case .note(let label): opening = "[^\(label)]:"
        case .paragraph: opening = "The tide"
        }
        #expect(rest.drop { $0 == " " }.hasPrefix(opening), "\(block.kind) at \(block.start)")
    }
    // Offsets count bytes, not characters, so a chapter with accents keeps them right.
    let accented = "# Café\n\nÉté au bord de l’eau.\n\nFin."
    let starts = Blocks.parse(accented).map(\.start)
    #expect(starts == [0, "# Café\n\n".utf8.count, "# Café\n\nÉté au bord de l’eau.\n\n".utf8.count])
}

@Test func aLineThatOnlyLooksLikeAMarkIsAParagraph() {
    let kinds = Blocks.parse("#hashtag at the start\n\n12 o’clock came.\n\n![not alone](a.png) and more").map(\.kind)
    #expect(kinds == [.paragraph, .paragraph, .paragraph])
}

@Test func aFootnoteMarkReadsAsItsLabelAndAnEscapedOneStaysText() {
    #expect(Inline.plain("The birds *lifted*[^1] and [flew](https://shore.example).") == "The birds lifted1 and flew.")
    #expect(Inline.plain("Printed \\[^2] in the book, and a real one[^note].") == "Printed [^2] in the book, and a real onenote.")
}

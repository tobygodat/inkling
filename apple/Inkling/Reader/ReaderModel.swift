import Foundation
import InklingCore
import Observation

/// One open item: its text as drawn, its highlights and the reader's place, kept through the
/// bridge as they change.
@Observable
final class ReaderModel {
    let item: Item
    let rendered: Rendered
    private(set) var highlights: [Passage]
    /// The chapter and how far through it, as the reader scrolls.
    private(set) var chapter: Int
    private(set) var fraction: Double
    /// Where the text should scroll to once, on opening or when a highlight is chosen.
    var jump: Int?
    /// The text selected on the Mac, for Highlight in the menu bar.
    var selection = NSRange()
    /// Whether the bars are out of the way: on iPhone they hide as the reader scrolls on and come
    /// back on a scroll back or a tap.
    var chromeHidden = false
    private let shelf: Shelf

    init(item: Item, rendered: Rendered, shelf: Shelf) {
        self.item = item
        self.rendered = rendered
        self.shelf = shelf
        highlights = shelf.highlights(item.id)
        chapter = min(Int(item.chapter), max(rendered.chapters.count - 1, 0))
        fraction = Double(item.fraction)
        jump = rendered.chapters.isEmpty ? nil : character(chapter: chapter, fraction: fraction)
    }

    var progress: Double {
        let chapters = max(rendered.chapters.count, 1)
        return min(1, (Double(chapter) + fraction) / Double(chapters))
    }

    var minutesLeft: Int? {
        item.minutes.map { max(0, Int((Double($0) * (1 - progress)).rounded(.up))) }
    }

    var highlightRanges: [NSRange] { highlights.compactMap(rendered.range(of:)) }

    /// The reader's place from the first character on screen; `atEnd` when the text's end shows.
    func scrolled(toCharacter character: Int, atEnd: Bool) {
        let chapter = rendered.chapter(at: character)
        let span = rendered.chapters[chapter]
        let last = rendered.chapters.count - 1
        let fraction = atEnd && chapter >= last - 1 ? 1 : min(1, max(0, Double(character - span.location) / Double(max(span.length, 1))))
        let (chapterNow, fractionNow) = atEnd ? (last, 1.0) : (chapter, fraction)
        guard chapterNow != self.chapter || abs(fractionNow - self.fraction) > 0.002 else { return }
        self.chapter = chapterNow
        self.fraction = fractionNow
        shelf.keepPlace(item.id, chapter: chapterNow, fraction: fractionNow)
    }

    func character(chapter: Int, fraction: Double) -> Int {
        let span = rendered.chapters[chapter]
        return span.location + Int(Double(span.length) * min(max(fraction, 0), 1))
    }

    /// The highlight that covers the whole of `selection`, if one does.
    func highlight(covering selection: NSRange) -> Passage? {
        highlights.first { passage in
            rendered.range(of: passage).map { NSIntersectionRange($0, selection).length == selection.length && selection.length > 0 } ?? false
        }
    }

    func highlight(_ selection: NSRange) {
        let made = UInt64(Date().timeIntervalSince1970 * 1000)
        guard let passage = rendered.passage(for: selection, made: made) else { return }
        var all = highlights.filter { $0.chapter != passage.chapter || $0.end <= passage.start || $0.start >= passage.end }
        all.append(passage)
        all.sort { ($0.chapter, $0.start) < ($1.chapter, $1.start) }
        highlights = all
        shelf.setHighlights(item.id, all)
    }

    /// Highlights the selection, or removes the highlight it is inside.
    func toggleHighlight() {
        guard selection.length > 0 else { return }
        if let passage = highlight(covering: selection) { remove(passage) } else { highlight(selection) }
    }

    func remove(_ passage: Passage) {
        highlights.removeAll { $0 == passage }
        shelf.setHighlights(item.id, highlights)
    }

    func go(to passage: Passage) {
        jump = rendered.range(of: passage)?.location
    }
}

import Foundation
import InklingCore
import Observation

/// One open item: its text as drawn, its highlights and the reader's place, kept through the
/// bridge as they change.
@Observable
final class ReaderModel {
    let item: Item
    private(set) var rendered: Rendered
    private(set) var highlights: [Passage]
    /// The chapter and how far through it, as the reader scrolls.
    private(set) var chapter: Int
    private(set) var fraction: Double
    /// The formatted place shown in the bar. Stored so the bar only redraws when its words change,
    /// not on every scrolled frame.
    private(set) var placeText: String
    /// Where the text should scroll to once, on opening or when a highlight is chosen.
    var jump: Int?
    /// How far down its paragraph the line at the top was, when `jump` restores a place exactly.
    var jumpWithin: CGFloat = 0
    /// The line at the top of the column as the reader left it: what a redraw in a new look opens
    /// at, so changing the paper or the size never moves the text.
    @ObservationIgnored var anchor: Anchor?
    /// Draws the text as it is on screen, for the new paper to spread over.
    @ObservationIgnored var snapshot: (() -> PlatformImage?)?
    /// Counts the times a finger lands on the text, which hurries a paper still spreading.
    var touches = 0
    /// Whether the text stands at its place yet, which a new paper waits for before it spreads.
    var ready = false
    /// How the text gets to `jump`.
    var motion = Motion.instant
    /// The text selected, for Highlight in the menu bar and for the Ask bar.
    var selection = NSRange()
    /// Whether the bars are out of the way: on iPhone they hide as the reader scrolls on and come
    /// back on a scroll back or a tap.
    var chromeHidden = false
    /// Archives the item and leaves it, from "Done" at its end.
    @ObservationIgnored var done: (() -> Void)?
    private let shelf: Shelf

    init(item: Item, rendered: Rendered, shelf: Shelf, at anchor: Anchor? = nil) {
        self.item = item
        self.rendered = rendered
        self.shelf = shelf
        highlights = shelf.highlights(item.id)
        let saved = shelf.pendingPlace(for: item.id)
        let initialChapter = min(saved?.chapter ?? Int(item.chapter), max(rendered.chapters.count - 1, 0))
        let initialFraction = saved?.fraction ?? Double(item.fraction)
        chapter = initialChapter
        fraction = initialFraction
        placeText = Self.formatPlace(item: item, titles: rendered.titles, chapter: initialChapter, fraction: initialFraction)
        jump = rendered.chapters.isEmpty ? nil : anchor?.character ?? character(chapter: initialChapter, fraction: initialFraction)
        jumpWithin = anchor?.within ?? 0
        self.anchor = anchor
        jump.map(rendered.preparePictures(around:))
    }

    /// The same text drawn in a new paper, which the text on screen takes without moving.
    func recolour(_ rendered: Rendered) {
        self.rendered = rendered
    }

    var progress: Double {
        Self.progress(chapter: chapter, chapters: rendered.chapters.count, fraction: fraction)
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
        let words = Self.formatPlace(item: item, titles: rendered.titles, chapter: chapterNow, fraction: fractionNow)
        if words != placeText { placeText = words }
        shelf.keepPlace(item.id, chapter: chapterNow, fraction: fractionNow)
    }

    private static func progress(chapter: Int, chapters: Int, fraction: Double) -> Double {
        min(1, (Double(chapter) + fraction) / Double(max(chapters, 1)))
    }

    private static func formatPlace(item: Item, titles: [String], chapter: Int, fraction: Double) -> String {
        let progress = Self.progress(chapter: chapter, chapters: titles.count, fraction: fraction)
        let percent = "\(Int((progress * 100).rounded()))%"
        if item.kind == .article {
            guard let minutes = item.minutes, progress < 0.98 else { return "Finished" }
            let left = max(0, Int((Double(minutes) * (1 - progress)).rounded(.up)))
            return left == 0 ? percent : "\(left) min left"
        }
        return titles.indices.contains(chapter) ? "\(titles[chapter]) · \(percent)" : percent
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
        guard let location = rendered.range(of: passage)?.location else { return }
        motion = .fade
        jump = location
    }

    var chapters: Int { rendered.chapters.count }

    /// Goes to the start of chapter `index`, if the book has one.
    func go(toChapter index: Int, _ motion: Motion) {
        guard rendered.chapters.indices.contains(index) else { return }
        self.motion = motion
        jump = rendered.chapters[index].location
    }

    /// How the text moves to a place. A keyboard command jumps, because it is pressed often and
    /// any motion would make it feel slow. "Next: …" glides, because the next chapter is just
    /// below. A chapter chosen from Contents dips out and in, rising from the direction it is in,
    /// because scrolling through pages to get there would only blur. Reduce Motion keeps only the
    /// dip's fade.
    enum Motion {
        case instant, glide, fade
    }

    /// A line of the text: the paragraph it is in, by its first character, and how far down the
    /// paragraph, as a share of its height so it holds in a new size too.
    struct Anchor: Equatable {
        var character: Int
        var within: CGFloat
    }
}

import Foundation
import ImageIO
import InklingCore
import Markdown
import os

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// A book or an article drawn as one attributed text, every chapter in turn, with a map back to
/// each chapter's Markdown in bytes: a highlight is kept as bytes (ADR 0006), so the GPUI app and
/// this one find the same passage.
nonisolated struct Rendered: @unchecked Sendable {
    let text: NSAttributedString
    /// Each chapter's characters in `text`, in UTF-16 units.
    let chapters: [NSRange]
    /// What each chapter is called in Contents, the place and "Next: …": core's `label`.
    let titles: [String]
    /// The runs of text that come straight from the Markdown, in order.
    let runs: [Run]
    /// The pictures, in the order they come.
    var pictures: [Picture] = []

    struct Run {
        let chapter: Int
        let rendered: NSRange
        let source: Range<Int>
        let text: String

        /// The byte in the chapter's Markdown of the character `offset` UTF-16 units into the run.
        func byte(at offset: Int) -> Int {
            let offset = min(max(offset, 0), rendered.length)
            guard source.count == text.utf8.count else {
                return source.lowerBound + Int((Double(offset) / Double(max(rendered.length, 1)) * Double(source.count)).rounded())
            }
            let index = text.utf16.index(text.startIndex, offsetBy: offset, limitedBy: text.endIndex) ?? text.endIndex
            return source.lowerBound + text.utf8.distance(from: text.startIndex, to: index)
        }

        /// The UTF-16 offset into the run of the byte `byte` of the chapter's Markdown.
        func offset(of byte: Int) -> Int {
            let bytes = min(max(byte - source.lowerBound, 0), source.count)
            guard source.count == text.utf8.count else {
                return Int((Double(bytes) / Double(max(source.count, 1)) * Double(rendered.length)).rounded())
            }
            let index = text.utf8.index(text.startIndex, offsetBy: bytes, limitedBy: text.endIndex) ?? text.endIndex
            return text.utf16.distance(from: text.startIndex, to: index)
        }
    }

    func chapter(at character: Int) -> Int {
        guard !chapters.isEmpty else { return 0 }
        var low = 0, high = chapters.count
        while low + 1 < high {
            let middle = (low + high) / 2
            if chapters[middle].location <= character { low = middle } else { high = middle }
        }
        return low
    }

    /// The passage a selection covers, kept to the chapter it starts in.
    func passage(for selection: NSRange, made: UInt64) -> Passage? {
        let chapter = chapter(at: selection.location)
        let end = min(NSMaxRange(selection), NSMaxRange(chapters[chapter]))
        let covered = runs.filter { $0.chapter == chapter && $0.rendered.location < end && NSMaxRange($0.rendered) > selection.location }
        guard let first = covered.first, let last = covered.last else { return nil }
        let start = first.byte(at: selection.location - first.rendered.location)
        let stop = last.byte(at: end - last.rendered.location)
        guard stop > start else { return nil }
        let words = (text.string as NSString).substring(with: NSRange(location: selection.location, length: end - selection.location))
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return Passage(chapter: UInt32(chapter), start: UInt64(start), end: UInt64(stop), words: words, made: made)
    }

    /// Where a highlighted passage is drawn, or nil when its chapter changed under it.
    func range(of passage: Passage) -> NSRange? {
        let (start, end) = (Int(passage.start), Int(passage.end))
        let covered = runs.filter { $0.chapter == Int(passage.chapter) && $0.source.lowerBound < end && $0.source.upperBound > start }
        guard let first = covered.first, let last = covered.last else { return nil }
        let from = first.rendered.location + first.offset(of: start)
        let to = last.rendered.location + last.offset(of: end)
        return to > from ? NSRange(location: from, length: to - from) : nil
    }
}

/// How the text is set: the look's face, size and spacing, the palette, the column's width, and
/// the screen's pixels to a point, which pictures are decoded for.
nonisolated struct Typesetting: @unchecked Sendable {
    var face: String
    var size: CGFloat
    var lineHeight: CGFloat
    var titleSize: CGFloat
    var palette: Palette
    var column: CGFloat
    var scale: CGFloat = 2
}

/// Draws Markdown chapters (ADR 0006) for the reader. Runs off the main thread.
nonisolated struct Renderer {
    let set: Typesetting
    let folder: URL
    /// The line under an article's title: its author, its site and how long it takes to read.
    let byline: String?

    private var out = NSMutableAttributedString()
    private var runs: [Rendered.Run] = []
    private var pictures: [Picture] = []
    private var fontCache: [FontKey: PlatformFont] = [:]
    private var chapter = 0
    private var lines: [Int] = [0]
    private var titled = false
    /// The footnote definitions drawn so far in this chapter.
    private var notes = 0
    /// Bytes the next run of text leaves out: a footnote definition's `[^n]: `.
    private var dropping = 0

    private struct FontKey: Hashable {
        let size: CGFloat
        let bold: Bool
        let italic: Bool
    }

    init(set: Typesetting, folder: URL, byline: String?) {
        self.set = set
        self.folder = folder
        self.byline = byline
    }

    static func render(_ book: Book, set: Typesetting, folder: URL, byline: String?) -> Rendered {
        var renderer = Renderer(set: set, folder: folder, byline: byline)
        let titles = book.chapters.map(\.label)
        var spans: [NSRange] = []
        for (index, chapter) in book.chapters.enumerated() {
            let start = renderer.out.length
            renderer.chapter(index, chapter, in: book, next: titles.indices.contains(index + 1) ? titles[index + 1] : nil)
            spans.append(NSRange(location: start, length: renderer.out.length - start))
        }
        return Rendered(text: renderer.out, chapters: spans, titles: titles, runs: renderer.runs, pictures: renderer.pictures)
    }

    /// The link "Next: …" carries at a chapter's end.
    static func link(toChapter index: Int) -> URL {
        URL(string: "inkling-chapter:\(index)")!
    }

    static func chapter(of link: URL) -> Int? {
        link.scheme == "inkling-chapter" ? Int(link.absoluteString.dropFirst("inkling-chapter:".count)) : nil
    }

    /// The link "Done" carries at the end: it archives what was read.
    static let done = URL(string: "inkling-done:")!

    /// Whether a link is one the reader follows itself rather than one out of the book.
    static func isOwn(_ link: URL) -> Bool {
        chapter(of: link) != nil || link == done
    }

    /// Each footnote mark `[^n]`, which CommonMark reads as text or as a link, with its brackets
    /// swapped for two control characters it leaves alone. Every byte stays where it was, so a
    /// highlight still addresses the chapter's own Markdown. A mark the book escaped stays text.
    static func marking(_ markdown: String) -> String {
        guard markdown.contains("[^") else { return markdown }
        return markdown.replacing(#/(\\?)\[\^([^\]\s]+)\]/#) { match in
            match.output.1.isEmpty ? "\(Self.markOpen)^\(match.output.2)\(Self.markClose)" : String(match.output.0)
        }
    }

    private static let markOpen = "\u{1}"
    private static let markClose = "\u{2}"

    private mutating func chapter(_ index: Int, _ chapter: Chapter, in book: Book, next: String?) {
        self.chapter = index
        notes = 0
        let markdown = Self.marking(chapter.text)
        lines = [0]
        for (offset, byte) in markdown.utf8.enumerated() where byte == 0x0A { lines.append(offset + 1) }
        let document = Document(parsing: markdown)
        let opensWithHeading = document.child(at: 0) is Heading
        if !opensWithHeading, let title = chapter.title, book.chapters.count > 1 {
            append(title + "\n", attributes(font: font(size: set.size * 1.35, weight: .semibold), color: set.palette.ink, paragraph: paragraph(before: 0, after: set.size * 0.9, lineHeight: 1.25)))
        }
        for block in document.children { self.block(block, indent: 0) }
        if let next { nextLine(next, chapter: index + 1) } else if book.kind == .article { doneLine() }
    }

    /// "Done" at an article's end, centred in the UI face like "Next: …": it archives the article. A
    /// book ends without it, because finishing a book is no reason to put it away.
    private mutating func doneLine() {
        let style = paragraph(align: .center, before: set.size * 2.2, after: set.size * 3.6, lineHeight: 1.5)
        var label = attributes(font: interface(size: max(13, set.size * 0.7), weight: .semibold), color: set.palette.ink, paragraph: style)
        label[.link] = Self.done
        append("Done\n", label)
    }

    /// "Next: …" at a chapter's end, centred in the UI face: the way on, and the break between
    /// chapters.
    private mutating func nextLine(_ title: String, chapter: Int) {
        let style = paragraph(align: .center, before: set.size * 2.2, after: set.size * 3.6, lineHeight: 1.5)
        let size = max(13, set.size * 0.7)
        var label = attributes(font: interface(size: size, weight: .medium), color: set.palette.muted, paragraph: style)
        label[.link] = Self.link(toChapter: chapter)
        append("Next: ", label)
        label[.foregroundColor] = set.palette.ink
        append(title, label)
        label[.foregroundColor] = set.palette.muted
        append("  \u{2192}\n", label)
    }

    // MARK: Blocks

    private mutating func block(_ markup: Markup, indent: CGFloat, quoted: Bool = false, marker: String? = nil) {
        switch markup {
        case let heading as Heading:
            let isTitle = heading.level == 1 && !titled
            let scale: CGFloat = isTitle ? set.titleSize / set.size : [1.45, 1.25, 1.12][min(heading.level, 3) - 1]
            let base = Style(size: set.size * scale, weight: .semibold, color: set.palette.ink)
            let style = paragraph(indent: indent, before: isTitle ? 0 : set.size * 1.5, after: isTitle ? set.size * 0.55 : set.size * 0.6, lineHeight: isTitle ? 1.18 : 1.3)
            inlines(heading, base, style)
            if isTitle {
                titled = true
                if let byline {
                    append(byline + "\n", attributes(font: interface(size: max(13, set.size * 0.68)), color: set.palette.muted, paragraph: paragraph(before: 0, after: set.size * 1.9, lineHeight: 1.4)))
                }
            }
        case let paragraph as Paragraph:
            if let first = paragraph.child(at: 0) as? Markdown.Text, let note = Self.note(first.string) {
                footnote(paragraph, label: note.label, prefix: note.prefix, indent: indent)
                return
            }
            if let image = paragraph.children.compactMap({ $0 as? Markdown.Image }).first, paragraph.childCount == 1 {
                figure(image, indent: indent)
                return
            }
            var style = self.paragraph(indent: indent, before: 0, after: set.size * 0.85)
            if let marker {
                style = self.paragraph(indent: indent, before: 0, after: set.size * 0.45, hanging: set.size * 1.4)
                append(marker + "\t", attributes(font: font(size: set.size), color: set.palette.muted, paragraph: style))
            }
            inlines(paragraph, Style(size: set.size, italic: quoted, color: quoted ? set.palette.muted : set.palette.ink), style)
        case let quote as BlockQuote:
            for child in quote.children { block(child, indent: indent + set.size * 1.2, quoted: true) }
        case let list as UnorderedList:
            for item in list.listItems { listItem(item, indent: indent, marker: "•") }
        case let list as OrderedList:
            for (n, item) in list.listItems.enumerated() { listItem(item, indent: indent, marker: "\(Int(list.startIndex) + n).") }
        case let code as CodeBlock:
            let mono = PlatformFont.monospacedSystemFont(ofSize: set.size * 0.78, weight: .regular)
            let text = code.code.hasSuffix("\n") ? code.code : code.code + "\n"
            var attrs = attributes(font: mono, color: set.palette.ink, paragraph: paragraph(indent: indent + set.size * 0.8, before: 0, after: set.size * 0.85, lineHeight: 1.45))
            attrs[.backgroundColor] = set.palette.segment
            append(text, attrs)
        case is ThematicBreak:
            append("*\u{2003}*\u{2003}*\n", attributes(font: font(size: set.size), color: set.palette.muted, paragraph: paragraph(align: .center, before: set.size * 0.6, after: set.size * 1.4)))
        case let table as Markdown.Table:
            let style = paragraph(indent: indent, before: 0, after: set.size * 0.3)
            for row in [Array(table.head.cells)] + table.body.rows.map({ Array($0.cells) }) {
                for (n, cell) in row.enumerated() {
                    if n > 0 { append("\t", attributes(font: font(size: set.size * 0.9), color: set.palette.ink, paragraph: style)) }
                    inlines(cell, Style(size: set.size * 0.9, weight: row.first?.parent is Markdown.Table.Head ? .semibold : .regular, color: set.palette.ink), style, newline: false)
                }
                append("\n", attributes(font: font(size: set.size * 0.9), color: set.palette.ink, paragraph: style))
            }
        case is HTMLBlock:
            break
        default:
            for child in markup.children { block(child, indent: indent, quoted: quoted) }
        }
    }

    /// A footnote's definition, `[^n]: …`: its label in Link and its words in muted ink, a size
    /// down, hanging from the label.
    private mutating func footnote(_ paragraph: Paragraph, label: String, prefix: Int, indent: CGFloat) {
        let size = set.size * 0.82
        let style = self.paragraph(indent: indent, before: notes == 0 ? set.size * 1.6 : 0, after: set.size * 0.5, lineHeight: set.lineHeight * 0.92, hanging: size * 1.6, size: size)
        notes += 1
        append(label + "\t", attributes(font: font(size: size), color: set.palette.link, paragraph: style))
        dropping = prefix
        inlines(paragraph, Style(size: size, color: set.palette.muted), style)
    }

    /// The label of a footnote's definition and how many bytes its `[^n]: ` takes.
    private static func note(_ text: String) -> (label: String, prefix: Int)? {
        guard let match = text.prefixMatch(of: #/\x{1}\^([^\x{2}]+)\x{2}:[ \t]*/#) else { return nil }
        return (String(match.output.1), text[match.range].utf8.count)
    }

    private mutating func listItem(_ item: ListItem, indent: CGFloat, marker: String) {
        for (n, child) in item.children.enumerated() {
            if child is Paragraph {
                block(child, indent: indent, marker: n == 0 ? marker : nil)
            } else {
                block(child, indent: indent + set.size * 1.4)
            }
        }
    }

    /// A picture centred in the column, no wider than its file, with its caption under it.
    private mutating func figure(_ image: Markdown.Image, indent: CGFloat) {
        let centred = paragraph(align: .center, before: set.size * 0.4, after: set.size * 0.5, lineHeight: 1)
        if let source = image.source, let picture = Picture(at: folder.appending(path: source), location: out.length, column: set.column, scale: set.scale) {
            pictures.append(picture)
            let attachment = FittedAttachment()
            attachment.picture = picture
            attachment.natural = picture.size
            let picture = NSMutableAttributedString(attachment: attachment)
            picture.addAttributes(attributes(font: font(size: set.size), color: set.palette.ink, paragraph: centred), range: NSRange(location: 0, length: picture.length))
            out.append(picture)
            append("\n", attributes(font: font(size: set.size), color: set.palette.ink, paragraph: centred))
        }
        let caption = image.plainText
        if !caption.isEmpty {
            append(caption + "\n", attributes(font: font(size: set.size * 0.78, italic: true), color: set.palette.muted, paragraph: paragraph(align: .center, before: 0, after: set.size * 1.1, lineHeight: 1.4)))
        }
    }

    // MARK: Inlines

    private struct Style {
        var size: CGFloat
        var weight: PlatformFont.Weight = .regular
        var italic = false
        var code = false
        var link = false
        var strike = false
        var color: PlatformColor
    }

    private mutating func inlines(_ container: Markup, _ style: Style, _ paragraph: NSParagraphStyle, newline: Bool = true) {
        for child in container.children { inline(child, style, paragraph) }
        if newline { append("\n", attributes(font: font(size: style.size), color: style.color, paragraph: paragraph)) }
    }

    private mutating func inline(_ markup: Markup, _ style: Style, _ paragraph: NSParagraphStyle) {
        var style = style
        switch markup {
        case let text as Markdown.Text:
            marked(text, style, paragraph)
        case is SoftBreak:
            append(" ", attributes(style, paragraph))
        case is LineBreak:
            append("\u{2028}", attributes(style, paragraph))
        case let code as InlineCode:
            style.code = true
            append(code.code, attributes(style, paragraph))
        case is Emphasis:
            style.italic.toggle()
            for child in markup.children { inline(child, style, paragraph) }
        case is Strong:
            style.weight = .semibold
            for child in markup.children { inline(child, style, paragraph) }
        case is Strikethrough:
            style.strike = true
            for child in markup.children { inline(child, style, paragraph) }
        case is Markdown.Link:
            style.link = true
            style.color = set.palette.link
            for child in markup.children { inline(child, style, paragraph) }
        case let image as Markdown.Image:
            append(image.plainText, attributes(style, paragraph))
        case is InlineHTML:
            break
        default:
            for child in markup.children { inline(child, style, paragraph) }
        }
    }

    /// A run of text, its footnote marks raised: each one's label, smaller, in Link.
    private mutating func marked(_ text: Markdown.Text, _ style: Style, _ paragraph: NSParagraphStyle) {
        var string = text.string
        var bytes = text.range.flatMap { range in byte(range.lowerBound).flatMap { lower in byte(range.upperBound).map { lower..<max($0, lower) } } }
        if bytes?.count != string.utf8.count, string.contains(Self.markOpen) || dropping > 0 { bytes = nil }
        if dropping > 0 {
            let drop = String(decoding: string.utf8.prefix(dropping), as: UTF8.self)
            string.removeFirst(drop.count)
            bytes = bytes.map { $0.lowerBound + dropping..<$0.upperBound }
            dropping = 0
        }
        guard string.contains(Self.markOpen) else {
            append(string, attributes(style, paragraph), bytes: bytes)
            return
        }
        var at = string.startIndex
        for match in string.matches(of: #/\x{1}\^([^\x{2}]+)\x{2}/#) {
            let words = String(string[at..<match.range.lowerBound])
            let offset = string.utf8.distance(from: string.startIndex, to: at)
            if !words.isEmpty { append(words, attributes(style, paragraph), bytes: bytes.map { $0.lowerBound + offset..<$0.lowerBound + offset + words.utf8.count }) }
            var mark = attributes(font: font(size: style.size * 0.64), color: set.palette.link, paragraph: paragraph)
            mark[.baselineOffset] = ((attributes(style, paragraph)[.baselineOffset] as? CGFloat) ?? 0) + style.size * 0.36
            append(String(match.output.1), mark)
            at = match.range.upperBound
        }
        let rest = String(string[at...])
        let offset = string.utf8.distance(from: string.startIndex, to: at)
        if !rest.isEmpty { append(rest, attributes(style, paragraph), bytes: bytes.map { $0.lowerBound + offset..<$0.upperBound }) }
    }

    // MARK: Text

    private mutating func append(_ string: String, _ attributes: [NSAttributedString.Key: Any], source: SourceRange? = nil) {
        let bytes = source.flatMap { source in byte(source.lowerBound).flatMap { lower in byte(source.upperBound).flatMap { $0 >= lower ? lower..<$0 : nil } } }
        append(string, attributes, bytes: bytes)
    }

    private mutating func append(_ string: String, _ attributes: [NSAttributedString.Key: Any], bytes: Range<Int>?) {
        let start = out.length
        out.append(NSAttributedString(string: string, attributes: attributes))
        guard let bytes else { return }
        runs.append(Rendered.Run(chapter: chapter, rendered: NSRange(location: start, length: out.length - start), source: bytes, text: string))
    }

    private func byte(_ location: SourceLocation) -> Int? {
        guard location.line >= 1, location.line <= lines.count else { return nil }
        return lines[location.line - 1] + location.column - 1
    }

    private mutating func attributes(_ style: Style, _ paragraph: NSParagraphStyle) -> [NSAttributedString.Key: Any] {
        let face = style.code ? PlatformFont.monospacedSystemFont(ofSize: style.size * 0.86, weight: .regular) : font(size: style.size, weight: style.weight, italic: style.italic)
        var attrs = attributes(font: face, color: style.color, paragraph: paragraph)
        if style.code { attrs[.backgroundColor] = set.palette.segment }
        if style.link { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue; attrs[.underlineColor] = set.palette.link.withAlphaComponent(0.45) }
        if style.strike { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        return attrs
    }

    private func attributes(font: PlatformFont, color: PlatformColor, paragraph: NSParagraphStyle) -> [NSAttributedString.Key: Any] {
        let line = paragraph.minimumLineHeight
        let offset = line > 0 ? (line - (font.ascender - font.descender)) / 4 : 0
        return [.font: font, .foregroundColor: color, .paragraphStyle: paragraph, .baselineOffset: offset]
    }

    private func paragraph(indent: CGFloat = 0, align: NSTextAlignment = .natural, before: CGFloat, after: CGFloat, lineHeight: CGFloat? = nil, hanging: CGFloat = 0, size: CGFloat? = nil) -> NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        let size = size ?? set.size
        let line = (lineHeight ?? set.lineHeight) * size
        style.minimumLineHeight = line
        style.maximumLineHeight = line
        style.paragraphSpacingBefore = before
        style.paragraphSpacing = after
        style.alignment = align
        style.firstLineHeadIndent = indent
        style.headIndent = indent + hanging
        if hanging > 0 { style.tabStops = [NSTextTab(textAlignment: .left, location: indent + hanging)] }
        style.hyphenationFactor = 0.2
        style.lineBreakStrategy = .pushOut
        return style
    }

    private mutating func font(size: CGFloat, weight: PlatformFont.Weight = .regular, italic: Bool = false) -> PlatformFont {
        let key = FontKey(size: size, bold: weight != .regular, italic: italic)
        if let cached = fontCache[key] { return cached }
        let made = Fonts.reading(set.face, size: size, bold: key.bold, italic: italic)
        fontCache[key] = made
        return made
    }

    private func interface(size: CGFloat, weight: PlatformFont.Weight = .regular) -> PlatformFont {
        PlatformFont(name: weight == .regular ? Fonts.inter : Fonts.interMedium, size: size) ?? .systemFont(ofSize: size, weight: weight)
    }
}

#if os(macOS)
typealias PlatformImage = NSImage
typealias PlatformView = NSView
typealias PlatformFontDescriptor = NSFontDescriptor
#else
typealias PlatformImage = UIImage
typealias PlatformView = UIView
typealias PlatformFontDescriptor = UIFontDescriptor
#endif

/// A picture no wider than its file and no wider than the line it lands on, so a column narrower
/// than the look's page width still holds it. Drawn by a real image view through TextKit 2's
/// `NSTextAttachmentViewProvider` so the GPU scales it during scroll instead of resampling it on
/// the CPU, and so AppKit draws it without an eager bitmap on `NSTextAttachment.image`.
nonisolated final class FittedAttachment: NSTextAttachment {
    var natural = CGSize.zero
    var picture: Picture?

    override var usesTextAttachmentView: Bool { picture != nil }

    override func viewProvider(for parentView: PlatformView?, location: any NSTextLocation, textContainer: NSTextContainer?) -> NSTextAttachmentViewProvider? {
        guard picture != nil else { return super.viewProvider(for: parentView, location: location, textContainer: textContainer) }
        return PictureViewProvider(textAttachment: self, parentView: parentView, textLayoutManager: textContainer?.textLayoutManager, location: location)
    }

    override func image(for bounds: CGRect, attributes: [NSAttributedString.Key: Any] = [:], location: any NSTextLocation, textContainer: NSTextContainer?) -> PlatformImage? {
        picture?.image ?? super.image(for: bounds, attributes: attributes, location: location, textContainer: textContainer)
    }

    override func attachmentBounds(for attributes: [NSAttributedString.Key: Any], location: any NSTextLocation, textContainer: NSTextContainer?, proposedLineFragment: CGRect, position: CGPoint) -> CGRect {
        let width = min(natural.width, proposedLineFragment.width)
        return CGRect(x: 0, y: 0, width: width, height: natural.height * width / max(natural.width, 1))
    }
}

nonisolated private final class PictureViewProvider: NSTextAttachmentViewProvider {
    override init(textAttachment: NSTextAttachment, parentView: PlatformView?, textLayoutManager: NSTextLayoutManager?, location: any NSTextLocation) {
        super.init(textAttachment: textAttachment, parentView: parentView, textLayoutManager: textLayoutManager, location: location)
        tracksTextAttachmentViewBounds = true
    }

    override func loadView() {
        let picture = (textAttachment as? FittedAttachment)?.picture
        view = MainActor.assumeIsolated {
            #if os(macOS)
            let imageView = NSImageView()
            imageView.image = picture?.image
            imageView.imageScaling = .scaleProportionallyUpOrDown
            return imageView
            #else
            let imageView = UIImageView(image: picture?.image)
            imageView.contentMode = .scaleAspectFit
            imageView.isUserInteractionEnabled = false
            return imageView
            #endif
        }
    }

    override func attachmentBounds(for attributes: [NSAttributedString.Key: Any], location: any NSTextLocation, textContainer: NSTextContainer?, proposedLineFragment: CGRect, position: CGPoint) -> CGRect {
        if let attachment = textAttachment as? FittedAttachment {
            return attachment.attachmentBounds(for: attributes, location: location, textContainer: textContainer, proposedLineFragment: proposedLineFragment, position: position)
        }
        return super.attachmentBounds(for: attributes, location: location, textContainer: textContainer, proposedLineFragment: proposedLineFragment, position: position)
    }
}

/// A picture in the text, decoded at the size it is drawn only as the reader nears it, off the main
/// thread, and kept in a cache the system may empty. A file left to decode as it is drawn stalls
/// the scroll the moment it comes in, and decoding every picture of an illustrated book up front
/// would hold hundreds of megabytes.
nonisolated final class Picture: @unchecked Sendable {
    /// Where it is in the text.
    let location: Int
    /// Its size in points: its own, fitted into the column and 90% of the column's height.
    let size: CGSize
    private let url: URL
    private let pixels: CGFloat
    private let key: NSString
    private let preparing = OSAllocatedUnfairLock(initialState: false)
    // NSCache is safe to use from any thread.
    nonisolated(unsafe) private static let cache: NSCache<NSString, PlatformImage> = {
        let cache = NSCache<NSString, PlatformImage>()
        cache.totalCostLimit = 64 << 20
        return cache
    }()
    private static let decoder = DispatchQueue(label: "inkling.pictures", qos: .userInitiated)

    init?(at url: URL, location: Int, column: CGFloat, scale: CGFloat) {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              var width = (properties[kCGImagePropertyPixelWidth] as? NSNumber).map({ CGFloat($0.doubleValue) }),
              var height = (properties[kCGImagePropertyPixelHeight] as? NSNumber).map({ CGFloat($0.doubleValue) }) else { return nil }
        if let orientation = properties[kCGImagePropertyOrientation] as? NSNumber, orientation.intValue >= 5 { swap(&width, &height) }
        let sourcePixels = max(width, height)
        #if os(macOS)
        // AppKit sizes a picture by its resolution, as NSImage(contentsOfFile:) does.
        if let dpi = (properties[kCGImagePropertyDPIWidth] as? NSNumber)?.doubleValue, dpi > 0 { width *= 72 / dpi }
        if let dpi = (properties[kCGImagePropertyDPIHeight] as? NSNumber)?.doubleValue, dpi > 0 { height *= 72 / dpi }
        #endif
        let fit = min(1, column / max(width, 1), column * 0.9 / max(height, 1))
        self.location = location
        self.url = url
        size = CGSize(width: width * fit, height: height * fit)
        // Never upscale a small source image beyond its native pixel dimensions.
        pixels = min(sourcePixels, (max(size.width, size.height) * max(scale, 1)).rounded(.up))
        key = "\(pixels)@\(url.path)" as NSString
    }

    /// The picture as drawn, decoded now if it wasn't ready.
    var image: PlatformImage? {
        Self.cache.object(forKey: key) ?? decode()
    }

    /// Decodes the picture in the background, unless it is ready or already queued.
    func prepare() {
        guard Self.cache.object(forKey: key) == nil else { return }
        let shouldQueue = preparing.withLock { active -> Bool in
            guard !active else { return false }
            active = true
            return true
        }
        guard shouldQueue else { return }
        Self.decoder.async { [self] in
            defer { preparing.withLock { $0 = false } }
            if Self.cache.object(forKey: key) == nil { _ = decode() }
        }
    }

    private func decode() -> PlatformImage? {
        let options = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: pixels,
        ] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let decoded = CGImageSourceCreateThumbnailAtIndex(source, 0, options) else { return nil }
        #if os(macOS)
        let image = NSImage(cgImage: decoded, size: size)
        #else
        let image = UIImage(cgImage: decoded, scale: CGFloat(decoded.width) / max(size.width, 1), orientation: .up)
        #endif
        Self.cache.setObject(image, forKey: key, cost: decoded.bytesPerRow * decoded.height)
        return image
    }
}

extension Rendered {
    /// Readies the pictures a few screens either side of `character`, so each is decoded before it
    /// scrolls in.
    func preparePictures(around character: Int) {
        guard !pictures.isEmpty else { return }
        let near = (character - 4_000)...(character + 12_000)
        var low = 0, high = pictures.count
        while low < high {
            let middle = (low + high) / 2
            if pictures[middle].location < near.lowerBound { low = middle + 1 } else { high = middle }
        }
        for picture in pictures[low...].prefix(while: { near.contains($0.location) }) { picture.prepare() }
    }
}

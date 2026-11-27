import Foundation
import InklingCore
import Markdown

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
    /// The runs of text that come straight from the Markdown, in order.
    let runs: [Run]

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
        chapters.lastIndex { $0.location <= character } ?? 0
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

/// How the text is set: the look's face, size and spacing, the palette, and the column's width.
nonisolated struct Typesetting: @unchecked Sendable {
    var face: String
    var size: CGFloat
    var lineHeight: CGFloat
    var titleSize: CGFloat
    var palette: Palette
    var column: CGFloat
}

/// Draws Markdown chapters (ADR 0006) for the reader. Runs off the main thread.
nonisolated struct Renderer {
    let set: Typesetting
    let folder: URL
    /// The line under an article's title: its author, its site and how long it takes to read.
    let byline: String?

    private var out = NSMutableAttributedString()
    private var runs: [Rendered.Run] = []
    private var chapter = 0
    private var lines: [Int] = [0]
    private var titled = false

    init(set: Typesetting, folder: URL, byline: String?) {
        self.set = set
        self.folder = folder
        self.byline = byline
    }

    static func render(_ book: Book, set: Typesetting, folder: URL, byline: String?) -> Rendered {
        var renderer = Renderer(set: set, folder: folder, byline: byline)
        var spans: [NSRange] = []
        for (index, chapter) in book.chapters.enumerated() {
            let start = renderer.out.length
            renderer.chapter(index, chapter, in: book)
            spans.append(NSRange(location: start, length: renderer.out.length - start))
        }
        return Rendered(text: renderer.out, chapters: spans, runs: renderer.runs)
    }

    private mutating func chapter(_ index: Int, _ chapter: Chapter, in book: Book) {
        self.chapter = index
        lines = [0]
        for (offset, byte) in chapter.text.utf8.enumerated() where byte == 0x0A { lines.append(offset + 1) }
        let document = Document(parsing: chapter.text)
        let opensWithHeading = document.child(at: 0) is Heading
        if index > 0 {
            append("\u{2042}\n", attributes(font: font(size: set.size * 0.9), color: set.palette.muted, paragraph: paragraph(align: .center, before: set.size * 2.4, after: set.size * 2.4)))
        }
        if !opensWithHeading, let title = chapter.title, book.chapters.count > 1 {
            append(title + "\n", attributes(font: font(size: set.size * 1.35, weight: .semibold), color: set.palette.ink, paragraph: paragraph(before: 0, after: set.size * 0.9, lineHeight: 1.25)))
        }
        for block in document.children { self.block(block, indent: 0) }
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
        if let source = image.source, let picture = PlatformImage(contentsOfFile: folder.appending(path: source).path) {
            let attachment = FittedAttachment()
            attachment.image = picture
            let size = picture.size
            let scale = min(1, set.column / max(size.width, 1), set.column * 0.9 / max(size.height, 1))
            attachment.natural = CGSize(width: size.width * scale, height: size.height * scale)
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
            append(text.string, attributes(style, paragraph), source: text.range)
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

    // MARK: Text

    private mutating func append(_ string: String, _ attributes: [NSAttributedString.Key: Any], source: SourceRange? = nil) {
        let start = out.length
        out.append(NSAttributedString(string: string, attributes: attributes))
        guard let source, let lower = byte(source.lowerBound), let upper = byte(source.upperBound), upper >= lower else { return }
        runs.append(Rendered.Run(chapter: chapter, rendered: NSRange(location: start, length: out.length - start), source: lower..<upper, text: string))
    }

    private func byte(_ location: SourceLocation) -> Int? {
        guard location.line >= 1, location.line <= lines.count else { return nil }
        return lines[location.line - 1] + location.column - 1
    }

    private func attributes(_ style: Style, _ paragraph: NSParagraphStyle) -> [NSAttributedString.Key: Any] {
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

    private func paragraph(indent: CGFloat = 0, align: NSTextAlignment = .natural, before: CGFloat, after: CGFloat, lineHeight: CGFloat? = nil, hanging: CGFloat = 0) -> NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        let size = set.size
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

    private func font(size: CGFloat, weight: PlatformFont.Weight = .regular, italic: Bool = false) -> PlatformFont {
        Fonts.reading(set.face, size: size, bold: weight != .regular, italic: italic)
    }

    private func interface(size: CGFloat) -> PlatformFont {
        PlatformFont(name: Fonts.inter, size: size) ?? .systemFont(ofSize: size)
    }
}

#if os(macOS)
typealias PlatformImage = NSImage
typealias PlatformFontDescriptor = NSFontDescriptor
#else
typealias PlatformImage = UIImage
typealias PlatformFontDescriptor = UIFontDescriptor
#endif

/// A picture no wider than its file and no wider than the line it lands on, so a column narrower
/// than the look's page width still holds it.
nonisolated final class FittedAttachment: NSTextAttachment {
    var natural = CGSize.zero

    override func attachmentBounds(for attributes: [NSAttributedString.Key: Any], location: any NSTextLocation, textContainer: NSTextContainer?, proposedLineFragment: CGRect, position: CGPoint) -> CGRect {
        let width = min(natural.width, proposedLineFragment.width)
        return CGRect(x: 0, y: 0, width: width, height: natural.height * width / max(natural.width, 1))
    }
}

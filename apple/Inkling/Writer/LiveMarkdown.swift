import Foundation
import InklingCore
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Custom attributed-string key for TextKit 2 block decorations drawn by `InklingLayoutFragment`.
extension NSAttributedString.Key {
    nonisolated static let inklingBlock = NSAttributedString.Key("com.tobygodat.inkling.block")
}

/// Visual chrome drawn behind or beside a paragraph by `InklingLayoutFragment` (TextKit 2).
nonisolated enum BlockDecoration: Equatable {
    case blockquote
    case codeBlock(top: Bool, bottom: Bool)
    case thematicBreak
}

/// The typographic configuration for the live Markdown editor.
nonisolated struct WritingStyle: Equatable {
    let face: String
    let size: CGFloat
    let lineHeight: CGFloat
    let titleSize: CGFloat
    let palette: Palette
    let column: CGFloat

    static func make(look: Look, palette: Palette, column: CGFloat, isPhone: Bool) -> WritingStyle {
        let scale: CGFloat = isPhone ? 19.0 / 21.0 : 1.0
        let lineScale: CGFloat = isPhone ? 1.65 / 1.75 : 1.0
        let baseTitle: CGFloat = isPhone ? 28.0 : 36.0
        let size = CGFloat(look.textSize) * scale
        return WritingStyle(
            face: look.face,
            size: size,
            lineHeight: CGFloat(look.lineSpacing) / 100.0 * lineScale,
            titleSize: baseTitle * CGFloat(look.textSize) / 21.0,
            palette: palette,
            column: column
        )
    }
}

/// Incremental, paragraph-scoped live Markdown styler for TextKit 2.
///
/// The text storage always holds the exact plain Markdown source. Every construct (headings,
/// emphasis, strikethrough, inline code, links, blockquotes, lists, fenced code blocks and
/// thematic breaks) is styled in-place with Inkling's typography and palette while keeping syntax
/// marks quieter than the prose.
///
/// On ordinary keystrokes only the edited line(s) are re-styled, so typing in a 5,000-word draft
/// touches one paragraph instead of the whole document.
final class LiveMarkdownStyler {
    private(set) var style: WritingStyle

    // Precomputed fonts and colours so styling a line allocates almost nothing.
    private var bodyFont: PlatformFont
    private var boldFont: PlatformFont
    private var italicFont: PlatformFont
    private var boldItalicFont: PlatformFont
    private var monoFont: PlatformFont
    private var monoBlockFont: PlatformFont
    private var headingFonts: [PlatformFont] = []
    private var headingMarkFonts: [PlatformFont] = []
    private var urlFont: PlatformFont
    private var markColor: PlatformColor
    private var quoteColor: PlatformColor
    private var linkUnderlineColor: PlatformColor

    // Precomputed paragraph styles.
    private var bodyParagraph: NSParagraphStyle
    private var blankParagraph: NSParagraphStyle
    private var quoteParagraph: NSParagraphStyle
    private var codeParagraph: NSParagraphStyle
    private var breakParagraph: NSParagraphStyle
    private var headingParagraphs: [NSParagraphStyle] = []
    private var firstHeadingParagraphs: [NSParagraphStyle] = []

    /// Whether a fenced code block is open *after* line `i`. Keeps incremental edits O(1) unless
    /// a fence opens or closes.
    private var fenceOpenAfterLine: [Bool] = []
    private var cachedLineCount = 0

    init(style: WritingStyle) {
        self.style = style
        self.bodyFont = Fonts.reading(style.face, size: style.size)
        self.boldFont = Fonts.reading(style.face, size: style.size, bold: true)
        self.italicFont = Fonts.reading(style.face, size: style.size, italic: true)
        self.boldItalicFont = Fonts.reading(style.face, size: style.size, bold: true, italic: true)
        self.monoFont = PlatformFont.monospacedSystemFont(ofSize: style.size * 0.84, weight: .regular)
        self.monoBlockFont = PlatformFont.monospacedSystemFont(ofSize: style.size * 0.78, weight: .regular)
        self.urlFont = PlatformFont(name: Fonts.inter, size: max(11, style.size * 0.68)) ?? .systemFont(ofSize: max(11, style.size * 0.68))
        self.markColor = style.palette.muted.withAlphaComponent(style.palette.isDark ? 0.56 : 0.48)
        self.quoteColor = Self.blend(style.palette.ink, toward: style.palette.muted, amount: 0.24)
        self.linkUnderlineColor = style.palette.link.withAlphaComponent(0.45)
        self.bodyParagraph = NSParagraphStyle()
        self.blankParagraph = NSParagraphStyle()
        self.quoteParagraph = NSParagraphStyle()
        self.codeParagraph = NSParagraphStyle()
        self.breakParagraph = NSParagraphStyle()
        rebuildCache()
    }

    func update(style newStyle: WritingStyle, storage: NSTextStorage) {
        guard newStyle != style else { return }
        style = newStyle
        rebuildCache()
        restyleAll(storage)
    }

    /// Default typing attributes for plain body text.
    var defaultTypingAttributes: [NSAttributedString.Key: Any] {
        baseAttributes(font: bodyFont, color: style.palette.ink, paragraph: bodyParagraph)
    }

    /// Re-styles the whole storage in one editing batch.
    func restyleAll(_ storage: NSTextStorage) {
        let string = storage.string as NSString
        let length = string.length
        fenceOpenAfterLine.removeAll(keepingCapacity: true)
        guard length > 0 else {
            cachedLineCount = 0
            return
        }

        storage.beginEditing()
        var pos = 0
        var lineIndex = 0
        var inFence = false
        var fenceChar: unichar = 0
        var fenceLen = 0
        var codeRuns: [(range: NSRange, isFence: Bool)] = []

        while pos < length {
            let lineRange = string.lineRange(for: NSRange(location: pos, length: 0))
            let contentRange = trimmedNewline(lineRange, in: string)
            let isFirst = (lineIndex == 0)

            if let fence = parseFence(in: string, range: contentRange) {
                if !inFence {
                    inFence = true
                    fenceChar = fence.char
                    fenceLen = fence.count
                    codeRuns.append((lineRange, true))
                } else if fence.char == fenceChar && fence.count >= fenceLen && fence.restIsBlank {
                    codeRuns.append((lineRange, true))
                    applyCodeBlock(codeRuns, to: storage, in: string)
                    codeRuns.removeAll(keepingCapacity: true)
                    inFence = false
                    fenceChar = 0
                    fenceLen = 0
                } else {
                    codeRuns.append((lineRange, false))
                }
            } else if inFence {
                codeRuns.append((lineRange, false))
            } else {
                styleNonCodeLine(lineRange: lineRange, contentRange: contentRange, isFirst: isFirst, storage: storage, string: string)
            }

            fenceOpenAfterLine.append(inFence)
            pos = NSMaxRange(lineRange)
            lineIndex += 1
        }
        if !codeRuns.isEmpty {
            applyCodeBlock(codeRuns, to: storage, in: string)
        }
        cachedLineCount = lineIndex
        storage.endEditing()
    }

    /// Re-styles only the affected line(s) after an edit, falling back to a full pass only when
    /// fenced code blocks change state or the line structure shifts across fences.
    func restyleEdited(_ storage: NSTextStorage, editedRange: NSRange) {
        let string = storage.string as NSString
        let length = string.length
        guard length > 0 else {
            fenceOpenAfterLine.removeAll(keepingCapacity: true)
            cachedLineCount = 0
            return
        }

        let safeLoc = min(editedRange.location, max(0, length - 1))
        let safeEnd = min(NSMaxRange(editedRange), length)
        let safeRange = NSRange(location: safeLoc, length: max(0, safeEnd - safeLoc))
        let blockRange = string.lineRange(for: safeRange)

        // If any fenced code block exists in the document OR the edited text introduces a fence
        // delimiter (``` or ~~~), re-sync fence state cleanly. Otherwise, style ONLY the edited
        // line(s) in O(edited length).
        let hasExistingFences = fenceOpenAfterLine.contains(true)
        let editedText = string.substring(with: blockRange)
        let touchesFence = editedText.contains("```") || editedText.contains("~~~")

        if hasExistingFences || touchesFence {
            restyleAll(storage)
            return
        }

        storage.beginEditing()
        var pos = blockRange.location
        let end = NSMaxRange(blockRange)
        while pos < end {
            let lineRange = string.lineRange(for: NSRange(location: pos, length: 0))
            let contentRange = trimmedNewline(lineRange, in: string)
            styleNonCodeLine(
                lineRange: lineRange,
                contentRange: contentRange,
                isFirst: lineRange.location == 0,
                storage: storage,
                string: string
            )
            pos = NSMaxRange(lineRange)
        }
        storage.endEditing()
    }

    /// Returns appropriate typing attributes for the caret position so inserted text inherits
    /// the current heading/body/code font immediately without visual flicker.
    func typingAttributes(in storage: NSTextStorage, at location: Int) -> [NSAttributedString.Key: Any] {
        let length = storage.length
        guard length > 0 else { return defaultTypingAttributes }
        let string = storage.string as NSString
        let probe = min(max(location, 0), length)
        // A caret after the draft's last newline is on a new, empty line of body text (or code
        // inside an open fence), not on the line before it, whose heading or quote it would
        // otherwise take until the first character is typed.
        if probe == length, string.character(at: length - 1) == 0x0A, fenceOpenAfterLine.last != true {
            return defaultTypingAttributes
        }
        let lineRange = string.lineRange(for: NSRange(location: min(probe, length - 1), length: 0))
        let contentRange = trimmedNewline(lineRange, in: string)
        if let heading = parseHeading(in: string, range: contentRange), probe > contentRange.location + heading.prefixLen {
            let level = heading.level
            let para = lineRange.location == 0 ? firstHeadingParagraphs[level - 1] : headingParagraphs[level - 1]
            var attrs = baseAttributes(font: headingFonts[level - 1], color: style.palette.ink, paragraph: para)
            if level == 1 { attrs[.kern] = -0.018 * style.titleSize }
            else if level == 2 { attrs[.kern] = -0.012 * style.size * 1.40 }
            return attrs
        }
        if probe > 0 {
            let prev = storage.attributes(at: probe - 1, effectiveRange: nil)
            if prev[.inklingBlock] as? BlockDecoration != nil, let font = prev[.font], let para = prev[.paragraphStyle] {
                var attrs = prev
                attrs[.foregroundColor] = style.palette.ink
                attrs[.font] = font
                attrs[.paragraphStyle] = para
                attrs.removeValue(forKey: .strikethroughStyle)
                attrs.removeValue(forKey: .underlineStyle)
                attrs.removeValue(forKey: .backgroundColor)
                return attrs
            }
        }
        return defaultTypingAttributes
    }

    // MARK: - Block Styling

    private func styleNonCodeLine(lineRange: NSRange, contentRange: NSRange, isFirst: Bool, storage: NSTextStorage, string: NSString) {
        if contentRange.length == 0 {
            storage.setAttributes(baseAttributes(font: bodyFont, color: style.palette.ink, paragraph: blankParagraph), range: lineRange)
            return
        }

        // 1. ATX Heading (# .. ######)
        if let heading = parseHeading(in: string, range: contentRange) {
            let level = heading.level
            let hFont = headingFonts[level - 1]
            let mFont = headingMarkFonts[level - 1]
            let para = isFirst ? firstHeadingParagraphs[level - 1] : headingParagraphs[level - 1]
            var base = baseAttributes(font: hFont, color: style.palette.ink, paragraph: para)
            if level == 1 { base[.kern] = -0.018 * style.titleSize }
            else if level == 2 { base[.kern] = -0.012 * style.size * 1.40 }
            storage.setAttributes(base, range: lineRange)

            let markRange = NSRange(location: contentRange.location, length: heading.prefixLen)
            storage.addAttributes([.font: mFont, .foregroundColor: markColor], range: markRange)

            let textStart = contentRange.location + heading.prefixLen
            let textLen = NSMaxRange(contentRange) - textStart
            if textLen > 0 {
                let headingStyle = InlineContext(
                    baseFont: hFont,
                    boldFont: hFont,
                    italicFont: Fonts.reading(style.face, size: hFont.pointSize, bold: true, italic: true),
                    boldItalicFont: Fonts.reading(style.face, size: hFont.pointSize, bold: true, italic: true),
                    baseColor: style.palette.ink
                )
                styleInlines(in: string, range: NSRange(location: textStart, length: textLen), context: headingStyle, storage: storage)
            }
            return
        }

        // 2. Thematic break (---, ***, ___)
        if isThematicBreak(in: string, range: contentRange) {
            var attrs = baseAttributes(font: bodyFont, color: markColor.withAlphaComponent(0.35), paragraph: breakParagraph)
            attrs[.inklingBlock] = BlockDecoration.thematicBreak
            storage.setAttributes(attrs, range: lineRange)
            return
        }

        // 3. Blockquote (> ...)
        if let quotePrefix = parseBlockquote(in: string, range: contentRange) {
            var attrs = baseAttributes(font: italicFont, color: quoteColor, paragraph: quoteParagraph)
            attrs[.inklingBlock] = BlockDecoration.blockquote
            storage.setAttributes(attrs, range: lineRange)
            let markRange = NSRange(location: contentRange.location, length: quotePrefix)
            storage.addAttributes([.font: bodyFont, .foregroundColor: markColor], range: markRange)
            let textStart = contentRange.location + quotePrefix
            let textLen = NSMaxRange(contentRange) - textStart
            if textLen > 0 {
                let ctx = InlineContext(
                    baseFont: italicFont,
                    boldFont: boldItalicFont,
                    italicFont: bodyFont,
                    boldItalicFont: boldFont,
                    baseColor: quoteColor
                )
                styleInlines(in: string, range: NSRange(location: textStart, length: textLen), context: ctx, storage: storage)
            }
            return
        }

        // 4. Unordered or ordered list (- , * , + , 1. )
        if let list = parseList(in: string, range: contentRange) {
            let indentLevel = min(list.leadingSpaces / 2, 4)
            let baseIndent = CGFloat(indentLevel) * style.size * 1.15
            let hanging = style.size * (list.isOrdered ? 1.55 : 1.05)
            let para = makeParagraph(
                lineHeight: style.lineHeight,
                before: 0,
                after: style.size * 0.22,
                firstLineIndent: baseIndent,
                headIndent: baseIndent + hanging
            )
            storage.setAttributes(baseAttributes(font: bodyFont, color: style.palette.ink, paragraph: para), range: lineRange)
            let markerRange = NSRange(location: contentRange.location + list.leadingSpaces, length: list.markerLen)
            storage.addAttribute(.foregroundColor, value: style.palette.muted, range: markerRange)
            let textStart = contentRange.location + list.leadingSpaces + list.markerLen
            let textLen = NSMaxRange(contentRange) - textStart
            if textLen > 0 {
                styleInlines(in: string, range: NSRange(location: textStart, length: textLen), context: bodyInlineContext, storage: storage)
            }
            return
        }

        // 5. Ordinary body paragraph
        storage.setAttributes(baseAttributes(font: bodyFont, color: style.palette.ink, paragraph: bodyParagraph), range: lineRange)
        styleInlines(in: string, range: contentRange, context: bodyInlineContext, storage: storage)
    }

    private func applyCodeBlock(_ lines: [(range: NSRange, isFence: Bool)], to storage: NSTextStorage, in string: NSString) {
        let count = lines.count
        for (idx, item) in lines.enumerated() {
            let isTop = idx == 0
            let isBottom = idx == count - 1
            let para = makeParagraph(
                lineHeight: 1.48,
                before: isTop ? style.size * 0.45 : 0,
                after: isBottom ? style.size * 0.45 : 0,
                firstLineIndent: 16,
                headIndent: 16,
                tailIndent: -16,
                size: monoBlockFont.pointSize
            )
            var attrs = baseAttributes(
                font: monoBlockFont,
                color: item.isFence ? markColor : style.palette.ink,
                paragraph: para
            )
            attrs[.inklingBlock] = BlockDecoration.codeBlock(top: isTop, bottom: isBottom)
            storage.setAttributes(attrs, range: item.range)
        }
    }

    // MARK: - Inline Markdown Scanner (Linear-Time UTF-16)

    private struct InlineContext {
        let baseFont: PlatformFont
        let boldFont: PlatformFont
        let italicFont: PlatformFont
        let boldItalicFont: PlatformFont
        let baseColor: PlatformColor
    }

    private var bodyInlineContext: InlineContext {
        InlineContext(
            baseFont: bodyFont,
            boldFont: boldFont,
            italicFont: italicFont,
            boldItalicFont: boldItalicFont,
            baseColor: style.palette.ink
        )
    }

    private func styleInlines(in string: NSString, range: NSRange, context: InlineContext, storage: NSTextStorage) {
        let end = NSMaxRange(range)
        var i = range.location

        while i < end {
            let ch = string.character(at: i)

            // Backslash escape: mute the backslash, skip the escaped character.
            if ch == 0x5C /* \ */ && i + 1 < end {
                let next = string.character(at: i + 1)
                if isAsciiPunctuation(next) {
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 1))
                    i += 2
                    continue
                }
            }

            // Inline code: `...`
            if ch == 0x60 /* ` */ {
                var ticks = 1
                while i + ticks < end && string.character(at: i + ticks) == 0x60 { ticks += 1 }
                if let close = findMatchingTicks(in: string, from: i + ticks, end: end, count: ticks) {
                    let full = NSRange(location: i, length: (close + ticks) - i)
                    storage.addAttributes([
                        .font: monoFont,
                        .backgroundColor: style.palette.segment,
                        .foregroundColor: context.baseColor,
                    ], range: full)
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: ticks))
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: close, length: ticks))
                    i = close + ticks
                    continue
                }
                i += ticks
                continue
            }

            // Link: [label](destination)
            if ch == 0x5B /* [ */ {
                if let link = parseLink(in: string, from: i, end: end) {
                    // Opening `[`
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 1))
                    // Link label in Link colour, subtly underlined
                    if link.labelRange.length > 0 {
                        storage.addAttributes([
                            .foregroundColor: style.palette.link,
                            .underlineStyle: NSUnderlineStyle.single.rawValue,
                            .underlineColor: linkUnderlineColor,
                        ], range: link.labelRange)
                    }
                    // Closing `]` and `(url)` in quiet muted style
                    storage.addAttributes([
                        .foregroundColor: markColor,
                        .font: urlFont,
                    ], range: link.tailRange)
                    i = NSMaxRange(link.tailRange)
                    continue
                }
            }

            // Strikethrough: ~~text~~
            if ch == 0x7E /* ~ */ && i + 1 < end && string.character(at: i + 1) == 0x7E {
                if let close = findPair(in: string, char: 0x7E, from: i + 2, end: end) {
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 2))
                    let inner = NSRange(location: i + 2, length: close - (i + 2))
                    if inner.length > 0 {
                        storage.addAttributes([
                            .strikethroughStyle: NSUnderlineStyle.single.rawValue,
                            .foregroundColor: style.palette.muted,
                        ], range: inner)
                    }
                    storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: close, length: 2))
                    i = close + 2
                    continue
                }
            }

            // Bold / Italic / Bold-Italic: *** / ** / * / ___ / __ / _
            if ch == 0x2A /* * */ || ch == 0x5F /* _ */ {
                // Respect word boundaries for `_` so identifiers like foo_bar_baz stay plain.
                if ch == 0x5F && i > range.location && isWordChar(string.character(at: i - 1)) {
                    i += 1
                    continue
                }
                var run = 1
                while run < 3 && i + run < end && string.character(at: i + run) == ch { run += 1 }
                if run == 3, let close = findDelimiterRun(in: string, char: ch, count: 3, from: i + 3, end: end) {
                    let inner = NSRange(location: i + 3, length: close - (i + 3))
                    if inner.length > 0 {
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 3))
                        storage.addAttribute(.font, value: context.boldItalicFont, range: inner)
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: close, length: 3))
                        i = close + 3
                        continue
                    }
                }
                if run >= 2, let close = findDelimiterRun(in: string, char: ch, count: 2, from: i + 2, end: end) {
                    let inner = NSRange(location: i + 2, length: close - (i + 2))
                    if inner.length > 0 {
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 2))
                        storage.addAttribute(.font, value: context.boldFont, range: inner)
                        let nested = InlineContext(
                            baseFont: context.boldFont,
                            boldFont: context.boldFont,
                            italicFont: context.boldItalicFont,
                            boldItalicFont: context.boldItalicFont,
                            baseColor: context.baseColor
                        )
                        styleInlines(in: string, range: inner, context: nested, storage: storage)
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: close, length: 2))
                        i = close + 2
                        continue
                    }
                }
                if run == 1, let close = findSingleDelimiter(in: string, char: ch, from: i + 1, end: end) {
                    let inner = NSRange(location: i + 1, length: close - (i + 1))
                    if inner.length > 0 {
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: i, length: 1))
                        storage.addAttribute(.font, value: context.italicFont, range: inner)
                        storage.addAttribute(.foregroundColor, value: markColor, range: NSRange(location: close, length: 1))
                        i = close + 1
                        continue
                    }
                }
            }

            i += 1
        }
    }

    // MARK: - Lightweight UTF-16 Parsers

    private func trimmedNewline(_ lineRange: NSRange, in string: NSString) -> NSRange {
        var len = lineRange.length
        while len > 0 {
            let c = string.character(at: lineRange.location + len - 1)
            if c == 0x0A || c == 0x0D { len -= 1 } else { break }
        }
        return NSRange(location: lineRange.location, length: len)
    }

    private func parseFence(in string: NSString, range: NSRange) -> (char: unichar, count: Int, restIsBlank: Bool)? {
        guard range.length >= 3 else { return nil }
        var p = range.location
        let end = NSMaxRange(range)
        var spaces = 0
        while p < end && spaces < 3 && string.character(at: p) == 0x20 { p += 1; spaces += 1 }
        guard p < end else { return nil }
        let ch = string.character(at: p)
        guard ch == 0x60 || ch == 0x7E else { return nil }
        var count = 0
        while p + count < end && string.character(at: p + count) == ch { count += 1 }
        guard count >= 3 else { return nil }
        var q = p + count
        var blank = true
        while q < end {
            let c = string.character(at: q)
            if c != 0x20 && c != 0x09 { blank = false; break }
            q += 1
        }
        return (ch, count, blank)
    }

    private func parseHeading(in string: NSString, range: NSRange) -> (level: Int, prefixLen: Int)? {
        let end = NSMaxRange(range)
        var p = range.location
        var level = 0
        while p < end && level < 6 && string.character(at: p) == 0x23 /* # */ {
            level += 1
            p += 1
        }
        guard level > 0 else { return nil }
        if p < end {
            guard string.character(at: p) == 0x20 || string.character(at: p) == 0x09 else { return nil }
            p += 1
        }
        return (level, p - range.location)
    }

    private func parseBlockquote(in string: NSString, range: NSRange) -> Int? {
        let end = NSMaxRange(range)
        var p = range.location
        var spaces = 0
        while p < end && spaces < 3 && string.character(at: p) == 0x20 { p += 1; spaces += 1 }
        guard p < end, string.character(at: p) == 0x3E /* > */ else { return nil }
        p += 1
        if p < end && string.character(at: p) == 0x20 { p += 1 }
        return p - range.location
    }

    private func parseList(in string: NSString, range: NSRange) -> (leadingSpaces: Int, markerLen: Int, isOrdered: Bool)? {
        let end = NSMaxRange(range)
        var p = range.location
        while p < end && string.character(at: p) == 0x20 { p += 1 }
        let leading = p - range.location
        guard p + 1 < end else { return nil }
        let c = string.character(at: p)
        if (c == 0x2D /* - */ || c == 0x2A /* * */ || c == 0x2B /* + */),
           string.character(at: p + 1) == 0x20 || string.character(at: p + 1) == 0x09 {
            return (leading, 2, false)
        }
        if c >= 0x30 && c <= 0x39 {
            var q = p
            while q < end && q - p < 9 {
                let d = string.character(at: q)
                if d >= 0x30 && d <= 0x39 { q += 1 } else { break }
            }
            if q + 1 < end && (string.character(at: q) == 0x2E /* . */ || string.character(at: q) == 0x29 /* ) */) &&
                (string.character(at: q + 1) == 0x20 || string.character(at: q + 1) == 0x09) {
                return (leading, (q + 2) - p, true)
            }
        }
        return nil
    }

    private func isThematicBreak(in string: NSString, range: NSRange) -> Bool {
        guard range.length >= 3 else { return false }
        var marker: unichar = 0
        var count = 0
        for idx in range.location..<NSMaxRange(range) {
            let c = string.character(at: idx)
            if c == 0x20 || c == 0x09 { continue }
            if c == 0x2D || c == 0x2A || c == 0x5F {
                if marker == 0 { marker = c }
                else if marker != c { return false }
                count += 1
            } else {
                return false
            }
        }
        return count >= 3
    }

    private func findMatchingTicks(in string: NSString, from start: Int, end: Int, count: Int) -> Int? {
        var i = start
        while i < end {
            if string.character(at: i) == 0x60 {
                var n = 1
                while i + n < end && string.character(at: i + n) == 0x60 { n += 1 }
                if n == count { return i }
                i += n
            } else {
                i += 1
            }
        }
        return nil
    }

    private func parseLink(in string: NSString, from start: Int, end: Int) -> (labelRange: NSRange, tailRange: NSRange)? {
        var i = start + 1
        var depth = 0
        while i < end {
            let c = string.character(at: i)
            if c == 0x5C && i + 1 < end { i += 2; continue }
            if c == 0x5B { depth += 1 }
            else if c == 0x5D {
                if depth == 0 { break }
                depth -= 1
            }
            i += 1
        }
        guard i < end, string.character(at: i) == 0x5D else { return nil }
        let closeBracket = i
        guard closeBracket + 1 < end, string.character(at: closeBracket + 1) == 0x28 /* ( */ else { return nil }
        var j = closeBracket + 2
        var parenDepth = 0
        while j < end {
            let c = string.character(at: j)
            if c == 0x5C && j + 1 < end { j += 2; continue }
            if c == 0x28 { parenDepth += 1 }
            else if c == 0x29 {
                if parenDepth == 0 { break }
                parenDepth -= 1
            }
            j += 1
        }
        guard j < end, string.character(at: j) == 0x29 else { return nil }
        let label = NSRange(location: start + 1, length: closeBracket - (start + 1))
        let tail = NSRange(location: closeBracket, length: (j + 1) - closeBracket)
        return (label, tail)
    }

    private func findPair(in string: NSString, char: unichar, from start: Int, end: Int) -> Int? {
        guard start < end, string.character(at: start) != 0x20 else { return nil }
        var i = start
        while i + 1 < end {
            let c = string.character(at: i)
            if c == 0x5C && i + 1 < end { i += 2; continue }
            if c == char && string.character(at: i + 1) == char && string.character(at: i - 1) != 0x20 {
                return i
            }
            i += 1
        }
        return nil
    }

    private func findDelimiterRun(in string: NSString, char: unichar, count: Int, from start: Int, end: Int) -> Int? {
        guard start < end, string.character(at: start) != 0x20 else { return nil }
        var i = start
        while i + count <= end {
            let c = string.character(at: i)
            if c == 0x5C && i + 1 < end { i += 2; continue }
            if c == 0x60 { // skip inline code inside bold
                i += 1
                while i < end && string.character(at: i) != 0x60 { i += 1 }
                i += 1
                continue
            }
            if c == char {
                var matched = true
                for k in 1..<count where string.character(at: i + k) != char { matched = false; break }
                if matched && i > start && string.character(at: i - 1) != 0x20 {
                    if char == 0x5F && i + count < end && isWordChar(string.character(at: i + count)) {
                        i += count
                        continue
                    }
                    return i
                }
            }
            i += 1
        }
        return nil
    }

    private func findSingleDelimiter(in string: NSString, char: unichar, from start: Int, end: Int) -> Int? {
        guard start < end, string.character(at: start) != 0x20 else { return nil }
        var i = start
        while i < end {
            let c = string.character(at: i)
            if c == 0x5C && i + 1 < end { i += 2; continue }
            if c == 0x60 {
                i += 1
                while i < end && string.character(at: i) != 0x60 { i += 1 }
                i += 1
                continue
            }
            if c == char {
                // Skip double delimiters (`**`) while hunting for a single `*`.
                if i + 1 < end && string.character(at: i + 1) == char {
                    i += 2
                    continue
                }
                if i > start && string.character(at: i - 1) != 0x20 {
                    if char == 0x5F && i + 1 < end && isWordChar(string.character(at: i + 1)) {
                        i += 1
                        continue
                    }
                    return i
                }
            }
            i += 1
        }
        return nil
    }

    private func isWordChar(_ c: unichar) -> Bool {
        (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A)
    }

    private func isAsciiPunctuation(_ c: unichar) -> Bool {
        (c >= 0x21 && c <= 0x2F) || (c >= 0x3A && c <= 0x40) || (c >= 0x5B && c <= 0x60) || (c >= 0x7B && c <= 0x7E)
    }

    // MARK: - Cache Construction

    private func rebuildCache() {
        bodyFont = Fonts.reading(style.face, size: style.size)
        boldFont = Fonts.reading(style.face, size: style.size, bold: true)
        italicFont = Fonts.reading(style.face, size: style.size, italic: true)
        boldItalicFont = Fonts.reading(style.face, size: style.size, bold: true, italic: true)
        monoFont = PlatformFont.monospacedSystemFont(ofSize: style.size * 0.84, weight: .regular)
        monoBlockFont = PlatformFont.monospacedSystemFont(ofSize: style.size * 0.78, weight: .regular)
        urlFont = PlatformFont(name: Fonts.inter, size: max(11, style.size * 0.68)) ?? .systemFont(ofSize: max(11, style.size * 0.68))
        markColor = style.palette.muted.withAlphaComponent(style.palette.isDark ? 0.56 : 0.48)
        quoteColor = Self.blend(style.palette.ink, toward: style.palette.muted, amount: 0.24)
        linkUnderlineColor = style.palette.link.withAlphaComponent(0.45)

        bodyParagraph = makeParagraph(lineHeight: style.lineHeight, before: 0, after: 0)
        // Compact blank line so Markdown's `\n\n` produces a book-grade paragraph gap.
        blankParagraph = makeParagraph(lineHeight: 0.68, before: 0, after: 0)
        quoteParagraph = makeParagraph(lineHeight: style.lineHeight, before: 2, after: 2, firstLineIndent: 20, headIndent: 20)
        codeParagraph = makeParagraph(lineHeight: 1.48, before: 0, after: 0, firstLineIndent: 16, headIndent: 16, tailIndent: -16, size: monoBlockFont.pointSize)
        breakParagraph = makeParagraph(lineHeight: 1.4, before: style.size * 0.4, after: style.size * 0.4, align: .center)

        let scales: [CGFloat] = [style.titleSize / style.size, 1.40, 1.18, 1.06, 1.02, 1.0]
        let lineHeights: [CGFloat] = [1.20, 1.28, 1.34, 1.40, 1.42, 1.44]
        let befores: [CGFloat] = [style.size * 1.1, style.size * 0.95, style.size * 0.75, style.size * 0.55, style.size * 0.45, style.size * 0.4]
        let afters: [CGFloat] = [style.size * 0.25, style.size * 0.18, style.size * 0.12, style.size * 0.08, style.size * 0.06, style.size * 0.06]

        headingFonts = scales.map { Fonts.reading(style.face, size: style.size * $0, bold: true) }
        headingMarkFonts = scales.map { Fonts.reading(style.face, size: style.size * $0, bold: false) }
        headingParagraphs = (0..<6).map { i in
            makeParagraph(lineHeight: lineHeights[i], before: befores[i], after: afters[i], size: style.size * scales[i])
        }
        firstHeadingParagraphs = (0..<6).map { i in
            makeParagraph(lineHeight: lineHeights[i], before: 0, after: afters[i], size: style.size * scales[i])
        }
    }

    private func baseAttributes(font: PlatformFont, color: PlatformColor, paragraph: NSParagraphStyle) -> [NSAttributedString.Key: Any] {
        let line = paragraph.minimumLineHeight
        let offset = line > 0 ? max(0, (line - (font.ascender - font.descender)) / 4) : 0
        return [.font: font, .foregroundColor: color, .paragraphStyle: paragraph, .baselineOffset: offset]
    }

    private func makeParagraph(
        lineHeight: CGFloat,
        before: CGFloat,
        after: CGFloat,
        align: NSTextAlignment = .natural,
        firstLineIndent: CGFloat = 0,
        headIndent: CGFloat = 0,
        tailIndent: CGFloat = 0,
        size: CGFloat? = nil
    ) -> NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        let pt = size ?? style.size
        let line = lineHeight * pt
        p.minimumLineHeight = line
        p.maximumLineHeight = line
        p.paragraphSpacingBefore = before
        p.paragraphSpacing = after
        p.alignment = align
        p.firstLineHeadIndent = firstLineIndent
        p.headIndent = headIndent
        if tailIndent != 0 { p.tailIndent = tailIndent }
        p.lineBreakStrategy = .pushOut
        return p
    }

    private static func blend(_ a: PlatformColor, toward b: PlatformColor, amount: CGFloat) -> PlatformColor {
        #if os(macOS)
        guard let c1 = a.usingColorSpace(.sRGB), let c2 = b.usingColorSpace(.sRGB) else { return a }
        return NSColor(
            srgbRed: c1.redComponent + (c2.redComponent - c1.redComponent) * amount,
            green: c1.greenComponent + (c2.greenComponent - c1.greenComponent) * amount,
            blue: c1.blueComponent + (c2.blueComponent - c1.blueComponent) * amount,
            alpha: 1
        )
        #else
        var r1: CGFloat = 0, g1: CGFloat = 0, b1: CGFloat = 0, a1: CGFloat = 0
        var r2: CGFloat = 0, g2: CGFloat = 0, b2: CGFloat = 0, a2: CGFloat = 0
        a.getRed(&r1, green: &g1, blue: &b1, alpha: &a1)
        b.getRed(&r2, green: &g2, blue: &b2, alpha: &a2)
        return UIColor(red: r1 + (r2 - r1) * amount, green: g1 + (g2 - g1) * amount, blue: b1 + (b2 - b1) * amount, alpha: 1)
        #endif
    }
}

/// TextKit 2 layout fragment that draws quiet block chrome (blockquote bars, rounded code cards,
/// thematic break rules) directly into the graphics context for visible paragraphs only.
nonisolated final class InklingLayoutFragment: NSTextLayoutFragment {
    let decoration: BlockDecoration
    let palette: Palette

    init(textElement: NSTextElement, range: NSTextRange?, decoration: BlockDecoration, palette: Palette) {
        self.decoration = decoration
        self.palette = palette
        super.init(textElement: textElement, range: range)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override var renderingSurfaceBounds: CGRect {
        let base = super.renderingSurfaceBounds
        switch decoration {
        case .blockquote:
            return base.union(CGRect(x: -20, y: 0, width: base.width + 20, height: layoutFragmentFrame.height))
        case .codeBlock:
            let containerWidth = textLayoutManager?.textContainer?.size.width ?? base.width
            let width = containerWidth > 0 && containerWidth < 10_000 ? containerWidth : base.width
            return base.union(CGRect(x: -18, y: -2, width: width + 4, height: layoutFragmentFrame.height + 4))
        case .thematicBreak:
            let containerWidth = textLayoutManager?.textContainer?.size.width ?? base.width
            let width = containerWidth > 0 && containerWidth < 10_000 ? containerWidth : base.width
            return base.union(CGRect(x: 0, y: 0, width: width, height: layoutFragmentFrame.height))
        }
    }

    override func draw(at point: CGPoint, in context: CGContext) {
        let bounds = layoutFragmentFrame
        let containerWidth = textLayoutManager?.textContainer?.size.width ?? bounds.width
        let width = containerWidth > 0 && containerWidth < 10_000 ? containerWidth : bounds.width

        context.saveGState()
        switch decoration {
        case .blockquote:
            let bar = CGRect(x: point.x - 15, y: point.y + 5, width: 2.5, height: max(0, bounds.height - 10))
            let path = CGPath(roundedRect: bar, cornerWidth: 1.25, cornerHeight: 1.25, transform: nil)
            context.setFillColor(palette.muted.withAlphaComponent(palette.isDark ? 0.42 : 0.34).cgColor)
            context.addPath(path)
            context.fillPath()
            context.restoreGState()
            super.draw(at: point, in: context)

        case .codeBlock(let top, let bottom):
            let topPad: CGFloat = top ? 4 : 0
            let bottomPad: CGFloat = bottom ? 4 : 0
            let rect = CGRect(
                x: point.x - 16,
                y: point.y + topPad,
                width: width,
                height: max(0, bounds.height - topPad - bottomPad)
            )
            let radius: CGFloat = 8
            let path = Self.roundedRect(rect, topRadius: top ? radius : 0, bottomRadius: bottom ? radius : 0)
            context.setFillColor(palette.segment.cgColor)
            context.addPath(path)
            context.fillPath()

            context.setStrokeColor(palette.hairline.cgColor)
            context.setLineWidth(1)
            // Left and right edges always; top/bottom only on first/last line of the block.
            context.move(to: CGPoint(x: rect.minX + 0.5, y: rect.minY + (top ? radius : 0)))
            context.addLine(to: CGPoint(x: rect.minX + 0.5, y: rect.maxY - (bottom ? radius : 0)))
            context.move(to: CGPoint(x: rect.maxX - 0.5, y: rect.minY + (top ? radius : 0)))
            context.addLine(to: CGPoint(x: rect.maxX - 0.5, y: rect.maxY - (bottom ? radius : 0)))
            if top {
                context.move(to: CGPoint(x: rect.minX + 0.5, y: rect.minY + radius))
                context.addArc(tangent1End: CGPoint(x: rect.minX + 0.5, y: rect.minY + 0.5), tangent2End: CGPoint(x: rect.minX + radius, y: rect.minY + 0.5), radius: radius)
                context.addLine(to: CGPoint(x: rect.maxX - radius, y: rect.minY + 0.5))
                context.addArc(tangent1End: CGPoint(x: rect.maxX - 0.5, y: rect.minY + 0.5), tangent2End: CGPoint(x: rect.maxX - 0.5, y: rect.minY + radius), radius: radius)
            }
            if bottom {
                context.move(to: CGPoint(x: rect.minX + 0.5, y: rect.maxY - radius))
                context.addArc(tangent1End: CGPoint(x: rect.minX + 0.5, y: rect.maxY - 0.5), tangent2End: CGPoint(x: rect.minX + radius, y: rect.maxY - 0.5), radius: radius)
                context.addLine(to: CGPoint(x: rect.maxX - radius, y: rect.maxY - 0.5))
                context.addArc(tangent1End: CGPoint(x: rect.maxX - 0.5, y: rect.maxY - 0.5), tangent2End: CGPoint(x: rect.maxX - 0.5, y: rect.maxY - radius), radius: radius)
            }
            context.strokePath()
            context.restoreGState()
            super.draw(at: point, in: context)

        case .thematicBreak:
            let midY = point.y + bounds.height / 2
            context.setStrokeColor(palette.hairline.cgColor)
            context.setLineWidth(1)
            context.move(to: CGPoint(x: point.x, y: midY))
            context.addLine(to: CGPoint(x: point.x + width, y: midY))
            context.strokePath()
            context.restoreGState()
            super.draw(at: point, in: context)
        }
    }

    private static func roundedRect(_ rect: CGRect, topRadius: CGFloat, bottomRadius: CGFloat) -> CGPath {
        let path = CGMutablePath()
        let minX = rect.minX, maxX = rect.maxX, minY = rect.minY, maxY = rect.maxY
        path.move(to: CGPoint(x: minX + topRadius, y: minY))
        path.addLine(to: CGPoint(x: maxX - topRadius, y: minY))
        if topRadius > 0 {
            path.addArc(tangent1End: CGPoint(x: maxX, y: minY), tangent2End: CGPoint(x: maxX, y: minY + topRadius), radius: topRadius)
        }
        path.addLine(to: CGPoint(x: maxX, y: maxY - bottomRadius))
        if bottomRadius > 0 {
            path.addArc(tangent1End: CGPoint(x: maxX, y: maxY), tangent2End: CGPoint(x: maxX - bottomRadius, y: maxY), radius: bottomRadius)
        }
        path.addLine(to: CGPoint(x: minX + bottomRadius, y: maxY))
        if bottomRadius > 0 {
            path.addArc(tangent1End: CGPoint(x: minX, y: maxY), tangent2End: CGPoint(x: minX, y: maxY - bottomRadius), radius: bottomRadius)
        }
        path.addLine(to: CGPoint(x: minX, y: minY + topRadius))
        if topRadius > 0 {
            path.addArc(tangent1End: CGPoint(x: minX, y: minY), tangent2End: CGPoint(x: minX + topRadius, y: minY), radius: topRadius)
        }
        path.closeSubpath()
        return path
    }
}

/// TextKit 2 layout-manager delegate that vends `InklingLayoutFragment` for decorated paragraphs.
nonisolated final class InklingLayoutDelegate: NSObject, NSTextLayoutManagerDelegate {
    private let lock = NSLock()
    private var _palette: Palette

    var palette: Palette {
        get { lock.withLock { _palette } }
        set { lock.withLock { _palette = newValue } }
    }

    init(palette: Palette) {
        self._palette = palette
    }

    func textLayoutManager(_ textLayoutManager: NSTextLayoutManager, textLayoutFragmentFor location: any NSTextLocation, in textElement: NSTextElement) -> NSTextLayoutFragment {
        if let paragraph = textElement as? NSTextParagraph,
           paragraph.attributedString.length > 0,
           let decoration = paragraph.attributedString.attribute(.inklingBlock, at: 0, effectiveRange: nil) as? BlockDecoration {
            return InklingLayoutFragment(textElement: textElement, range: textElement.elementRange, decoration: decoration, palette: palette)
        }
        return NSTextLayoutFragment(textElement: textElement, range: textElement.elementRange)
    }
}

import Foundation

/// One block of a chapter, as the reader draws it: a heading, a paragraph, one item of a list.
///
/// A chapter is the Markdown `inkling-books` writes (ADR 0006): blocks apart by blank lines,
/// `# <title>` first, pictures as `![alt](images/<id>/<n>.<ext> "caption")` on a line of their own,
/// footnotes as `[^n]` with their definitions at the end. `start` is where the block begins in the
/// chapter's text, in bytes, which is how highlights and places address it.
public struct Block: Identifiable, Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case heading(level: Int)
        case paragraph
        case quote
        /// One item of a list: its marker ("•" or "3.") and how deep it is nested, from 0.
        case item(marker: String, depth: Int)
        case image(path: String, alt: String, caption: String?)
        /// A scene break.
        case rule
        case code
        /// A footnote's definition.
        case note(label: String)
    }

    public let kind: Kind
    /// The block's words as inline Markdown (emphasis, links, footnote marks), or the code itself.
    public let text: String
    public let start: Int

    public var id: Int { start }

    public init(kind: Kind, text: String, start: Int) {
        self.kind = kind
        self.text = text
        self.start = start
    }
}

public enum Blocks {
    private typealias Line = (text: Substring, start: Int)

    /// Splits a chapter's Markdown into the blocks the reader draws, in order.
    public static func parse(_ markdown: String) -> [Block] {
        var lines: [Line] = []
        var offset = 0
        for line in markdown.split(separator: "\n", omittingEmptySubsequences: false) {
            lines.append((line, offset))
            offset += line.utf8.count + 1
        }

        var blocks: [Block] = []
        var i = 0
        while i < lines.count {
            let (line, start) = lines[i]
            let trimmed = line.drop { $0 == " " }
            if isBlank(line) {
                i += 1
                continue
            }
            if trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~") {
                let fence = trimmed.prefix(3)
                var body: [Substring] = []
                i += 1
                while i < lines.count, !lines[i].text.drop(while: { $0 == " " }).hasPrefix(fence) {
                    body.append(lines[i].text)
                    i += 1
                }
                i += 1
                blocks.append(Block(kind: .code, text: body.joined(separator: "\n"), start: start))
                continue
            }
            var group: [Line] = []
            while i < lines.count, !isBlank(lines[i].text) {
                group.append(lines[i])
                i += 1
            }
            blocks += parse(group: group)
        }
        return blocks
    }

    private static func parse(group: [Line]) -> [Block] {
        guard let (first, start) = group.first else { return [] }

        if let level = headingLevel(first) {
            let words = first.dropFirst(level).drop { $0 == " " }
            return [Block(kind: .heading(level: level), text: String(words), start: start)]
                + parse(group: Array(group.dropFirst()))
        }
        if group.count == 1, isRule(first) {
            return [Block(kind: .rule, text: "", start: start)]
        }
        if group.allSatisfy({ $0.text.hasPrefix(">") }) {
            let inner = group.map { line -> Substring in
                let rest = line.text.dropFirst()
                return rest.hasPrefix(" ") ? rest.dropFirst() : rest
            }
            return [Block(kind: .quote, text: paragraph(inner), start: start)]
        }
        if listMarker(first.drop { $0 == " " }) != nil {
            var items: [Block] = []
            for (line, offset) in group {
                let indent = line.prefix { $0 == " " }.count
                let body = line.dropFirst(indent)
                if let marker = listMarker(body) {
                    let words = body.dropFirst(marker.length)
                    items.append(Block(kind: .item(marker: marker.shown, depth: indent / 2), text: String(words), start: offset))
                } else if let last = items.popLast() {
                    items.append(Block(kind: last.kind, text: last.text + " " + body, start: last.start))
                }
            }
            return items
        }
        if let note = noteLabel(first) {
            let rest = group.dropFirst().map { $0.text.drop { $0 == " " } }
            return [Block(kind: .note(label: String(note.label)), text: paragraph([note.words] + rest), start: start)]
        }
        if group.count == 1, let image = image(first) {
            return [Block(kind: image, text: "", start: start)]
        }
        return [Block(kind: .paragraph, text: paragraph(group.map(\.text)), start: start)]
    }

    /// Joins a paragraph's lines: a soft break reads as a space, a hard break (two spaces or a
    /// backslash at the end) as a new line, and an empty line inside a quote as a new paragraph.
    private static func paragraph(_ lines: [Substring]) -> String {
        var out = ""
        var pending = ""
        for line in lines {
            if line.allSatisfy({ $0 == " " }) {
                if !out.isEmpty { pending = "\n\n" }
                continue
            }
            out += pending
            if line.hasSuffix("\\") {
                out += line.dropLast()
                pending = "\n"
            } else if line.hasSuffix("  ") {
                out += line.dropLast(2)
                pending = "\n"
            } else {
                out += line
                pending = " "
            }
        }
        return out
    }

    private static func isBlank(_ line: Substring) -> Bool {
        line.allSatisfy { $0 == " " || $0 == "\t" }
    }

    private static func headingLevel(_ line: Substring) -> Int? {
        let hashes = line.prefix { $0 == "#" }.count
        guard (1...6).contains(hashes) else { return nil }
        let after = line.dropFirst(hashes)
        return after.isEmpty || after.hasPrefix(" ") ? hashes : nil
    }

    private static func isRule(_ line: Substring) -> Bool {
        let marks = line.filter { $0 != " " }
        guard marks.count >= 3, let mark = marks.first, "-*_".contains(mark) else { return false }
        return marks.allSatisfy { $0 == mark }
    }

    private static func listMarker(_ line: Substring) -> (shown: String, length: Int)? {
        if line.hasPrefix("- ") || line.hasPrefix("* ") || line.hasPrefix("+ ") {
            return ("•", 2)
        }
        let digits = line.prefix { $0.isASCII && $0.isNumber }
        guard !digits.isEmpty, digits.count <= 9 else { return nil }
        let after = line.dropFirst(digits.count)
        guard after.hasPrefix(". ") || after.hasPrefix(") ") else { return nil }
        return ("\(digits).", digits.count + 2)
    }

    private static func noteLabel(_ line: Substring) -> (label: Substring, words: Substring)? {
        guard line.hasPrefix("[^"), let close = line.firstIndex(of: "]") else { return nil }
        let after = line[line.index(after: close)...]
        guard after.hasPrefix(":") else { return nil }
        let label = line[line.index(line.startIndex, offsetBy: 2)..<close]
        return label.isEmpty ? nil : (label, after.dropFirst().drop { $0 == " " })
    }

    private static func image(_ line: Substring) -> Block.Kind? {
        guard let match = try? #/^!\[(.*)\]\((\S+?)(?: "(.*)")?\)$/#.wholeMatch(in: line) else { return nil }
        let caption = match.output.3.map(String.init)
        return .image(path: String(match.output.2), alt: String(match.output.1), caption: caption?.isEmpty == true ? nil : caption)
    }
}

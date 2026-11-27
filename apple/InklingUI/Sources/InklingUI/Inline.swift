import Foundation
import SwiftUI

/// A block's inline Markdown as styled text: emphasis, strong, code and links as Foundation parses
/// them, and each footnote mark `[^n]` as a small raised `n`.
public enum Inline {
    private static let open: Character = "\u{E000}"
    private static let close: Character = "\u{E001}"

    public static func text(_ markdown: String, noteFont: Font? = nil) -> AttributedString {
        // Marks become private-use brackets before Foundation parses the rest, which would
        // otherwise read `[^n]` as text. A mark the book printed is escaped, `\[^n]`, and stays text.
        let marked = markdown.replacing(#/(\\?)\[\^([^\]\s]+)\]/#) { match in
            match.output.1.isEmpty ? "\(open)\(match.output.2)\(close)" : String(match.output.0)
        }
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible
        )
        var result = (try? AttributedString(markdown: marked, options: options)) ?? AttributedString(marked)
        while let start = result.characters.firstIndex(of: open),
              let end = result.characters[start...].firstIndex(of: close) {
            let label = String(result.characters[result.characters.index(after: start)..<end])
            var mark = AttributedString(label)
            mark.swiftUI.baselineOffset = 6
            if let noteFont { mark.swiftUI.font = noteFont }
            result.replaceSubrange(start...end, with: mark)
        }
        return result
    }

    /// The words alone, for accessibility and tests.
    public static func plain(_ markdown: String) -> String {
        String(text(markdown).characters)
    }
}

import InklingCore
import SwiftUI

/// One item: its title in the reading face, where it is from and how long it takes, how far
/// through it the reader is, and a book's cover.
struct ItemRow: View {
    @Environment(\.palette) private var palette
    let item: Item

    var body: some View {
        HStack(alignment: .center, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text(item.title)
                    .font(.serif(17, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(ink)
                    .lineLimit(3)
                    .multilineTextAlignment(.leading)
                HStack(spacing: 8) {
                    Text(details)
                        .font(.inter(13, relativeTo: .subheadline))
                        .foregroundStyle(muted)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Progress(item: item, ink: ink, track: muted.opacity(0.3))
                }
            }
            if let cover = item.cover, let image = Covers.image(cover) {
                image
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 42, height: 62)
                    .clipShape(.rect(cornerRadius: 3))
                    .overlay { RoundedRectangle(cornerRadius: 3).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
                    .shadow(color: .black.opacity(0.08), radius: 2, y: 1)
            }
        }
        .padding(.vertical, 12)
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }

    #if os(macOS)
    /// The Mac's label colours, which turn white on a selected row.
    private var ink: Color { .primary }
    private var muted: Color { .secondary }
    #else
    private var ink: Color { Color(palette.ink) }
    private var muted: Color { Color(palette.muted) }
    #endif

    private var details: String {
        var parts: [String] = []
        switch item.kind {
        case .article:
            if let site = item.site { parts.append(site) }
            if let minutes = item.minutes { parts.append("\(minutes) min") }
        case .epub, .pdf:
            if let author = item.author { parts.append(author) }
            parts.append(item.kind == .pdf ? "PDF" : "Book")
        }
        return parts.joined(separator: " · ")
    }
}

/// How far through: nothing before the first read, a short line while reading, a tick once done.
private struct Progress: View {
    let item: Item
    let ink: Color
    let track: Color

    var body: some View {
        if item.isFinished {
            Image(systemName: "checkmark")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(ink.opacity(0.7))
                .accessibilityLabel("Finished")
        } else if item.progress > 0.005 {
            Capsule()
                .fill(track)
                .frame(width: 30, height: 3)
                .overlay(alignment: .leading) {
                    Capsule().fill(ink).frame(width: max(3, 30 * CGFloat(item.progress)), height: 3)
                }
                .accessibilityLabel("\(Int(item.progress * 100)) percent read")
        }
    }
}

/// Book covers, read from disk once.
enum Covers {
    private static var cache: [String: Image] = [:]

    static func image(_ path: String) -> Image? {
        if let image = cache[path] { return image }
        #if os(macOS)
        guard let platform = NSImage(contentsOfFile: path) else { return nil }
        let image = Image(nsImage: platform)
        #else
        guard let platform = UIImage(contentsOfFile: path) else { return nil }
        let image = Image(uiImage: platform)
        #endif
        cache[path] = image
        return image
    }
}

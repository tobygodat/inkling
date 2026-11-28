import ImageIO
import InklingCore
import SwiftUI

/// One item as a card, in Compact (tight two-line default) or Comfortable density: its title in
/// the reading face, the site/author/blog name next to the reading length, progress, and a cover
/// for every row (lead image or site icon for articles; book cover for books, with a quiet
/// monogram tile when none is stored).
struct ItemRow: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    let item: Item

    var body: some View {
        let compact = shelf.look.density == .compact
        HStack(alignment: compact ? .center : .top, spacing: 14) {
            VStack(alignment: .leading, spacing: 0) {
                Text(item.title)
                    .font(.serif(compact ? 16.5 : 18, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Color(palette.ink))
                    .lineLimit(compact ? 1 : 2)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)

                if !compact, let excerpt {
                    Text(excerpt)
                        .font(.inter(13.5, relativeTo: .subheadline))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(1)
                        .padding(.top, 5)
                }

                HStack(spacing: 8) {
                    Text(meta)
                        .font(.inter(12.5, relativeTo: .caption))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(1)
                    Progress(item: item, ink: Color(palette.ink), track: Color(palette.muted).opacity(0.25))
                    Spacer(minLength: 0)
                }
                .padding(.top, compact ? 4 : 8)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            cover(compact: compact)
        }
        .padding(.vertical, compact ? 10 : 14)
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }

    /// Always renders a cover: lead image, site icon, book cover, or a quiet serif monogram tile.
    @ViewBuilder private func cover(compact: Bool) -> some View {
        if item.kind == .article {
            let side: CGFloat = compact ? 44 : 68
            let radius: CGFloat = compact ? 8 : 10
            if let path = item.picture, let image = Thumbnails.image(path) {
                image
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: side, height: side)
                    .clipShape(.rect(cornerRadius: radius))
                    .overlay { RoundedRectangle(cornerRadius: radius).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
            } else if let path = item.icon, let image = Thumbnails.image(path) {
                let inset: CGFloat = compact ? 26 : 36
                ZStack {
                    RoundedRectangle(cornerRadius: radius, style: .continuous)
                        .fill(Color(palette.segment))
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: inset, height: inset)
                        .clipShape(.rect(cornerRadius: compact ? 5 : 7))
                }
                .frame(width: side, height: side)
                .overlay { RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
            } else {
                placeholder(width: side, height: side, radius: radius, letter: initial(of: source ?? item.title), fontSize: compact ? 18 : 24)
            }
        } else {
            let width: CGFloat = compact ? 34 : 48
            let height: CGFloat = compact ? 46 : 68
            let radius: CGFloat = 4
            if let path = item.cover, let image = Thumbnails.image(path) {
                image
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: width, height: height)
                    .clipShape(.rect(cornerRadius: radius))
                    .overlay { RoundedRectangle(cornerRadius: radius).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
                    .shadow(color: .black.opacity(0.1), radius: 2, y: 1)
            } else {
                placeholder(width: width, height: height, radius: radius, letter: initial(of: item.title), fontSize: compact ? 16 : 22)
            }
        }
    }

    private func placeholder(width: CGFloat, height: CGFloat, radius: CGFloat, letter: String, fontSize: CGFloat) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: radius, style: .continuous)
                .fill(Color(palette.segment))
            Text(letter)
                .font(.serif(fontSize, weight: .semibold, relativeTo: .headline))
                .foregroundStyle(Color(palette.muted))
        }
        .frame(width: width, height: height)
        .overlay { RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
    }

    private func initial(of text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.first(where: \.isLetter).map { String($0).uppercased() } ?? "I"
    }

    /// The website, blog or author name for an article, or a book's author.
    private var source: String? {
        let raw = item.kind == .article ? (item.site ?? item.author) : item.author
        let trimmed = raw?.trimmingCharacters(in: .whitespacesAndNewlines)
        return (trimmed?.isEmpty == false) ? trimmed : nil
    }

    private var excerpt: String? {
        guard let excerpt = item.excerpt?.trimmingCharacters(in: .whitespacesAndNewlines), !excerpt.isEmpty else { return nil }
        return excerpt
    }

    /// Source next to reading length, e.g. "Daring Fireball · 4 min" or "Jane Austen · 8h 40m".
    private var meta: String {
        let parts: [String]
        switch item.kind {
        case .article:
            parts = [source, item.readingLength].compactMap(\.self)
        case .epub:
            parts = [source, item.readingLength ?? "Book"].compactMap(\.self)
        case .pdf:
            parts = [source ?? "PDF", item.readingLength].compactMap(\.self)
        }
        return parts.isEmpty ? (item.kind == .pdf ? "PDF" : "Book") : parts.joined(separator: " · ")
    }
}

private extension Item {
    /// Human-readable reading time: "4 min" under an hour, "2h 15m" or "2 hr" at an hour or more.
    var readingLength: String? {
        guard let minutes else { return nil }
        if minutes < 60 { return "\(minutes) min" }
        let hours = minutes / 60
        let rest = minutes % 60
        return rest == 0 ? "\(hours) hr" : "\(hours)h \(rest)m"
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
                .frame(width: 26, height: 3)
                .overlay(alignment: .leading) {
                    Capsule().fill(ink).frame(width: max(3, 26 * CGFloat(item.progress)), height: 3)
                }
                .accessibilityLabel("\(Int(item.progress * 100)) percent read")
        }
    }
}

/// Pictures for the rows, decoded once at the size a row draws them, off the full-size files.
enum Thumbnails {
    private static var cache: [String: Image] = [:]

    static func image(_ path: String) -> Image? {
        if let image = cache[path] { return image }
        let options = [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: 300] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(URL(filePath: path) as CFURL, nil),
              let thumbnail = CGImageSourceCreateThumbnailAtIndex(source, 0, options) else { return nil }
        let image = Image(decorative: thumbnail, scale: 1)
        cache[path] = image
        return image
    }
}

import ImageIO
import InklingCore
import SwiftUI

/// One item as a card: where it is from, its title in the reading face, a line or two of what it
/// says, how long it takes and how far through it the reader is, and its picture: an article's
/// first one, a book's cover. Each part that's missing leaves no gap.
struct ItemRow: View {
    @Environment(\.palette) private var palette
    let item: Item

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            VStack(alignment: .leading, spacing: 0) {
                if let from {
                    Text(from)
                        .font(.inter(12, weight: .medium, relativeTo: .caption))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(1)
                        .padding(.bottom, 5)
                }
                Text(item.title)
                    .font(.serif(18, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Color(palette.ink))
                    .lineLimit(3)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                if let excerpt {
                    Text(excerpt)
                        .font(.inter(14, relativeTo: .subheadline))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(2)
                        .lineSpacing(2)
                        .padding(.top, 6)
                }
                HStack(spacing: 10) {
                    Text(details)
                        .font(.inter(12, relativeTo: .caption))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(1)
                    Progress(item: item, ink: Color(palette.ink), track: Color(palette.muted).opacity(0.25))
                    Spacer(minLength: 0)
                }
                .padding(.top, 10)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            picture
        }
        .padding(.vertical, 16)
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder private var picture: some View {
        if item.kind == .article, let path = item.picture, let image = Thumbnails.image(path) {
            image
                .resizable()
                .aspectRatio(contentMode: .fill)
                .frame(width: 84, height: 84)
                .clipShape(.rect(cornerRadius: 10))
                .overlay { RoundedRectangle(cornerRadius: 10).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
        } else if let path = item.cover, let image = Thumbnails.image(path) {
            image
                .resizable()
                .aspectRatio(contentMode: .fill)
                .frame(width: 56, height: 84)
                .clipShape(.rect(cornerRadius: 3))
                .overlay { RoundedRectangle(cornerRadius: 3).strokeBorder(Color(palette.hairline), lineWidth: 0.5) }
                .shadow(color: .black.opacity(0.1), radius: 3, y: 1)
        }
    }

    /// The site an article is from, or a book's author.
    private var from: String? {
        item.kind == .article ? item.site : item.author
    }

    private var excerpt: String? {
        guard let excerpt = item.excerpt?.trimmingCharacters(in: .whitespacesAndNewlines), !excerpt.isEmpty else { return nil }
        return excerpt
    }

    private var details: String {
        switch item.kind {
        case .article:
            [item.author, item.minutes.map { "\($0) min" }].compactMap(\.self).joined(separator: " · ")
        case .epub: "Book"
        case .pdf: "PDF"
        }
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

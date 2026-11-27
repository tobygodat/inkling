import InklingCore
import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// A book or an article, one chapter at a time, in one centred column on Inkling's paper.
public struct ReaderView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.scenePhase) private var phase
    private let id: String

    @State private var book: Book?
    @State private var problem: String?
    @State private var chapter = 0
    @State private var blocks: [Block] = []
    @State private var fraction = 0.0
    @State private var kept = 0.0
    @State private var restore: Double?
    @State private var position = ScrollPosition(edge: .top)

    public init(id: String) {
        self.id = id
    }

    public var body: some View {
        Group {
            if let book {
                page(book)
            } else if let problem {
                ContentUnavailableView("This couldn’t be opened", systemImage: "exclamationmark.triangle", description: Text(problem))
            } else {
                ProgressView()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Palette.ground)
        .task(id: id) { await open() }
        .onDisappear { keep() }
        .onChange(of: phase) { _, now in if now != .active { keep() } }
    }

    private func page(_ book: Book) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Typeface.body * 0.9) {
                ForEach(blocks) { block in
                    BlockView(block: block, folder: shelf.folder)
                }
                if chapter + 1 < book.chapters.count {
                    Button {
                        go(to: chapter + 1)
                    } label: {
                        Label("Next: \(title(of: chapter + 1, in: book))", systemImage: "arrow.right")
                            .font(Typeface.ui(14, weight: .medium))
                    }
                    .buttonStyle(.bordered)
                    .padding(.top, Typeface.body)
                }
            }
            .textSelection(.enabled)
            .foregroundStyle(Palette.ink)
            .tint(Palette.link)
            .frame(maxWidth: 700, alignment: .leading)
            .padding(.horizontal, 24)
            .padding(.vertical, 40)
            .frame(maxWidth: .infinity)
        }
        .scrollPosition($position)
        .onScrollGeometryChange(for: Scroll.self) { geometry in
            Scroll(offset: geometry.contentOffset.y, range: geometry.contentSize.height - geometry.containerSize.height)
        } action: { _, scroll in
            guard scroll.range > 0 else { return }
            if let restore {
                self.restore = nil
                position.scrollTo(y: restore * scroll.range)
                return
            }
            fraction = min(1, max(0, scroll.offset / scroll.range))
            if abs(fraction - kept) > 0.05 { keep() }
        }
        .navigationTitle(book.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            if book.chapters.count > 1 {
                ToolbarItemGroup {
                    Button("Previous Chapter", systemImage: "chevron.left") { go(to: chapter - 1) }
                        .disabled(chapter == 0)
                        .keyboardShortcut("[", modifiers: .command)
                    Menu {
                        ForEach(book.chapters.indices, id: \.self) { index in
                            Button(title(of: index, in: book)) { go(to: index) }
                        }
                    } label: {
                        Label("Chapters", systemImage: "list.bullet")
                    }
                    Button("Next Chapter", systemImage: "chevron.right") { go(to: chapter + 1) }
                        .disabled(chapter + 1 >= book.chapters.count)
                        .keyboardShortcut("]", modifiers: .command)
                }
            }
        }
    }

    private func open() async {
        do {
            let opened = try await shelf.book(id)
            let place = shelf.item(id)
            let at = min(Int(place?.chapter ?? 0), max(0, opened.chapters.count - 1))
            chapter = at
            blocks = opened.chapters.isEmpty ? [] : Blocks.parse(opened.chapters[at].text)
            fraction = Double(place?.fraction ?? 0)
            kept = fraction
            restore = fraction > 0 ? fraction : nil
            book = opened
        } catch {
            problem = Shelf.describe(error)
        }
    }

    private func go(to index: Int) {
        guard let book, book.chapters.indices.contains(index), index != chapter else { return }
        keep()
        chapter = index
        blocks = Blocks.parse(book.chapters[index].text)
        fraction = 0
        restore = nil
        position.scrollTo(edge: .top)
        keep()
    }

    private func keep() {
        guard book != nil else { return }
        kept = fraction
        let (id, chapter, fraction) = (id, chapter, fraction)
        Task { await shelf.keep(id, chapter: chapter, fraction: fraction) }
    }

    private func title(of index: Int, in book: Book) -> String {
        book.chapters[index].title ?? "Chapter \(index + 1)"
    }

    private struct Scroll: Equatable {
        let offset: Double
        let range: Double
    }
}

/// One block of a chapter in Inkling's reading type.
struct BlockView: View {
    let block: Block
    let folder: URL

    private let size = Typeface.body

    var body: some View {
        switch block.kind {
        case .heading(let level):
            Text(Inline.text(block.text))
                .font(Typeface.serif(heading(level), weight: .semibold))
                .padding(.top, level == 1 ? 0 : size * 0.6)
                .accessibilityAddTraits(.isHeader)
        case .paragraph:
            prose(block.text)
        case .quote:
            prose(block.text)
                .foregroundStyle(Palette.muted)
                .padding(.leading, 20)
                .overlay(alignment: .leading) {
                    Rectangle().fill(Palette.hairline).frame(width: 3)
                }
        case .item(let marker, let depth):
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(marker)
                    .font(Typeface.serif(size))
                    .foregroundStyle(Palette.muted)
                prose(block.text)
            }
            .padding(.leading, CGFloat(depth) * 24)
        case .image(let path, let alt, let caption):
            Picture(file: folder.appending(path: path), alt: alt, caption: caption)
        case .rule:
            Text("⁂")
                .font(Typeface.serif(size))
                .foregroundStyle(Palette.muted)
                .frame(maxWidth: .infinity)
                .accessibilityLabel("Scene break")
        case .code:
            Text(block.text)
                .font(.system(size: size * 0.75, design: .monospaced))
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Palette.segment, in: RoundedRectangle(cornerRadius: 6))
        case .note(let label):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(label)
                    .font(Typeface.ui(13, weight: .medium))
                Text(Inline.text(block.text))
                    .font(Typeface.serif(size * 0.82))
                    .lineSpacing(Typeface.leading(size * 0.82))
            }
            .foregroundStyle(Palette.muted)
        }
    }

    private func prose(_ text: String) -> some View {
        Text(Inline.text(text, noteFont: Typeface.ui(size * 0.6, weight: .medium)))
            .font(Typeface.serif(size))
            .lineSpacing(Typeface.leading(size))
            .fixedSize(horizontal: false, vertical: true)
    }

    private func heading(_ level: Int) -> CGFloat {
        switch level {
        case 1: Typeface.title
        case 2: size * 1.35
        case 3: size * 1.15
        default: size
        }
    }
}

/// A chapter's picture, read from the library's folder off the main thread.
private struct Picture: View {
    let file: URL
    let alt: String
    let caption: String?

    @State private var image: Image?
    @State private var width: CGFloat = 0

    var body: some View {
        VStack(spacing: 8) {
            if let image {
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: width > 0 ? width : nil)
                    .accessibilityLabel(alt)
            }
            if let caption {
                Text(Inline.text(caption))
                    .font(Typeface.ui(13))
                    .foregroundStyle(Palette.muted)
                    .multilineTextAlignment(.center)
            }
        }
        .frame(maxWidth: .infinity)
        .task(id: file) {
            let file = file
            guard let data = await Task.detached(operation: { try? Data(contentsOf: file) }).value else { return }
            #if canImport(AppKit)
            guard let picture = NSImage(data: data) else { return }
            image = Image(nsImage: picture)
            width = picture.size.width
            #else
            guard let picture = UIImage(data: data) else { return }
            image = Image(uiImage: picture)
            width = picture.size.width
            #endif
        }
    }
}

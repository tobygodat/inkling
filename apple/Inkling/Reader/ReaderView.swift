import InklingCore
import SwiftUI

/// The reader: the item's text in reading's face on its paper, the place kept as it scrolls, and
/// how far through at the foot.
struct ReaderView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.colorScheme) private var scheme
    @Environment(\.dismiss) private var dismiss
    let item: Item
    @State private var model: ReaderModel?
    @State private var failed: String?
    @State private var choosing = false

    var body: some View {
        let look = shelf.look
        let palette = Self.palette(look.tone, shelf: shelf, scheme: scheme)
        ZStack(alignment: .bottom) {
            Color(palette.ground).ignoresSafeArea()
            if let model {
                ReaderText(model: model, palette: palette, column: CGFloat(look.pageWidth))
                    .id(ObjectIdentifier(model))
                    .ignoresSafeArea(edges: .bottom)
                    .transition(.opacity)
            } else if let failed {
                Text(failed)
                    .font(.inter(14))
                    .foregroundStyle(Color(palette.muted))
                    .padding(40)
                    .frame(maxHeight: .infinity)
            }
        }
        .overlay(alignment: .top) {
            if let model { ProgressLine(progress: model.progress, palette: palette) }
        }
        .animation(.easeOut(duration: 0.2), value: model.map(ObjectIdentifier.init))
        .environment(\.palette, palette)
        .toolbar {
            if let model {
                #if os(iOS)
                ToolbarItem(placement: .principal) { Place(model: model, palette: palette) }
                ToolbarItemGroup(placement: .bottomBar) {
                    Highlights(model: model)
                    Spacer()
                    Button("Archive", systemImage: "archivebox") {
                        shelf.setArchived(item, true)
                        dismiss()
                    }
                    .accessibilityIdentifier("archive-item")
                    appearanceButton
                }
                #else
                ToolbarItem(placement: .primaryAction) {
                    appearanceButton
                        .popover(isPresented: $choosing, arrowEdge: .bottom) { AppearancePanel() }
                }
                ToolbarItem(placement: .primaryAction) { Highlights(model: model) }
                #endif
            }
        }
        #if os(iOS)
        .toolbarVisibility(model?.chromeHidden == true ? .hidden : .visible, for: .navigationBar, .bottomBar)
        #endif
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .navigationTitle("")
        #else
        .navigationTitle(item.title)
        .navigationSubtitle(model.map(Place.words) ?? "")
        #endif
        #if os(iOS)
        .sheet(isPresented: $choosing) {
            AppearancePanel()
                .presentationDetents([.height(330)])
        }
        #endif
        .task(id: Setting(id: item.id, look: look, dark: palette.isDark)) { await load(look: look, palette: palette) }
        #if os(iOS)
        .onChange(of: look.tone, initial: true) { navigator.readerScheme = Self.scheme(of: look.tone, palette: palette) }
        .onDisappear { navigator.readerScheme = nil }
        #endif
        .onDisappear { shelf.flushPlaces() }
        #if os(macOS)
        .focusedSceneValue(\.reader, model)
        #endif
    }

    private var appearanceButton: some View {
        Button("Appearance", systemImage: "textformat.size") { choosing = true }
            .accessibilityIdentifier("appearance")
    }

    private func load(look: Look, palette: Palette) async {
        do {
            let book = try await shelf.open(item.id)
            #if os(macOS)
            let scale: CGFloat = 1
            let lineScale: CGFloat = 1
            let column = CGFloat(look.pageWidth)
            let title: CGFloat = 36
            #else
            let scale: CGFloat = 19 / 21
            let lineScale: CGFloat = 1.65 / 1.75
            let column = min(CGFloat(look.pageWidth), UIScreen.main.bounds.width - 48)
            let title: CGFloat = 28
            #endif
            let set = Typesetting(
                face: look.face,
                size: CGFloat(look.textSize) * scale,
                lineHeight: CGFloat(look.lineSpacing) / 100 * lineScale,
                titleSize: title * CGFloat(look.textSize) / 21,
                palette: palette,
                column: column
            )
            let byline = item.kind == .article ? [item.author, item.site, item.minutes.map { "\($0) min" }].compactMap(\.self).joined(separator: "  ·  ") : nil
            let folder = URL(filePath: shelf.library.folder())
            let rendered = try await Shelf.offMain { Renderer.render(book, set: set, folder: folder, byline: byline) }
            guard !Task.isCancelled else { return }
            model = ReaderModel(item: shelf.item(item.id) ?? item, rendered: rendered, shelf: shelf)
        } catch {
            failed = Shelf.words(error)
        }
    }

    /// What the text is drawn from: a new look or Light and Dark changing redraws it at the place.
    private struct Setting: Equatable {
        let id: String
        let look: Look
        let dark: Bool
    }

    /// The Light or Dark a paper tone asks of the bars around it. Auto asks nothing: its paper
    /// follows the app's own Light or Dark, so it must never decide that.
    static func scheme(of tone: Tone, palette: Palette) -> ColorScheme? {
        switch tone {
        case .light, .dark: nil
        default: palette.isDark ? .dark : .light
        }
    }

    /// Reading's look: Light and Dark follow the system's appearance; a paper tone is its own.
    static func palette(_ tone: Tone, shelf: Shelf, scheme: ColorScheme) -> Palette {
        switch tone {
        case .light, .dark: Palette(shelf.looks.colors(tone: scheme == .dark ? .dark : .light))
        default: Palette(shelf.looks.colors(tone: tone))
        }
    }
}

/// How far through, in muted ink in the bar above the text: the time left in an article, the
/// chapter and the share read in a book.
private struct Place: View {
    let model: ReaderModel
    let palette: Palette

    var body: some View {
        Text(Self.words(model))
            .font(.inter(13, weight: .medium, relativeTo: .footnote))
            .monospacedDigit()
            .foregroundStyle(Color(palette.muted))
            .contentTransition(.numericText())
            .animation(.snappy(duration: 0.25), value: Self.words(model))
            .accessibilityIdentifier("place")
    }

    static func words(_ model: ReaderModel) -> String {
        let percent = "\(Int((model.progress * 100).rounded()))%"
        if model.item.kind == .article {
            guard let left = model.minutesLeft, model.progress < 0.98 else { return "Finished" }
            return left == 0 ? percent : "\(left) min left"
        }
        return "Chapter \(model.chapter + 1) of \(model.rendered.chapters.count) · \(percent)"
    }
}

/// How far through, as a hairline across the top of the reader.
private struct ProgressLine: View {
    let progress: Double
    let palette: Palette

    var body: some View {
        Rectangle()
            .fill(Color(palette.hairline))
            .frame(height: 2)
            .overlay(alignment: .leading) {
                GeometryReader { space in
                    Rectangle()
                        .fill(Color(palette.ink).opacity(0.55))
                        .frame(width: space.size.width * progress)
                }
            }
            .animation(.linear(duration: 0.15), value: progress)
            .accessibilityHidden(true)
    }
}

/// The passages highlighted in the open item, each one a way back to where it is.
private struct Highlights: View {
    let model: ReaderModel

    var body: some View {
        Menu {
            if model.highlights.isEmpty {
                Text("Select a passage and choose Highlight")
            }
            ForEach(model.highlights, id: \.self) { passage in
                Button(passage.words.count > 80 ? String(passage.words.prefix(80)) + "…" : passage.words) { model.go(to: passage) }
                    .accessibilityIdentifier("passage")
            }
        } label: {
            Label("Highlights", systemImage: "highlighter")
        }
        .accessibilityIdentifier("highlights")
        .accessibilityValue("\(model.highlights.count)")
    }
}

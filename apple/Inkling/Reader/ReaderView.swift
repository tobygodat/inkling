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
    /// The reader's width on iPhone, which a picture is fitted to.
    @State private var width: CGFloat = 0

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
        #if os(iOS)
        .overlay { EdgeFades(ground: Color(palette.ground), bottomShown: model?.chromeHidden != true) }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        #endif
        .overlay(alignment: .top) {
            if let model { ProgressLine(progress: model.progress, palette: palette) }
        }
        .animation(.easeOut(duration: 0.2), value: model.map(ObjectIdentifier.init))
        .environment(\.palette, palette)
        .toolbar {
            if let model {
                #if os(iOS)
                ToolbarItem(placement: .principal) { Place(model: model, palette: palette) }
                ToolbarItemGroup(placement: .primaryAction) {
                    if model.chapters > 1 { Contents(model: model) }
                    appearanceButton
                    Highlights(model: model)
                }
                #else
                if model.chapters > 1 {
                    ToolbarItem(placement: .primaryAction) { Contents(model: model) }
                }
                ToolbarItem(placement: .primaryAction) {
                    appearanceButton
                        .popover(isPresented: $choosing, arrowEdge: .bottom) { AppearancePanel() }
                }
                ToolbarItem(placement: .primaryAction) { Highlights(model: model) }
                #endif
            }
        }
        #if os(iOS)
        .toolbarVisibility(model?.chromeHidden == true ? .hidden : .visible, for: .navigationBar, .tabBar)
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
        .task(id: Setting(id: item.id, look: look, dark: palette.isDark, width: width)) { await load(look: look, palette: palette) }
        #if os(iOS)
        .onChange(of: look.tone, initial: true) { navigator.readerScheme = Self.scheme(of: look.tone, palette: palette) }
        .onChange(of: model?.chromeHidden) { navigator.immersed = model?.chromeHidden == true }
        .onDisappear {
            navigator.readerScheme = nil
            navigator.immersed = false
        }
        #endif
        .onDisappear { shelf.flushPlaces() }
        .onAppear { if let model { shelf.asker.place = .reading(model) } }
        .onChange(of: navigator.passage?.passage) { showPassage() }
        #if os(macOS)
        .focusedSceneValue(\.reader, model)
        #endif
    }

    /// Goes to the passage a citation or a highlight elsewhere asked for, once this item is open.
    private func showPassage() {
        guard let model, let wanted = navigator.passage, wanted.id == item.id else { return }
        navigator.passage = nil
        model.motion = .fade
        model.go(to: wanted.passage)
    }

    private var appearanceButton: some View {
        Button("Appearance", systemImage: "textformat.size") { choosing = true }
            .accessibilityIdentifier("appearance")
    }

    private func load(look: Look, palette: Palette) async {
        #if os(iOS)
        guard width > 0 else { return }
        #endif
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
            let column = min(CGFloat(look.pageWidth), width - 48)
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
            let opened = ReaderModel(item: shelf.item(item.id) ?? item, rendered: rendered, shelf: shelf)
            opened.done = { [item, shelf, navigator, dismiss] in
                shelf.setArchived(item, true)
                #if os(iOS)
                dismiss()
                #else
                navigator.selection = nil
                #endif
            }
            model = opened
            shelf.asker.place = .reading(opened)
            showPassage()
        } catch {
            failed = Shelf.words(error)
        }
    }

    /// What the text is drawn from: a new look, Light and Dark changing, or on iPhone the reader's
    /// width, redraws it at the place.
    private struct Setting: Equatable {
        let id: String
        let look: Look
        let dark: Bool
        let width: CGFloat
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

#if os(iOS)
/// The text dims into the paper at the edges, so a line never reads through the bars' glass. At
/// the top the text stops under the progress hairline, and the fade takes the line passing under it
/// over the inset the column keeps above its first line, so nothing at rest is dimmed. The fade at
/// the bottom goes with the bars.
private struct EdgeFades: View {
    let ground: Color
    let bottomShown: Bool

    var body: some View {
        VStack(spacing: 0) {
            fade(from: .top).frame(height: 26)
            Spacer(minLength: 0)
            GeometryReader { space in
                fade(from: .bottom)
                    .frame(height: space.safeAreaInsets.bottom + 100)
                    .frame(maxHeight: .infinity, alignment: .bottom)
            }
            .ignoresSafeArea()
            .opacity(bottomShown ? 1 : 0)
            .animation(.easeInOut(duration: 0.25), value: bottomShown)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func fade(from edge: UnitPoint) -> LinearGradient {
        let stops: [Gradient.Stop] = edge == .top
            ? [.init(color: ground, location: 0), .init(color: ground.opacity(0), location: 1)]
            : [.init(color: ground, location: 0), .init(color: ground, location: 0.5), .init(color: ground.opacity(0), location: 1)]
        return LinearGradient(stops: stops, startPoint: edge, endPoint: edge == .top ? .bottom : .top)
    }
}
#endif

/// The chapters in order, and the chapter before and after this one on the side nearest the
/// button: below the list on iPhone, where the menu rises from the bottom bar, and above it on the
/// Mac.
private struct Contents: View {
    let model: ReaderModel

    var body: some View {
        Menu {
            #if os(iOS)
            chapters
            Divider()
            steps
            #else
            steps
            Divider()
            chapters
            #endif
        } label: {
            Label("Contents", systemImage: "list.bullet")
        }
        .menuOrder(.fixed)
        .accessibilityIdentifier("contents")
    }

    private var chapters: some View {
        Picker("Chapters", selection: Binding(get: { model.chapter }, set: { model.go(toChapter: $0, .fade) })) {
            ForEach(model.rendered.titles.indices, id: \.self) { index in
                Text(model.rendered.titles[index]).tag(index)
            }
        }
        .pickerStyle(.inline)
    }

    @ViewBuilder private var steps: some View {
        Button("Previous Chapter", systemImage: "chevron.backward") { model.go(toChapter: model.chapter - 1, .fade) }
            .disabled(model.chapter == 0)
        Button("Next Chapter", systemImage: "chevron.forward") { model.go(toChapter: model.chapter + 1, .fade) }
            .disabled(model.chapter + 1 >= model.chapters)
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

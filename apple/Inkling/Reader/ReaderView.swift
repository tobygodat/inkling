import InklingCore
import SwiftUI

/// The reader: the item's text in reading's face on its paper, the place kept as it scrolls, and
/// how far through at the foot.
struct ReaderView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.colorScheme) private var scheme
    @Environment(\.dismiss) private var dismiss
    @Environment(\.displayScale) private var displayScale
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let item: Item
    @State private var model: ReaderModel?
    @State private var failed: String?
    @State private var choosing = false
    @State private var listing = false
    /// The reader's width on iPhone, which a picture is fitted to.
    @State private var width: CGFloat = 0
    /// The paper the text on screen is drawn on. A new one takes over only with the text redrawn in
    /// it, so the screen never shows the text in one paper on another.
    @State private var drawn: Drawn?
    /// The page as it was before each change of paper, still washing away.
    @State private var spreads: [Spread] = []
    /// The item as the bridge opened it, kept so a new look redraws it without reading it again.
    @State private var opened: Book?
    /// The look the text on screen was laid out in, but for its paper.
    @State private var measure: Measure?

    var body: some View {
        let look = shelf.look
        let wanted = Self.palette(look.tone, shelf: shelf, scheme: scheme)
        let palette = drawn?.palette ?? wanted
        ZStack(alignment: .bottom) {
            Color(palette.ground).ignoresSafeArea()
            if let model {
                ReaderText(model: model, palette: palette, column: CGFloat(look.pageWidth))
                    .id(ObjectIdentifier(model))
                    #if os(iOS)
                    .ignoresSafeArea()
                    #else
                    .ignoresSafeArea(edges: .bottom)
                    #endif
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
        .overlay { EdgeFades(ground: Color(palette.ground), model: model) }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        #endif
        .overlay(alignment: .top) {
            if let model { ProgressLine(model: model, palette: palette) }
        }
        .overlay {
            ForEach(spreads) { spread in
                SpreadOverlay(spread: spread, model: model) { spreads.removeAll { $0.id == spread.id } }
            }
            .ignoresSafeArea()
        }
        .animation(reduceMotion ? .easeOut(duration: 0.15) : .smooth(duration: 0.22), value: model.map(ObjectIdentifier.init))
        .environment(\.palette, palette)
        .toolbar {
            if let model {
                #if os(iOS)
                ToolbarItem(placement: .principal) { Place(model: model, palette: palette) }
                ToolbarItemGroup(placement: .primaryAction) {
                    if model.chapters > 1 { contentsButton }
                    appearanceButton
                    Highlights(model: model)
                }
                #else
                if model.chapters > 1 {
                    ToolbarItem(placement: .primaryAction) {
                        contentsButton
                            .popover(isPresented: $listing, arrowEdge: .bottom) { ContentsPanel(model: model).frame(height: min(420, 64 + CGFloat(model.chapters) * 44)) }
                    }
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
        .modifier(ChromeVisibility(model: model))
        #endif
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        .navigationTitle("")
        #else
        .navigationTitle(item.title)
        .modifier(Subtitle(model: model))
        #endif
        #if os(iOS)
        .sheet(isPresented: $choosing) {
            AppearancePanel()
                .presentationDetents([.height(330)])
        }
        .sheet(isPresented: $listing) {
            if let model {
                ContentsPanel(model: model)
                    .presentationDetents([.height(330), .large])
            }
        }
        #endif
        .task(id: Setting(id: item.id, look: look, dark: wanted.isDark, width: width, scale: displayScale)) { await load(look: look, palette: wanted) }
        #if os(iOS)
        .onChange(of: drawn, initial: true) { navigator.readerScheme = Self.scheme(of: drawn?.tone ?? look.tone, palette: palette) }
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

    private var contentsButton: some View {
        Button("Contents", systemImage: "list.bullet") { listing = true }
            .accessibilityIdentifier("contents")
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
            let book: Book
            if let opened, opened.id == item.id {
                book = opened
            } else {
                book = try await shelf.open(item.id)
                opened = book
            }
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
                column: column,
                scale: displayScale
            )
            let byline = item.kind == .article ? [item.author, item.site, item.minutes.map { "\($0) min" }].compactMap(\.self).joined(separator: "  ·  ") : nil
            let folder = URL(filePath: shelf.library.folder())
            let rendered = try await Shelf.offMain { Renderer.render(book, set: set, folder: folder, byline: byline) }
            guard !Task.isCancelled else { return }
            let old = model
            var form = look
            form.tone = .light
            let measure = Measure(look: form, width: width, scale: displayScale)
            defer { self.measure = measure }
            if let old, old.item.id == item.id, measure == self.measure, let before = drawn, before.palette != palette {
                // Only the paper changed: the text on screen takes the new colours where it stands,
                // under the page as it was, which then washes away.
                if let image = old.snapshot?() { spreads.insert(Spread(image: image), at: 0) }
                var still = Transaction()
                still.disablesAnimations = true
                withTransaction(still) {
                    drawn = Drawn(tone: look.tone, palette: palette)
                    old.recolour(rendered)
                }
                return
            }
            let reader = ReaderModel(item: shelf.item(item.id) ?? item, rendered: rendered, shelf: shelf, at: old?.item.id == item.id ? old?.anchor : nil)
            reader.done = { [item, shelf, navigator, dismiss] in
                shelf.setArchived(item, true)
                #if os(iOS)
                dismiss()
                #else
                navigator.selection = nil
                #endif
            }
            let now = Drawn(tone: look.tone, palette: palette)
            if let before = drawn, before.palette != palette, let image = old?.snapshot?() {
                // The page as it was goes on top before the text under it is redrawn, then washes
                // away. One still washing stays above it, so a change of mind carries on from there.
                spreads.insert(Spread(image: image), at: 0)
                var still = Transaction()
                still.disablesAnimations = true
                withTransaction(still) {
                    drawn = now
                    model = reader
                }
            } else {
                drawn = now
                model = reader
            }
            shelf.asker.place = .reading(reader)
            showPassage()
        } catch {
            failed = Shelf.words(error)
        }
    }

    /// Everything about the look that moves the text: all of it but the paper.
    private struct Measure: Equatable {
        let look: Look
        let width: CGFloat
        let scale: CGFloat
    }

    private struct Drawn: Equatable {
        let tone: Tone
        let palette: Palette
    }

    /// What the text is drawn from: a new look, Light and Dark changing, or on iPhone the reader's
    /// width, redraws it at the place.
    private struct Setting: Equatable {
        let id: String
        let look: Look
        let dark: Bool
        let width: CGFloat
        let scale: CGFloat
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
/// chapter and the share read in a book. Reads only `model.placeText` so scrolling doesn't redraw
/// the navigation bar until the visible label changes.
private struct Place: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let model: ReaderModel
    let palette: Palette

    var body: some View {
        let text = model.placeText
        Text(text)
            .font(.inter(13, weight: .medium, relativeTo: .footnote))
            .monospacedDigit()
            .foregroundStyle(Color(palette.muted))
            .contentTransition(reduceMotion ? .identity : .numericText())
            .animation(reduceMotion ? nil : .snappy(duration: 0.25), value: text)
            .accessibilityIdentifier("place")
    }
}

#if os(macOS)
/// How far through, under the title. Its own view, so only it redraws as the reader scrolls.
private struct Subtitle: ViewModifier {
    let model: ReaderModel?

    func body(content: Content) -> some View {
        content.navigationSubtitle(model?.placeText ?? "")
    }
}
#endif

#if os(iOS)
extension Animation {
    /// The reader's bars going and coming as it reads on and back.
    static let bars = Animation.snappy(duration: 0.24, extraBounce: 0)
}

/// Keeps `model.chromeHidden` out of `ReaderView.body` so hiding or showing the bars never
/// invalidates the text view around it.
private struct ChromeVisibility: ViewModifier {
    let model: ReaderModel?

    func body(content: Content) -> some View {
        content.toolbarVisibility(model?.chromeHidden == true ? .hidden : .visible, for: .navigationBar, .tabBar)
    }
}

/// The text dims into the paper at the edges, so a line never reads through the bars' glass. At
/// the top the text stops under the progress hairline, where `ColumnTextView` covers it, and the
/// fade takes the line passing under it over the inset the column keeps above its first line, so
/// nothing at rest is dimmed. The fade at the bottom goes with the bars.
private struct EdgeFades: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let ground: Color
    let model: ReaderModel?

    var body: some View {
        let bottomShown = model?.chromeHidden != true
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
            .animation(reduceMotion ? .easeOut(duration: 0.15) : .bars, value: bottomShown)
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

/// One change of paper: the page as it was, washing away over the text redrawn in the new one.
private struct Spread: Identifiable {
    let id = UUID()
    let image: PlatformImage
    /// The phases and drifts of the front's waves, new each time so no two spreads look alike.
    let seed = (0..<14).map { _ in Double.random(in: 0...(2 * .pi)) }
}

/// The page in its old paper over the new one, washed away from where the paper was chosen: from
/// the top of Appearance's panel on iPhone, and from the toolbar on the Mac. Its front is soft
/// and uneven, like shallow water spreading over a surface, and it takes a second, so the eye can
/// follow it. A finger on the text hurries it away, since the page under it is about to move.
/// Reduce Motion crossfades instead.
private struct SpreadOverlay: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let spread: Spread
    let model: ReaderModel?
    let done: () -> Void
    @State private var progress: CGFloat = 0
    @State private var shown: CGFloat = 1

    var body: some View {
        GeometryReader { space in
            image
                .resizable()
                .frame(width: space.size.width, height: space.size.height)
                .mask {
                    Rectangle()
                        .overlay {
                            Front(progress: reduceMotion ? 0 : progress, origin: Self.origin(in: space.size), seed: spread.seed)
                                .fill(.black)
                                .blendMode(.destinationOut)
                        }
                        .compositingGroup()
                        .blur(radius: 26)
                }
                .opacity(shown)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .onAppear { if model?.ready == true { wash() } }
        .onChange(of: model?.ready) { if model?.ready == true { wash() } }
        .onChange(of: model?.touches) {
            withAnimation(.easeOut(duration: 0.15)) { shown = 0 } completion: { done() }
        }
    }

    /// Starts once the text under it stands at its place, so it never uncovers a page still being
    /// laid out.
    private func wash() {
        if reduceMotion {
            withAnimation(.easeOut(duration: 0.3)) { shown = 0 } completion: { done() }
        } else {
            withAnimation(.timingCurve(0.25, 0.1, 0.25, 1, duration: 1.1)) { progress = 1 } completion: { done() }
        }
    }

    private var image: Image {
        #if os(iOS)
        Image(uiImage: spread.image)
        #else
        Image(nsImage: spread.image)
        #endif
    }

    private static func origin(in size: CGSize) -> CGPoint {
        #if os(iOS)
        CGPoint(x: size.width / 2, y: size.height - 330)
        #else
        CGPoint(x: size.width - 40, y: -40)
        #endif
    }
}

/// How far the new paper has spread: a circle from `origin` whose edge seven slow waves push out
/// and hold back, so it runs ahead in tongues and lags in bays, and the waves drift as it goes. At
/// `progress` 1 it has passed every corner, its blur included.
private nonisolated struct Front: Shape {
    var progress: CGFloat
    let origin: CGPoint
    let seed: [Double]

    var animatableData: CGFloat {
        get { progress }
        set { progress = newValue }
    }

    func path(in rect: CGRect) -> Path {
        let corners = [CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY), CGPoint(x: rect.minX, y: rect.maxY), CGPoint(x: rect.maxX, y: rect.maxY)]
        let reach = corners.map { hypot($0.x - origin.x, $0.y - origin.y) }.max() ?? 0
        let radius = progress * (reach * 1.22 + 60)
        let points = (0..<96).map { step -> CGPoint in
            let angle = Double(step) / 96 * 2 * .pi
            var swell = 0.0
            for wave in 0..<7 {
                let k = Double(wave + 2)
                swell += sin(k * angle + seed[wave] + Double(progress) * (1.4 + seed[wave + 7])) * 0.085 / k.squareRoot()
            }
            let r = radius * (1 + swell)
            return CGPoint(x: origin.x + r * cos(angle), y: origin.y + r * sin(angle))
        }
        var path = Path()
        guard radius > 0 else { return path }
        path.move(to: points[0])
        for i in points.indices {
            let p0 = points[(i + points.count - 1) % points.count], p1 = points[i]
            let p2 = points[(i + 1) % points.count], p3 = points[(i + 2) % points.count]
            path.addCurve(to: p2,
                          control1: CGPoint(x: p1.x + (p2.x - p0.x) / 6, y: p1.y + (p2.y - p0.y) / 6),
                          control2: CGPoint(x: p2.x - (p3.x - p1.x) / 6, y: p2.y - (p3.y - p1.y) / 6))
        }
        path.closeSubpath()
        return path
    }
}

/// How far through, as a hairline across the top of the reader. It moves with the text, frame by
/// frame, using a horizontal scale so scrolling triggers no layout pass.
private struct ProgressLine: View {
    let model: ReaderModel
    let palette: Palette

    var body: some View {
        Rectangle()
            .fill(Color(palette.hairline))
            .frame(height: 2)
            .overlay {
                Rectangle()
                    .fill(Color(palette.ink).opacity(0.55))
                    .scaleEffect(x: max(0, min(1, model.progress)), y: 1, anchor: .leading)
            }
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

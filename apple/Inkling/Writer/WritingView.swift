import InklingCore
import SwiftUI

/// The native writing page: TextKit 2 live Markdown on Inkling's paper, and inline review over the
/// same column when the writer asks the Ask bar for changes.
struct WritingView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.colorScheme) private var scheme
    @Environment(\.scenePhase) private var phase

    let documentId: String
    @State private var model: WritingModel?
    @State private var showingAppearance = false
    #if os(iOS)
    @State private var editorFocused = false
    @State private var viewWidth: CGFloat = 390
    #endif

    init(documentId: String, initialModel: WritingModel? = nil) {
        self.documentId = documentId
        _model = State(initialValue: initialModel)
    }

    var body: some View {
        let look = shelf.writingLook
        let palette = Self.palette(look.tone, shelf: shelf, scheme: scheme)
        #if os(iOS)
        let column = min(CGFloat(look.pageWidth), max(280, viewWidth - 44))
        let style = WritingStyle.make(look: look, palette: palette, column: column, isPhone: true)
        #else
        let column = CGFloat(look.pageWidth)
        let style = WritingStyle.make(look: look, palette: palette, column: column, isPhone: false)
        #endif

        ZStack {
            Color(palette.ground).ignoresSafeArea()

            if let model {
                VStack(spacing: 0) {
                    if let snapshot = model.snapshot, model.isAsking || model.isReviewing {
                        VersionBar(model: model, snapshot: snapshot, palette: palette, column: column)
                            .transition(.move(edge: .top).combined(with: .opacity))
                    }

                    ZStack(alignment: .bottom) {
                        if case .reviewing(_, let segments) = model.snapshot?.state {
                            InlineReviewView(
                                model: model,
                                segments: segments,
                                nextPending: model.nextPendingId,
                                style: style
                            )
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                        } else {
                            #if os(macOS)
                            WritingTextView(model: model, style: style, editable: !model.isAsking)
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                            #else
                            WritingTextView(model: model, style: style, editable: !model.isAsking, isFocused: $editorFocused)
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                            #endif
                        }

                        LinearGradient(
                            colors: [Color(palette.ground).opacity(0), Color(palette.ground)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 22)
                        .allowsHitTesting(false)
                    }

                    #if os(iOS)
                    if case .reviewing(_, let segments) = model.snapshot?.state {
                        PhoneReviewSheet(model: model, segments: segments, palette: palette)
                            .transition(.move(edge: .bottom).combined(with: .opacity))
                    }
                    #endif

                    if !model.isReviewing, let notice = model.notice {
                        Notice(words: notice, missingKey: model.missingKeyProvider != nil, palette: palette, column: column) {
                            shelf.asker.choosingProvider = true
                        }
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                    }
                }
            }
        }
        .animation(.easeOut(duration: 0.2), value: model?.isReviewing)
        .animation(.easeOut(duration: 0.2), value: model?.isAsking)
        .environment(\.palette, palette)
        #if os(iOS)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { viewWidth = $0 }
        .navigationBarTitleDisplayMode(.inline)
        .navigationTitle("")
        .onChange(of: look.tone, initial: true) { navigator.readerScheme = Self.scheme(of: look.tone, palette: palette) }
        .onDisappear { navigator.readerScheme = nil }
        #else
        .navigationTitle(model?.title ?? "Writing")
        .navigationSubtitle(subtitle)
        #endif
        .toolbar {
            #if os(iOS)
            ToolbarItem(placement: .principal) {
                if let model {
                    Text(statusLine(for: model))
                        .font(.inter(13, weight: .medium, relativeTo: .footnote))
                        .foregroundStyle(Color(palette.muted))
                        .monospacedDigit()
                        .accessibilityIdentifier("writing-status")
                }
            }
            ToolbarItemGroup(placement: .primaryAction) {
                if editorFocused {
                    Button("Done") { editorFocused = false }
                        .fontWeight(.semibold)
                        .accessibilityIdentifier("done-typing")
                } else {
                    Button("Appearance", systemImage: "textformat.size") { showingAppearance = true }
                        .accessibilityIdentifier("writing-appearance")
                }
            }
            #else
            ToolbarItem(placement: .primaryAction) {
                Button("Appearance", systemImage: "textformat.size") { showingAppearance = true }
                    .popover(isPresented: $showingAppearance, arrowEdge: .bottom) {
                        AppearancePanel(target: .writing)
                    }
                    .accessibilityIdentifier("writing-appearance")
            }
            #endif
        }
        #if os(iOS)
        .sheet(isPresented: $showingAppearance) {
            AppearancePanel(target: .writing)
                .presentationDetents([.height(330)])
        }
        #endif
        .task(id: documentId) {
            if let existing = model, existing.id == documentId {
                shelf.activeWriter = existing
                shelf.asker.place = .writing(existing)
                return
            }
            model?.saveNow()
            let loaded = shelf.readDocument(documentId)
            shelf.setLastOpenDocument(documentId)
            let created = WritingModel(id: documentId, text: loaded, shelf: shelf)
            model = created
            shelf.activeWriter = created
            shelf.asker.place = .writing(created)
        }
        .onAppear {
            if let model { shelf.asker.place = .writing(model) }
        }
        .onChange(of: phase) {
            if phase != .active { model?.saveNow() }
        }
        .onDisappear {
            model?.saveNow()
            if shelf.activeWriter === model {
                shelf.activeWriter = nil
            }
        }
    }

    private var subtitle: String {
        guard let model else { return "" }
        return statusLine(for: model)
    }

    private func statusLine(for model: WritingModel) -> String {
        let words = model.wordCount == 1 ? "1 word" : "\(model.wordCount) words"
        return "\(model.saved.label) · \(words)"
    }

    static func palette(_ tone: Tone, shelf: Shelf, scheme: ColorScheme) -> Palette {
        switch tone {
        case .light, .dark: Palette(shelf.looks.colors(tone: scheme == .dark ? .dark : .light))
        default: Palette(shelf.looks.colors(tone: tone))
        }
    }

    static func scheme(of tone: Tone, palette: Palette) -> ColorScheme? {
        switch tone {
        case .light, .dark: nil
        default: palette.isDark ? .dark : .light
        }
    }
}

/// The top bar shown while asking a provider or reviewing a version.
struct VersionBar: View {
    let model: WritingModel
    let snapshot: RevisionSnapshot
    let palette: Palette
    let column: CGFloat

    var body: some View {
        HStack(spacing: 10) {
            switch snapshot.state {
            case .asking(let words):
                Text("Version \(model.versionNumber)")
                    .font(.inter(13, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Color(palette.ink))
                ProgressView()
                    .controlSize(.small)
                Text(progressText(words: words))
                    .font(.inter(13, relativeTo: .callout))
                    .foregroundStyle(Color(palette.muted))
                    .lineLimit(1)
                Spacer(minLength: 4)
                Button("Cancel") { model.cancelReview() }
                    .font(.inter(13, weight: .medium, relativeTo: .callout))
                    .foregroundStyle(Color(palette.muted))
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("cancel-review")

            case .reviewing(let stats, _):
                VStack(alignment: .leading, spacing: 2) {
                    Text("Version \(model.versionNumber)")
                        .font(.inter(13, weight: .semibold, relativeTo: .callout))
                        .foregroundStyle(Color(palette.ink))
                    HStack(spacing: 4) {
                        Text("\(snapshot.providerLabel) ·")
                            .foregroundStyle(Color(palette.muted))
                        Text("+\(stats.wordsAdded)")
                            .foregroundStyle(Color(palette.addedCount))
                        Text("−\(stats.wordsRemoved)")
                            .foregroundStyle(Color(palette.removedLine))
                        Text("· \(stats.accepted) of \(stats.total) accepted")
                            .foregroundStyle(Color(palette.muted))
                    }
                    .font(.inter(11.5, weight: .medium, relativeTo: .caption))
                    .monospacedDigit()
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                }
                Spacer(minLength: 4)
                #if os(macOS)
                Button("Back to draft") { model.finishReview() }
                    .font(.inter(13, weight: .medium, relativeTo: .callout))
                    .foregroundStyle(Color(palette.muted))
                    .buttonStyle(.plain)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .accessibilityIdentifier("back-to-draft")
                #endif
                Button("Accept the rest") { model.acceptRest() }
                    .font(.inter(12.5, weight: .semibold, relativeTo: .callout))
                    .foregroundStyle(Color(palette.surface))
                    .padding(.horizontal, 11)
                    .padding(.vertical, 7)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color(palette.ink)))
                    .buttonStyle(.plain)
                    .fixedSize()
                    .accessibilityIdentifier("accept-rest")

            default:
                EmptyView()
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color(palette.surface))
                .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.hairline), lineWidth: 1) }
        )
        .frame(maxWidth: column)
        .padding(.horizontal, 20)
        .padding(.top, 12)
        .padding(.bottom, 6)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("version-bar")
    }

    private func progressText(words: UInt32) -> String {
        switch words {
        case 0: "asking \(snapshot.providerLabel)…"
        case 1: "asking \(snapshot.providerLabel)… 1 word so far"
        default: "asking \(snapshot.providerLabel)… \(words) words so far"
        }
    }
}

/// What became of the last ask, when it brought no review: no changes, a failure, or a missing key.
private struct Notice: View {
    let words: String
    let missingKey: Bool
    let palette: Palette
    let column: CGFloat
    let addKey: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(words)
                .font(.inter(13, weight: .medium, relativeTo: .footnote))
                .foregroundStyle(Color(palette.muted))
                .lineLimit(2)
            if missingKey {
                Button("Add key…", action: addKey)
                    .font(.inter(13, weight: .semibold, relativeTo: .footnote))
                    .foregroundStyle(Color(palette.link))
                    .buttonStyle(.plain)
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: column)
        .padding(.horizontal, 24)
        .padding(.vertical, 10)
        .accessibilityIdentifier("writing-notice")
    }
}

/// Provider choice, Keychain API key entry and optional model override.
struct ProviderPanel: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @State private var selected: AiProvider = .claude
    @State private var apiKey = ""
    @State private var modelName = ""
    @State private var hasKey = false
    @State private var statusMessage: String?

    var body: some View {
        Form {
            SettingsCard {
                Picker("Provider", selection: $selected) {
                    Text("Claude").tag(AiProvider.claude)
                    Text("Gemini").tag(AiProvider.gemini)
                    Text("ChatGPT").tag(AiProvider.chatGpt)
                }
                .pickerStyle(.segmented)
            }

            SwiftUI.Section {
                SettingsCard {
                    HStack {
                        SecureField("", text: $apiKey, prompt: palette.prompt(hasKey ? "Saved in Keychain" : "Paste your \(providerLabel(provider: selected)) key"))
                            .accessibilityIdentifier("provider-key-field")
                        if !apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Button("Save") { saveKey() }
                                .buttonStyle(.borderedProminent)
                        }
                    }
                    if hasKey {
                        HStack {
                            Label("Key saved", systemImage: "checkmark.circle.fill")
                                .font(.inter(12, weight: .medium, relativeTo: .caption))
                                .foregroundStyle(Color(palette.muted))
                            Spacer()
                            Button("Remove") { removeKey() }
                                .font(.inter(12, relativeTo: .caption))
                        }
                    }
                }
            } header: {
                palette.caption("API Key")
            }

            SwiftUI.Section {
                SettingsCard {
                    TextField("", text: $modelName, prompt: palette.prompt(providerDefaultModel(provider: selected)))
                        .onSubmit { saveModel() }
                        .accessibilityIdentifier("provider-model-field")
                }
            } header: {
                palette.caption("Model")
            }

            if let statusMessage {
                Text(statusMessage)
                    .font(.inter(12, relativeTo: .caption))
                    .foregroundStyle(Color(palette.muted))
            }
        }
        .settingsForm(palette)
        .onAppear { load() }
        .onChange(of: selected) {
            try? shelf.ai.setProvider(provider: selected)
            loadForProvider(selected)
        }
        .onChange(of: modelName) {
            saveModel()
        }
    }

    private func load() {
        selected = shelf.ai.provider()
        loadForProvider(selected)
    }

    private func loadForProvider(_ provider: AiProvider) {
        hasKey = shelf.ai.hasKey(provider: provider)
        modelName = shelf.ai.typedModel(provider: provider) ?? ""
        apiKey = ""
        statusMessage = nil
    }

    private func saveKey() {
        let trimmed = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            try shelf.ai.setKey(provider: selected, key: trimmed)
            apiKey = ""
            hasKey = shelf.ai.hasKey(provider: selected)
            statusMessage = "Saved \(providerLabel(provider: selected)) key."
        } catch {
            statusMessage = Shelf.words(error)
        }
    }

    private func removeKey() {
        do {
            try shelf.ai.removeKey(provider: selected)
            hasKey = shelf.ai.hasKey(provider: selected)
            statusMessage = "Removed \(providerLabel(provider: selected)) key."
        } catch {
            statusMessage = Shelf.words(error)
        }
    }

    private func saveModel() {
        try? shelf.ai.setModel(provider: selected, model: modelName)
    }
}

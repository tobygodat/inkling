import SwiftUI

/// The whole window: a three-column split on the Mac, a tab bar on iPhone.
struct RootView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.colorScheme) private var scheme
    @Environment(\.scenePhase) private var phase

    var body: some View {
        @Bindable var navigator = navigator
        let chrome = Chrome(shelf.looks)
        Group {
            #if os(macOS)
            MacRoot()
            #else
            PhoneRoot()
            #endif
        }
        .environment(\.palette, chrome.palette(scheme))
        .tint(Color(chrome.palette(scheme).ink))
        .sheet(isPresented: $navigator.savingLink) {
            SaveLinkSheet()
                .environment(\.palette, chrome.palette(scheme))
                .tint(Color(chrome.palette(scheme).ink))
        }
        .fileImporter(isPresented: $navigator.addingBook, allowedContentTypes: [.epub, .pdf], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            Task { for url in urls { await shelf.addBook(at: url) } }
        }
        .task {
            shelf.account.startBackgroundSync()
        }
        .onChange(of: phase) {
            if phase == .active {
                shelf.account.syncNow()
            } else {
                shelf.flushPlaces()
            }
        }
        .alert("Something went wrong", isPresented: Binding(get: { shelf.failure != nil }, set: { if !$0 { shelf.failure = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(shelf.failure ?? "")
        }
    }
}

#if os(macOS)
struct MacRoot: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        @Bindable var navigator = navigator
        NavigationSplitView {
            List(Section.sidebar, selection: Binding($navigator.section)) { section in
                Label(section.title, systemImage: section.symbol)
                    .badge(shelf.count(section) ?? 0)
                    .tag(section)
            }
            .safeAreaInset(edge: .bottom) {
                if shelf.account.syncStopped {
                    Button {
                        openSettings()
                    } label: {
                        HStack(spacing: 8) {
                            Circle()
                                .fill(Color(palette.ink))
                                .frame(width: 6, height: 6)
                            Text("Sync stopped")
                                .font(.inter(12, weight: .medium, relativeTo: .caption))
                                .foregroundStyle(Color(palette.ink))
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("sync-stopped-banner")
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 260)
        } content: {
            SectionView(section: navigator.section)
                .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 520)
        } detail: {
            Group {
                if navigator.section == .writing {
                    if let docId = navigator.document {
                        WritingView(documentId: docId)
                            .id(docId)
                    } else {
                        NothingOpen(words: "Choose a document or start a new one", section: .writing)
                    }
                } else if let item = shelf.item(navigator.selection) {
                    ReaderView(item: item)
                        .id(item.id)
                } else {
                    NothingOpen(words: "Choose something to read", section: navigator.section)
                }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) { MacAskDock() }
        }
        .onChange(of: navigator.section) {
            let home: Section? = shelf.item(navigator.selection).map { $0.kind == .article ? .inbox : .library }
            if home != navigator.section { navigator.selection = nil }
            if navigator.section == .writing && navigator.document == nil {
                navigator.document = shelf.lastOpenDocumentId() ?? shelf.docs.first?.id
            }
        }
    }
}

private extension Binding where Value == Section? {
    init(_ section: Binding<Section>) {
        self.init(get: { section.wrappedValue }, set: { if let new = $0 { section.wrappedValue = new } })
    }
}

/// The reader's column before anything is chosen.
struct NothingOpen: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    let words: String
    let section: Section

    var body: some View {
        Text(words)
            .font(.serif(19, relativeTo: .title3))
            .foregroundStyle(Color(palette.muted))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(palette.ground))
            .onAppear { shelf.asker.place = .screen(section) }
            .onChange(of: section) { shelf.asker.place = .screen(section) }
    }
}
#else
/// Four tabs, and the Ask bar above them as the tab bar's bottom accessory, which folds away with
/// the tab bar as a page scrolls on and hides with it in the immersive reader.
struct PhoneRoot: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    @State private var writingPath: [DocumentRoute] = []

    var body: some View {
        @Bindable var navigator = navigator
        @Bindable var asker = shelf.asker
        TabView(selection: $navigator.section) {
            ForEach(Section.allCases) { section in
                Tab(section.title, systemImage: section.symbol, value: section) {
                    switch section {
                    case .writing:
                        NavigationStack(path: $writingPath) {
                            SectionView(section: .writing)
                                .navigationDestination(for: DocumentRoute.self) { route in
                                    WritingView(documentId: route.id)
                                }
                        }
                        .onChange(of: navigator.phoneDocument) {
                            if let route = navigator.phoneDocument {
                                navigator.phoneDocument = nil
                                writingPath.append(route)
                            }
                        }
                    case .inbox:
                        NavigationStack(path: $navigator.inboxPath) { reading(.inbox) }
                    case .library:
                        NavigationStack(path: $navigator.libraryPath) { reading(.library) }
                    case .profile:
                        NavigationStack { SectionView(section: .profile) }
                    }
                }
                .accessibilityIdentifier("tab-\(section.rawValue)")
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
        .modifier(AskAccessory(hidden: navigator.immersed))
        .sheet(item: Binding(get: { asker.composing ? AskSheet.composer : asker.adding ? .add : nil }, set: { if $0 == nil { asker.composing = false; asker.adding = false } })) { sheet in
            switch sheet {
            case .composer:
                AskPanel()
                    .presentationDetents([.medium, .large])
                    .presentationBackground(Color(palette.ground))
                    .sheet(isPresented: $asker.choosingProvider) { providerSheet }
                    .sheet(isPresented: $asker.adding) { addSheet }
            case .add:
                addSheet
            }
        }
    }

    private enum AskSheet: Identifiable {
        case composer, add
        var id: Self { self }
    }

    private func reading(_ section: Section) -> some View {
        SectionView(section: section)
            .navigationDestination(for: String.self) { id in
                PhoneReader(id: id)
            }
    }

    private var addSheet: some View {
        AskAddSheet()
            .presentationDetents([.height(shelf.asker.suggestions.isEmpty ? 190 : 290)])
            .presentationBackground(Color(palette.ground))
    }

    private var providerSheet: some View {
        NavigationStack {
            ProviderPanel()
                .navigationTitle("AI Provider")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { shelf.asker.choosingProvider = false }
                    }
                }
        }
        .presentationDetents([.medium])
    }
}

/// The Ask bar as the tab bar's bottom accessory, put away while the reader is immersed where the
/// system can (iOS 26.1).
private struct AskAccessory: ViewModifier {
    let hidden: Bool

    func body(content: Content) -> some View {
        if #available(iOS 26.1, *) {
            content.tabViewBottomAccessory(isEnabled: !hidden) { AskBar() }
        } else {
            content.tabViewBottomAccessory { AskBar() }
        }
    }
}

private struct PhoneReader: View {
    @Environment(Shelf.self) private var shelf
    let id: String

    var body: some View {
        if let item = shelf.item(id) {
            ReaderView(item: item)
        }
    }
}
#endif

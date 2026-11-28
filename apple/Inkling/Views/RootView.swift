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
        #if os(iOS)
        .sheet(isPresented: $navigator.showingSettings) {
            PhoneSettingsSheet()
                .environment(\.palette, chrome.palette(scheme))
                .tint(Color(chrome.palette(scheme).ink))
        }
        #endif
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

/// Save Link and Add Book, the one menu that adds to the library.
struct AddMenu: View {
    @Environment(Navigator.self) private var navigator

    var body: some View {
        Menu {
            Button("Save Link…", systemImage: "link") { navigator.savingLink = true }
            Button("Add Book…", systemImage: "book") { navigator.addingBook = true }
        } label: {
            Label("Add", systemImage: "plus")
        }
        .accessibilityIdentifier("add")
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
            List(Section.allCases, selection: Binding($navigator.section)) { section in
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
            if navigator.section == .writing {
                if let docId = navigator.document {
                    WritingView(documentId: docId)
                        .id(docId)
                } else {
                    NothingOpen(words: "Choose a document or start a new one")
                }
            } else if let item = shelf.item(navigator.selection) {
                ReaderView(item: item)
                    .id(item.id)
            } else {
                NothingOpen(words: shelf.count(navigator.section) == nil ? nil : "Choose something to read")
            }
        }
        .onChange(of: navigator.section) {
            navigator.selection = nil
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
    @Environment(\.palette) private var palette
    let words: String?

    var body: some View {
        Text(words ?? "")
            .font(.serif(19, relativeTo: .title3))
            .foregroundStyle(Color(palette.muted))
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color(palette.ground))
    }
}
#else
/// The essentials in the tab bar, and Search. Archive and Notebook sit inside Queue and Library so
/// the tab bar keeps four tabs beside the detached Search pill with no More tab.
struct PhoneRoot: View {
    private enum Place: Hashable { case section(Section), search }
    private static let tabs: [Section] = [.queue, .inbox, .library, .writing]
    @Environment(Navigator.self) private var navigator
    @State private var place = Place.section(.queue)
    @State private var writingPath: [DocumentRoute] = []

    var body: some View {
        TabView(selection: $place) {
            ForEach(Self.tabs) { section in
                Tab(section.title, systemImage: section.symbol, value: Place.section(section)) {
                    if section == .writing {
                        NavigationStack(path: $writingPath) {
                            SectionView(section: .writing)
                                .navigationDestination(for: DocumentRoute.self) { route in
                                    WritingView(documentId: route.id)
                                        .toolbar(.hidden, for: .tabBar)
                                }
                        }
                        .onChange(of: navigator.phoneDocument) {
                            if let route = navigator.phoneDocument {
                                navigator.phoneDocument = nil
                                writingPath.append(route)
                            }
                        }
                    } else {
                        NavigationStack {
                            SectionView(section: section)
                                .navigationDestination(for: String.self) { id in
                                    PhoneReader(id: id)
                                }
                        }
                    }
                }
            }
            Tab(value: Place.search, role: .search) {
                NavigationStack {
                    SearchView()
                        .navigationDestination(for: String.self) { id in
                            PhoneReader(id: id)
                        }
                }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
    }
}

/// Every item, archived ones too, found by its title, author, site or summary.
private struct SearchView: View {
    @Environment(Shelf.self) private var shelf
    @State private var query = ""

    var body: some View {
        let words = query.trimmingCharacters(in: .whitespaces)
        let found = words.isEmpty ? shelf.items : shelf.items.filter { item in
            [item.title, item.author, item.site, item.excerpt].compactMap(\.self).contains { $0.localizedStandardContains(words) }
        }
        ItemList(groups: [("", found)], section: .library) {
            ContentUnavailableView.search(text: words)
        }
        .navigationTitle("Search")
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Titles, authors and sites")
    }
}

private struct PhoneReader: View {
    @Environment(Shelf.self) private var shelf
    let id: String

    var body: some View {
        if let item = shelf.item(id) {
            ReaderView(item: item)
                .toolbar(.hidden, for: .tabBar)
        }
    }
}
#endif

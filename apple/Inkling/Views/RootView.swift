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
        .onChange(of: phase) { if phase != .active { shelf.flushPlaces() } }
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

    var body: some View {
        @Bindable var navigator = navigator
        NavigationSplitView {
            List(Section.allCases, selection: Binding($navigator.section)) { section in
                Label(section.title, systemImage: section.symbol)
                    .badge(shelf.count(section) ?? 0)
                    .tag(section)
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 210, max: 260)
        } content: {
            SectionView(section: navigator.section)
                .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 520)
        } detail: {
            if let item = shelf.item(navigator.selection) {
                ReaderView(item: item)
                    .id(item.id)
            } else {
                NothingOpen(words: shelf.count(navigator.section) == nil ? nil : "Choose something to read")
            }
        }
        .onChange(of: navigator.section) { navigator.selection = nil }
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
struct PhoneRoot: View {
    @Environment(Navigator.self) private var navigator

    var body: some View {
        @Bindable var navigator = navigator
        TabView(selection: $navigator.section) {
            ForEach(Section.allCases) { section in
                Tab(section.title, systemImage: section.symbol, value: section) {
                    NavigationStack {
                        SectionView(section: section)
                            .navigationDestination(for: String.self) { id in
                                PhoneReader(id: id)
                            }
                    }
                }
            }
        }
        .tabBarMinimizeBehavior(.onScrollDown)
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

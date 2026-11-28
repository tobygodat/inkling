import InklingCore
import SwiftUI

/// One section's screen: the Inbox and the Library with search at the top, Writing's documents,
/// or Profile.
struct SectionView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #endif
    let section: Section
    @State private var query = ""

    var body: some View {
        content
            .navigationTitle(section.title)
            .toolbar {
                if section == .inbox || section == .library {
                    ToolbarItem(placement: .primaryAction) { AddMenu(section: section) }
                }
            }
            #if os(iOS)
            .onAppear { shelf.asker.place = .screen(section) }
            #endif
    }

    @ViewBuilder private var content: some View {
        switch section {
        case .writing:
            DocumentListView()
        case .inbox:
            inbox
        case .library:
            ItemList(groups: [("", Self.found(shelf.books, query))], empty: {
                if query.isEmpty {
                    ContentUnavailableView {
                        Label("Your library is empty", systemImage: "books.vertical")
                    } description: {
                        Text("Add an EPUB or a PDF with text.")
                    } actions: {
                        Button("Add a Book") { navigator.addingBook = true }
                            .buttonStyle(.glassProminent)
                    }
                } else {
                    ContentUnavailableView.search(text: query)
                }
            }, header: {
                #if os(macOS)
                SearchField(query: $query, prompt: "Titles and authors")
                #endif
            })
            #if os(iOS)
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Titles and authors")
            #endif
        case .profile:
            #if os(iOS)
            ProfileView()
            #else
            ProfileSettingsView()
            #endif
        }
    }

    private var inbox: some View {
        @Bindable var navigator = navigator
        let items = Self.found(shelf.inbox(navigator.filter), query)
        return ItemList(groups: [("", items)]) {
            if !query.isEmpty {
                ContentUnavailableView.search(text: query)
            } else if navigator.filter == .saved || (navigator.filter == .all && !shelf.account.signedIn) {
                ContentUnavailableView {
                    Label("Nothing to read yet", systemImage: "tray")
                } description: {
                    Text("Save a link and it waits here, newest first, until you’re done with it.")
                } actions: {
                    Button("Save a Link") { navigator.savingLink = true }
                        .buttonStyle(.glassProminent)
                }
            } else {
                ContentUnavailableView {
                    Label(shelf.account.signedIn ? "All caught up" : "Newsletters and feeds", systemImage: "tray")
                } description: {
                    Text(shelf.account.signedIn
                         ? "New posts from your feeds and newsletters arrive here on sync."
                         : "Sign in with Google in Profile to follow feeds and receive newsletters here.")
                } actions: {
                    if !shelf.account.signedIn {
                        Button("Open Profile") {
                            #if os(macOS)
                            openSettings()
                            #else
                            navigator.section = .profile
                            #endif
                        }
                        .buttonStyle(.glassProminent)
                        .accessibilityIdentifier("inbox-settings")
                    }
                }
            }
        } header: {
            VStack(spacing: 10) {
                #if os(macOS)
                SearchField(query: $query, prompt: "Titles, authors and sites")
                #endif
                FilterPicker(filter: $navigator.filter)
            }
        }
        #if os(iOS)
        .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Titles, authors and sites")
        #endif
    }

    /// The items whose title, author, site or summary hold `query`.
    static func found(_ items: [Item], _ query: String) -> [Item] {
        let words = query.trimmingCharacters(in: .whitespaces)
        guard !words.isEmpty else { return items }
        return items.filter { item in
            [item.title, item.author, item.site, item.excerpt].compactMap(\.self).contains { $0.localizedStandardContains(words) }
        }
    }
}

#if os(macOS)
/// Search at the top of the Mac's list column, in Inkling's own field.
private struct SearchField: View {
    @Environment(\.palette) private var palette
    @Binding var query: String
    let prompt: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Color(palette.muted))
            TextField(prompt, text: $query)
                .textFieldStyle(.plain)
                .font(.inter(13, relativeTo: .body))
            if !query.isEmpty {
                Button("Clear", systemImage: "xmark.circle.fill") { query = "" }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.plain)
                    .foregroundStyle(Color(palette.muted))
            }
        }
        .padding(.horizontal, 10)
        .frame(maxWidth: .infinity, minHeight: 30, maxHeight: 30)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color(palette.segment)))
        .accessibilityIdentifier("search")
    }
}
#endif

/// All · Saved · Newsletters · Feeds.
struct FilterPicker: View {
    @Binding var filter: InboxFilter

    var body: some View {
        Picker("Show", selection: $filter.animation(.snappy(duration: 0.25))) {
            ForEach(InboxFilter.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented)
        #if os(macOS)
        .controlSize(.small)
        #endif
        .labelsHidden()
        .accessibilityIdentifier("inbox-filter")
    }
}

/// Save Link in the Inbox, Add Book in the Library.
struct AddMenu: View {
    @Environment(Navigator.self) private var navigator
    let section: Section

    var body: some View {
        Button(section == .inbox ? "Save Link" : "Add Book", systemImage: "plus") {
            if section == .inbox { navigator.savingLink = true } else { navigator.addingBook = true }
        }
        .accessibilityIdentifier("add")
    }
}

/// What has been archived, newest first. Bringing one back puts it in the Inbox or the Library.
struct ArchiveView: View {
    @Environment(Shelf.self) private var shelf
    @State private var query = ""

    var body: some View {
        ItemList(groups: [("", SectionView.found(shelf.archive, query))]) {
            ContentUnavailableView {
                Label("Nothing archived", systemImage: "archivebox")
            } description: {
                Text("Swipe a row, or choose Done at the end of an article, and it moves here.")
            }
        }
        .navigationTitle("Archive")
        .searchable(text: $query, prompt: "Titles, authors and sites")
    }
}

/// Every highlight, grouped by what it was made in; choosing one opens it there.
struct HighlightsView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette

    var body: some View {
        let groups = shelf.allHighlights()
        Group {
            if groups.isEmpty {
                ContentUnavailableView {
                    Label("No highlights yet", systemImage: "highlighter")
                } description: {
                    Text("Select a passage while reading and choose Highlight.")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(groups, id: \.item.id) { group in
                        SwiftUI.Section {
                            ForEach(group.passages, id: \.self) { passage in
                                Button {
                                    navigator.open(group.item, at: passage)
                                } label: {
                                    Text(passage.words)
                                        .font(.serif(16, relativeTo: .body))
                                        .foregroundStyle(Color(palette.ink))
                                        .lineSpacing(3)
                                        .lineLimit(5)
                                        .padding(.vertical, 6)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .listRowBackground(Color(palette.ground))
                                .listRowSeparatorTint(Color(palette.hairline))
                            }
                        } header: {
                            Text(group.item.title)
                                .font(.inter(13, weight: .semibold, relativeTo: .subheadline))
                                .foregroundStyle(Color(palette.muted))
                        }
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
            }
        }
        .background(Color(palette.ground))
        .navigationTitle("Highlights")
    }
}

import InklingCore
import SwiftUI

/// One section's screen: its items, or what is coming for the sections not built yet.
struct SectionView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    #else
    @Environment(Navigator.self) private var navigator
    #endif
    let section: Section

    var body: some View {
        content
            .navigationTitle(section.title)
            .toolbar {
                #if os(iOS)
                if section == .queue || section == .library {
                    ToolbarItemGroup(placement: .topBarLeading) {
                        NavigationLink {
                            SectionView(section: .archive)
                        } label: {
                            Label("Archive", systemImage: "archivebox")
                        }
                        .accessibilityIdentifier("archive")
                        if section == .library {
                            NavigationLink {
                                SectionView(section: .notebook)
                            } label: {
                                Label("Notebook", systemImage: "highlighter")
                            }
                            .accessibilityIdentifier("notebook")
                        }
                    }
                } else if section == .inbox {
                    ToolbarItem(placement: .topBarLeading) {
                        PhoneSettingsButton()
                    }
                }
                if section == .queue || section == .library {
                    ToolbarItemGroup(placement: .primaryAction) {
                        PhoneSettingsButton()
                        AddMenu()
                    }
                } else if section != .archive && section != .writing {
                    ToolbarItem(placement: .primaryAction) { AddMenu() }
                }
                #else
                if section != .archive && section != .writing {
                    ToolbarItem(placement: .primaryAction) { AddMenu() }
                }
                #endif
            }
    }

    @ViewBuilder private var content: some View {
        switch section {
        case .writing:
            DocumentListView()
        case .queue:
            ItemList(groups: [("", shelf.queue)], section: section) {
                ContentUnavailableView {
                    Label("Nothing to read yet", systemImage: "square.stack")
                } description: {
                    Text("Save a link and it waits here, newest first, until you archive it.")
                } actions: {
                    SaveLinkButton()
                }
            }
        case .library:
            ItemList(groups: [("Books", shelf.books), ("Articles", shelf.queue)], section: section) {
                ContentUnavailableView {
                    Label("Your library is empty", systemImage: "books.vertical")
                } description: {
                    Text("Add an EPUB or a PDF with text, or save a link to an article.")
                } actions: {
                    AddBookButton()
                }
            }
        case .archive:
            ItemList(groups: [("", shelf.archive)], section: section) {
                ContentUnavailableView {
                    Label("Nothing archived", systemImage: "archivebox")
                } description: {
                    Text("What you finish and archive moves here, out of the Queue. You can bring anything back.")
                }
            }
        case .inbox:
            ItemList(groups: [("", shelf.inbox)], section: section) {
                ContentUnavailableView {
                    Label(shelf.account.signedIn ? "Your Inbox is clear" : "Newsletters and feeds, in one inbox", systemImage: "tray")
                } description: {
                    Text(shelf.account.signedIn
                         ? "New posts from followed RSS feeds and newsletters arrive here on sync, ready to read or move to your Queue."
                         : "Sign in with Google in Settings to follow RSS feeds and receive newsletters here.")
                } actions: {
                    Button(shelf.account.signedIn ? "Manage Feeds" : "Sign in…") {
                        #if os(macOS)
                        openSettings()
                        #else
                        navigator.showingSettings = true
                        #endif
                    }
                    .buttonStyle(.glassProminent)
                    .accessibilityIdentifier("inbox-settings")
                }
            }
        case .notebook:
            Upcoming(
                symbol: "highlighter",
                title: "Every highlight, in one place",
                words: "The passages you highlight in articles and books will gather here, with your notes, ready to export. The notebook isn’t built yet."
            )
        }
    }
}

private struct SaveLinkButton: View {
    @Environment(Navigator.self) private var navigator
    var body: some View {
        Button("Save a Link") { navigator.savingLink = true }
            .buttonStyle(.glassProminent)
    }
}

private struct AddBookButton: View {
    @Environment(Navigator.self) private var navigator
    var body: some View {
        Button("Add a Book") { navigator.addingBook = true }
            .buttonStyle(.glassProminent)
    }
}

/// A section that isn't built yet says what it will hold, with nothing pretending to be in it.
struct Upcoming: View {
    @Environment(\.palette) private var palette
    let symbol: String
    let title: String
    let words: String

    var body: some View {
        ContentUnavailableView {
            Label {
                Text(title).font(.serif(22, weight: .semibold, relativeTo: .title2))
            } icon: {
                Image(systemName: symbol)
            }
        } description: {
            Text(words)
                .font(.inter(14, relativeTo: .callout))
                .foregroundStyle(Color(palette.muted))
                .frame(maxWidth: 340)
        }
        .foregroundStyle(Color(palette.ink))
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(palette.ground))
    }
}

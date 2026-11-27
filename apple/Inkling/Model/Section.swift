/// The app's top-level places, in the order the sidebar and the tab bar show them.
enum Section: String, CaseIterable, Identifiable, Hashable {
    case queue, inbox, library, archive, notebook

    var id: Self { self }

    var title: String {
        switch self {
        case .queue: "Queue"
        case .inbox: "Inbox"
        case .library: "Library"
        case .archive: "Archive"
        case .notebook: "Notebook"
        }
    }

    var symbol: String {
        switch self {
        case .queue: "square.stack"
        case .inbox: "tray"
        case .library: "books.vertical"
        case .archive: "archivebox"
        case .notebook: "highlighter"
        }
    }
}

/// The app's top-level places: the tabs on iPhone, and on the Mac the sidebar, which leaves
/// Profile to Settings.
enum Section: String, CaseIterable, Identifiable, Hashable {
    case inbox, library, writing, profile

    static let sidebar: [Section] = [.inbox, .library, .writing]

    var id: Self { self }

    var title: String {
        switch self {
        case .inbox: "Inbox"
        case .library: "Library"
        case .writing: "Writing"
        case .profile: "Profile"
        }
    }

    var symbol: String {
        switch self {
        case .inbox: "tray"
        case .library: "books.vertical"
        case .writing: "pencil.line"
        case .profile: "person.crop.circle"
        }
    }
}

/// What the Inbox shows: everything still to read, or one kind of it.
enum InboxFilter: String, CaseIterable, Identifiable, Hashable {
    case all, saved, newsletters, feeds

    var id: Self { self }

    var title: String {
        switch self {
        case .all: "All"
        case .saved: "Saved"
        case .newsletters: "Newsletters"
        case .feeds: "Feeds"
        }
    }
}

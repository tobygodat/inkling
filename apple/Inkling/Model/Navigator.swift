import InklingCore
import Observation
import SwiftUI

/// Where the reader or writer is in the app: the section, the item or document chosen in it,
/// and what is presented.
@Observable
final class Navigator {
    var section: Section = .inbox
    var filter: InboxFilter = .all
    /// The item open in the reader on the Mac, which the list highlights.
    var selection: String?
    /// The document open in Writing on the Mac, which the documents list highlights.
    var document: String?
    #if os(iOS)
    /// Pushes a newly created document onto the Writing stack on iPhone.
    var phoneDocument: DocumentRoute?
    /// What is open in the Inbox and the Library on iPhone, so a citation can open its passage.
    var inboxPath: [String] = []
    var libraryPath: [String] = []
    #else
    /// The Ask bar folds to a short pill while the page scrolls on, as the iPhone's does with
    /// the tab bar, and opens out again on a scroll back.
    var askFolded = false
    #endif
    /// A passage to show once its item's reader opens: a citation the Ask bar was tapped on.
    var passage: (id: String, passage: Passage)?
    /// The reader has put its bars away, so the Ask bar goes too.
    var immersed = false
    var savingLink = false
    var addingBook = false
    /// Light or Dark as the open reader's or writer's paper is, so the bars and the status bar match it on iPhone.
    var readerScheme: ColorScheme?

    /// Opens `item` in its own section's reader, at `passage` when one is given.
    func open(_ item: Item, at passage: Passage? = nil) {
        self.passage = passage.map { (item.id, $0) }
        let home: Section = item.kind == .article ? .inbox : .library
        section = home
        #if os(iOS)
        if home == .inbox {
            if inboxPath.last != item.id { inboxPath = [item.id] }
        } else if libraryPath.last != item.id {
            libraryPath = [item.id]
        }
        #else
        selection = item.id
        #endif
    }
}

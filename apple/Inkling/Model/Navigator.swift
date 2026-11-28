import Observation
import SwiftUI

/// Where the reader or writer is in the app: the section, the item or document chosen in it,
/// and what is presented.
@Observable
final class Navigator {
    var section: Section = .queue
    /// The item open in the reader on the Mac, which the list highlights.
    var selection: String?
    /// The document open in Writing on the Mac, which the documents list highlights.
    var document: String?
    #if os(iOS)
    /// Pushes a newly created document onto the Writing stack on iPhone.
    var phoneDocument: DocumentRoute?
    #endif
    var savingLink = false
    var addingBook = false
    #if os(iOS)
    var showingSettings = false
    #endif
    /// Light or Dark as the open reader's or writer's paper is, so the bars and the status bar match it on iPhone.
    var readerScheme: ColorScheme?
}

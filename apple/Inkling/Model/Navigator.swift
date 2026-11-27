import Observation
import SwiftUI

/// Where the reader is in the app: the section, the item chosen in it, and what is presented.
@Observable
final class Navigator {
    var section: Section = .queue
    /// The item open in the reader on the Mac, which the list highlights.
    var selection: String?
    var savingLink = false
    var addingBook = false
    /// Light or Dark as the open reader's paper is, so the bars and the status bar match it on iPhone.
    var readerScheme: ColorScheme?
}

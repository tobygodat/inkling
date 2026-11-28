#if os(macOS)
import SwiftUI

extension FocusedValues {
    /// The reader in the key window, for the Highlight command.
    @Entry var reader: ReaderModel?
}

/// The menu bar: adding to the library, the chosen item, the reader and the sections.
struct InklingCommands: Commands {
    let shelf: Shelf
    let navigator: Navigator
    @FocusedValue(\.reader) private var reader

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Document") {
                navigator.section = .writing
                navigator.document = shelf.createDocument()
            }
            .keyboardShortcut("n")
            Divider()
            Button("Save Link…") { navigator.savingLink = true }
                .keyboardShortcut("l")
            Button("Add Book…") { navigator.addingBook = true }
                .keyboardShortcut("o")
            Divider()
            Button("Sync Now") { shelf.account.syncNow() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
                .disabled(!shelf.account.signedIn || shelf.account.syncing)
        }
        CommandMenu("Item") {
            let item = shelf.item(navigator.selection)
            Button("Archive") { if let item { shelf.setArchived(item, true) } }
                .keyboardShortcut("e")
                .disabled(item == nil || item?.archived != 0)
            Button("Bring Back") { if let item { shelf.setArchived(item, false) } }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(item == nil || item?.archived == 0)
            Divider()
            Button("Highlight") { reader?.toggleHighlight() }
                .keyboardShortcut("h", modifiers: [.command, .shift])
                .disabled(reader == nil)
        }
        CommandMenu("Go") {
            Button("Previous Chapter") { if let reader { reader.go(toChapter: reader.chapter - 1, .instant) } }
                .keyboardShortcut("[")
                .disabled((reader?.chapter ?? 0) == 0)
            Button("Next Chapter") { if let reader { reader.go(toChapter: reader.chapter + 1, .instant) } }
                .keyboardShortcut("]")
                .disabled(reader.map { $0.chapter + 1 >= $0.chapters } ?? true)
        }
        CommandGroup(after: .toolbar) {
            Button("Bigger Text") { resize(1) }
                .keyboardShortcut("+")
            Button("Smaller Text") { resize(-1) }
                .keyboardShortcut("-")
            Divider()
        }
        CommandGroup(before: .sidebar) {
            ForEach(Array(Section.sidebar.enumerated()), id: \.element) { index, section in
                Button(section.title) { navigator.section = section }
                    .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")))
            }
            Divider()
        }
    }

    private func resize(_ by: Int) {
        var look = shelf.look
        look.textSize = UInt16(clamping: Int(look.textSize) + by)
        shelf.setLook(look)
    }
}
#endif

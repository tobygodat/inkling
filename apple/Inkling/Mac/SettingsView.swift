#if os(macOS)
import SwiftUI

/// The Settings window: where the library lives.
struct SettingsView: View {
    @Environment(Shelf.self) private var shelf

    var body: some View {
        TabView {
            Tab("Library", systemImage: "books.vertical") {
                Form {
                    LabeledContent("Folder") {
                        VStack(alignment: .trailing, spacing: 6) {
                            Text(shelf.folder.path(percentEncoded: false))
                                .font(.system(.body, design: .monospaced))
                                .textSelection(.enabled)
                                .truncationMode(.middle)
                                .lineLimit(1)
                            Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([shelf.folder]) }
                        }
                    }
                    LabeledContent("Items", value: "\(shelf.items.count)")
                }
                .formStyle(.grouped)
            }
        }
        .frame(width: 520, height: 220)
    }
}
#endif

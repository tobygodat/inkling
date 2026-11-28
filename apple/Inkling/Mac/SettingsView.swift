#if os(macOS)
import SwiftUI

enum SettingsTab: Hashable {
    case profile, account, feeds, reading, writing, provider, library
}

/// The Settings window: Profile (the newsletter address, Highlights and the Archive), Account &
/// sync, RSS/Atom feeds, reading/writing looks, AI provider and library.
struct SettingsView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.colorScheme) private var scheme
    @State private var tab: SettingsTab

    init(initialTab: SettingsTab = .profile) {
        _tab = State(initialValue: initialTab)
    }

    var body: some View {
        let palette = Chrome(shelf.looks).palette(scheme)
        TabView(selection: $tab) {
            Tab("Profile", systemImage: "person.crop.circle", value: .profile) {
                ProfileSettingsView()
                    .frame(width: 640, height: 400)
            }
            Tab("Account", systemImage: "person.badge.key", value: .account) {
                AccountSettingsView()
                    .frame(width: 640, height: 360)
            }
            Tab("Feeds", systemImage: "dot.radiowaves.up.forward", value: .feeds) {
                FeedsSettingsView()
                    .frame(width: 640, height: 380)
            }
            Tab("Reading", systemImage: "textformat.size", value: .reading) {
                AppearancePanel(target: .reading, includeListDensity: true)
                    .frame(width: 640)
                    .padding(.vertical, 8)
            }
            Tab("Writing", systemImage: "pencil.line", value: .writing) {
                AppearancePanel(target: .writing)
                    .frame(width: 640)
                    .padding(.vertical, 8)
            }
            Tab("AI", systemImage: "sparkles", value: .provider) {
                ProviderPanel()
                    .frame(width: 640, height: 280)
            }
            Tab("Library", systemImage: "books.vertical", value: .library) {
                Form {
                    SwiftUI.Section {
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
                    .settingsCard(palette)
                }
                .settingsForm(palette)
                .frame(width: 640, height: 160)
            }
        }
        .environment(\.palette, palette)
    }
}
#endif

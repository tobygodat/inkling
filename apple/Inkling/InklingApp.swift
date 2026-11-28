import SwiftUI

@main
struct InklingApp: App {
    @State private var shelf = Shelf.launch()
    @State private var navigator = Navigator()
    @AppStorage("appearance") private var appearance = Appearance.system

    init() {
        Fonts.register()
        #if os(iOS)
        FrameProbe.startIfAsked()
        if let title = UIFont(name: Fonts.serifSemibold, size: 34) {
            UINavigationBar.appearance().largeTitleTextAttributes = [.font: UIFontMetrics(forTextStyle: .largeTitle).scaledFont(for: title)]
        }
        #endif
    }

    var body: some Scene {
        #if os(macOS)
        WindowGroup {
            root
        }
        .defaultSize(width: 1280, height: 800)
        .commands { InklingCommands(shelf: shelf, navigator: navigator) }

        Settings {
            SettingsView()
                .environment(shelf)
                .environment(navigator)
                .preferredColorScheme(Appearance.forced ?? appearance.scheme)
        }
        #else
        WindowGroup {
            root
        }
        #endif
    }

    private var root: some View {
        RootView()
            .environment(shelf)
            .environment(navigator)
            #if os(iOS)
            .preferredColorScheme(navigator.readerScheme ?? Appearance.forced ?? appearance.scheme)
            #else
            .preferredColorScheme(Appearance.forced ?? appearance.scheme)
            #endif
    }
}

import SwiftUI

@main
struct InklingApp: App {
    @State private var shelf = Shelf.launch()
    @State private var navigator = Navigator()

    init() {
        Fonts.register()
        #if os(iOS)
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
            .preferredColorScheme(Self.forced)
    }

    /// Light or Dark from `INKLING_APPEARANCE`, for the snapshot tests on a headless Simulator.
    private static var forced: ColorScheme? {
        switch ProcessInfo.processInfo.environment["INKLING_APPEARANCE"] {
        case "light": .light
        case "dark": .dark
        default: nil
        }
    }
}

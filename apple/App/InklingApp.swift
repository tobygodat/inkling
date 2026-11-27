import InklingUI
import SwiftUI

/// The native Inkling (ADR 0007): one window over the library, on the Mac and on iPhone.
@main
struct InklingApp: App {
    @State private var shelf: Shelf

    init() {
        Typeface.register()
        _shelf = State(initialValue: Shelf(data: Shelf.defaultData()))
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(shelf)
                #if os(macOS)
                .frame(minWidth: 900, minHeight: 560)
                #endif
        }
        #if os(macOS)
        .defaultSize(width: 1280, height: 800)
        #endif
    }
}

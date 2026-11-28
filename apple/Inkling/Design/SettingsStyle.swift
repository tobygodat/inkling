import SwiftUI

extension View {
    /// A settings form on the app's palette, on the Mac and on iPhone: the page on Ground, each
    /// group a Segment card, and buttons in Ink.
    func settingsForm(_ palette: Palette) -> some View {
        formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .background(Color(palette.ground).ignoresSafeArea())
            .tint(Color(palette.ink))
    }

    /// One group of a settings form, as a Segment card. The system's grouped rows are Surface on
    /// Ground, 1.04:1 in Light, so the cards vanished into the page.
    func settingsCard(_ palette: Palette) -> some View {
        listRowBackground(Color(palette.segment))
            .listRowSeparatorTint(Color(palette.hairline))
    }
}

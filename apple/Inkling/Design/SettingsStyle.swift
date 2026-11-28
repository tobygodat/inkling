import SwiftUI

extension View {
    /// A settings form on the app's palette, on the Mac and on iPhone: the page on Ground, each
    /// group a Segment card, buttons in Ink, and secondary words in Muted, which reads at 4.5:1
    /// on both. The system's secondary grey is 3.4:1 on Ground.
    func settingsForm(_ palette: Palette) -> some View {
        formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .background(Color(palette.ground).ignoresSafeArea())
            .foregroundStyle(Color(palette.ink), Color(palette.muted), Color(palette.muted))
            .tint(Color(palette.ink))
    }


    /// One group of a settings form, as a Segment card. The system's grouped rows are Surface on
    /// Ground, 1.04:1 in Light, so the cards vanished into the page.
    func settingsCard(_ palette: Palette) -> some View {
        listRowBackground(Color(palette.segment))
            .listRowSeparatorTint(Color(palette.hairline))
    }
}

extension Palette {
    /// A settings field's placeholder in Muted: the system's is 1.7:1 on a Segment card.
    func prompt(_ words: String) -> Text {
        Text(words).foregroundStyle(Color(muted))
    }

    /// A settings header or footer in Muted, 6.0:1 on Ground; the system's grey is 3.4:1.
    func caption(_ words: String) -> Text {
        Text(words).foregroundStyle(Color(muted))
    }
}

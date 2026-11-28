import SwiftUI

extension View {
    /// A settings form on the app's palette, on the Mac and on iPhone: the page on Ground, each
    /// group a `SettingsCard`, buttons in Ink, and secondary words in Muted, which reads at 4.5:1
    /// on both. The system's secondary grey is 3.4:1 on Ground.
    func settingsForm(_ palette: Palette) -> some View {
        formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .background(Color(palette.ground).ignoresSafeArea())
            .foregroundStyle(Color(palette.ink), Color(palette.muted), Color(palette.muted))
            .tint(Color(palette.ink))
    }
}

/// The rows of one settings group as a Segment card outlined in Muted. Segment is only 1.1:1 on
/// Ground, so the outline, 6.0:1, is what shows the card's edge. Forms have no section background,
/// so each row draws its part of the outline: the sides, plus the top on the first row and the
/// bottom on the last, leaving Hairline separators between rows. Its 26 pt corners are the
/// system's card corners on iOS 26, which clip the row backgrounds. The Mac's grouped form draws
/// its own cards and ignores row backgrounds, so there the rows pass through unchanged (TOB-578).
struct SettingsCard<Content: View>: View {
    @Environment(\.palette) private var palette
    @ViewBuilder var content: Content

    var body: some View {
        #if os(macOS)
        content.listRowSeparatorTint(Color(palette.hairline))
        #else
        Group(subviews: content) { rows in
            ForEach(rows) { row in
                row.listRowBackground(edge(top: row.id == rows.first?.id, bottom: row.id == rows.last?.id))
            }
        }
        .listRowSeparatorTint(Color(palette.hairline))
        #endif
    }

    private func edge(top: Bool, bottom: Bool) -> some View {
        let shape = UnevenRoundedRectangle(topLeadingRadius: top ? 26 : 0, bottomLeadingRadius: bottom ? 26 : 0, bottomTrailingRadius: bottom ? 26 : 0, topTrailingRadius: top ? 26 : 0, style: .circular)
        return shape.fill(Color(palette.segment))
            .overlay(shape.stroke(Color(palette.muted), lineWidth: 2).clipShape(shape))
            .padding(.top, top ? 0 : -2)
            .padding(.bottom, bottom ? 0 : -2)
            .clipped()
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

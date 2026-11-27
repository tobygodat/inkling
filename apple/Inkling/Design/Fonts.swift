import CoreText
import SwiftUI

/// Source Serif 4 for reading and titles, Inter for the interface (DESIGN.md), both bundled.
nonisolated enum Fonts {
    static let serif = "SourceSerif4-Regular"
    static let serifItalic = "SourceSerif4-It"
    static let serifSemibold = "SourceSerif4-Semibold"
    static let inter = "Inter-Regular"
    static let interMedium = "Inter-Medium"
    static let interSemibold = "Inter-SemiBold"

    private static let registered: Void = {
        guard let folder = Bundle.inkling.url(forResource: "fonts", withExtension: nil),
              let files = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil) else { return }
        let fonts = files.filter { $0.pathExtension == "ttf" }
        CTFontManagerRegisterFontURLs(fonts as CFArray, .process, true, nil)
    }()

    static func register() {
        _ = registered
    }
}

extension Font {
    /// Source Serif 4, scaled with Dynamic Type from `style`.
    static func serif(_ size: CGFloat, weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        .custom(weight == .regular ? Fonts.serif : Fonts.serifSemibold, size: size, relativeTo: style)
    }

    /// Inter, scaled with Dynamic Type from `style`.
    static func inter(_ size: CGFloat, weight: Font.Weight = .regular, relativeTo style: Font.TextStyle = .body) -> Font {
        let name = switch weight {
        case .medium: Fonts.interMedium
        case .semibold, .bold: Fonts.interSemibold
        default: Fonts.inter
        }
        return .custom(name, size: size, relativeTo: style)
    }
}

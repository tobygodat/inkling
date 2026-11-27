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

    /// A reading face at `size`: Source Serif 4 from the bundle, every other face from the system.
    static func reading(_ family: String, size: CGFloat, bold: Bool = false, italic: Bool = false) -> PlatformFont {
        if family == "Source Serif 4" {
            return PlatformFont(name: bold ? serifSemibold : (italic ? serifItalic : serif), size: size) ?? .systemFont(ofSize: size)
        }
        let plain = PlatformFontDescriptor(fontAttributes: [.family: family])
        #if os(macOS)
        var traits: NSFontDescriptor.SymbolicTraits = []
        if italic { traits.insert(.italic) }
        if bold { traits.insert(.bold) }
        return PlatformFont(descriptor: plain.withSymbolicTraits(traits), size: size) ?? PlatformFont(name: serif, size: size) ?? .systemFont(ofSize: size)
        #else
        var traits: UIFontDescriptor.SymbolicTraits = []
        if italic { traits.insert(.traitItalic) }
        if bold { traits.insert(.traitBold) }
        return PlatformFont(descriptor: plain.withSymbolicTraits(traits) ?? plain, size: size)
        #endif
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

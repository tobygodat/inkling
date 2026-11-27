import CoreText
import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// Inkling's colours from DESIGN.md, light and dark.
public enum Palette {
    public static let ground = Color(light: 0xFBFBF9, dark: 0x15171A)
    public static let surface = Color(light: 0xFFFFFF, dark: 0x1E2125)
    public static let ink = Color(light: 0x1F2328, dark: 0xE8E6E1)
    public static let muted = Color(light: 0x5C6168, dark: 0xA3A7AD)
    public static let hairline = Color(light: 0xDEDFDA, dark: 0x34383E)
    public static let segment = Color(light: 0xEEEFEB, dark: 0x25282D)
    public static let link = Color(light: 0x0969DA, dark: 0x58A6FF)
}

/// Inkling's faces: Source Serif 4 for what is read, Inter for the app around it.
public enum Typeface {
    #if os(macOS)
    public static let body: CGFloat = 21
    public static let title: CGFloat = 36
    #else
    public static let body: CGFloat = 19
    public static let title: CGFloat = 28
    #endif

    /// The reader's line height, as the gap Text adds between lines.
    public static func leading(_ size: CGFloat) -> CGFloat {
        #if os(macOS)
        size * 0.55
        #else
        size * 0.45
        #endif
    }

    public static func serif(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .custom("Source Serif 4", size: size).weight(weight)
    }

    public static func ui(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .custom("Inter", size: size).weight(weight)
    }

    /// Registers the bundled faces with this process, once. The app calls it at launch.
    public static func register() {
        _ = registered
    }

    private static let registered: Void = {
        let fonts = Bundle.module.urls(forResourcesWithExtension: "ttf", subdirectory: "Fonts") ?? []
        for font in fonts {
            CTFontManagerRegisterFontsForURL(font as CFURL, .process, nil)
        }
    }()
}

extension Color {
    init(light: UInt32, dark: UInt32) {
        #if canImport(AppKit)
        self.init(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? NSColor(hex: dark) : NSColor(hex: light)
        })
        #else
        self.init(uiColor: UIColor { traits in
            traits.userInterfaceStyle == .dark ? UIColor(hex: dark) : UIColor(hex: light)
        })
        #endif
    }
}

#if canImport(AppKit)
extension NSColor {
    convenience init(hex: UInt32) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}
#else
extension UIColor {
    convenience init(hex: UInt32) {
        self.init(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}
#endif

import InklingCore
import SwiftUI

#if os(macOS)
import AppKit
typealias PlatformColor = NSColor
typealias PlatformFont = NSFont
#else
import UIKit
typealias PlatformColor = UIColor
typealias PlatformFont = UIFont
#endif

/// The colours a screen draws, made by `inkling_core` (`Looks.colors`), so DESIGN.md's tokens
/// have one source.
nonisolated struct Palette: Equatable {
    let ground: PlatformColor
    let surface: PlatformColor
    let ink: PlatformColor
    let muted: PlatformColor
    let hairline: PlatformColor
    let segment: PlatformColor
    let link: PlatformColor
    let selection: PlatformColor
    let highlight: PlatformColor
    let addedText: PlatformColor
    let addedFill: PlatformColor
    let addedLine: PlatformColor
    let addedCount: PlatformColor
    let removedText: PlatformColor
    let removedFill: PlatformColor
    let removedLine: PlatformColor
    let isDark: Bool

    init(_ c: Colors) {
        func color(_ hex: UInt32) -> PlatformColor {
            PlatformColor(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
        }
        ground = color(c.ground)
        surface = color(c.surface)
        ink = color(c.ink)
        muted = color(c.muted)
        hairline = color(c.hairline)
        segment = color(c.segment)
        link = color(c.link)
        selection = color(c.selection)
        highlight = color(c.highlight)
        addedText = color(c.addedText)
        addedFill = color(c.addedFill)
        addedLine = color(c.addedLine)
        addedCount = color(c.addedCount)
        removedText = color(c.removedText)
        removedFill = color(c.removedFill)
        removedLine = color(c.removedLine)
        let r = Double((c.ground >> 16) & 0xFF), g = Double((c.ground >> 8) & 0xFF), b = Double(c.ground & 0xFF)
        isDark = (0.299 * r + 0.587 * g + 0.114 * b) < 128
    }
}

extension Color {
    init(_ platform: PlatformColor) {
        #if os(macOS)
        self.init(nsColor: platform)
        #else
        self.init(uiColor: platform)
        #endif
    }
}

/// Light and Dark for the app's own screens, which follow the system's appearance.
struct Chrome {
    let light: Palette
    let dark: Palette

    init(_ looks: Looks) {
        light = Palette(looks.colors(tone: .light))
        dark = Palette(looks.colors(tone: .dark))
    }

    func palette(_ scheme: ColorScheme) -> Palette { scheme == .dark ? dark : light }
}

private struct ChromeKey: EnvironmentKey {
    /// Light, as core makes it from a folder with no settings. Every screen is given its own.
    static let defaultValue = Palette(Looks.open(data: NSTemporaryDirectory()).colors(tone: .light))
}

extension EnvironmentValues {
    /// The palette of the screen on show, for the system's light or dark appearance.
    var palette: Palette {
        get { self[ChromeKey.self] }
        set { self[ChromeKey.self] = newValue }
    }
}

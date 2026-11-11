// Renders Inkling's app icons from DESIGN.md's colours and the bundled Source Serif 4.
// Usage: swift icon/make-icon.swift <repo root>
// Writes the iPhone icon into ios/app/Inkling/Assets.xcassets (opaque, full bleed; iOS masks
// it) and icon/Inkling.icns for the Mac (a rounded square with the system margin).
import AppKit
import CoreText

let root = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
let fontURL = root.appendingPathComponent("ui/fonts/SourceSerif4-Semibold.ttf")
guard CTFontManagerRegisterFontsForURL(fontURL as CFURL, .process, nil) else { fatalError("register \(fontURL.path)") }

func rgb(_ hex: UInt32) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: 1)
}
let ground = rgb(0xFBFBF9), ink = rgb(0x1F2328), addedLine = rgb(0x116329), hairline = rgb(0xDEDFDA)

/// Draws the mark into `rect`: an ink "i" with an accepted-change underline.
func mark(_ ctx: CGContext, _ rect: CGRect) {
    let s = rect.width
    let font = CTFontCreateWithName("SourceSerif4-Semibold" as CFString, s * 0.62, nil)
    let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: ink]
    let line = CTLineCreateWithAttributedString(NSAttributedString(string: "i", attributes: attrs))
    let bounds = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
    let x = rect.midX - bounds.midX
    let y = rect.minY + s * 0.30 - bounds.minY
    ctx.textPosition = CGPoint(x: x, y: y)
    CTLineDraw(line, ctx)
    let w = s * 0.40, h = s * 0.045
    ctx.setFillColor(addedLine)
    ctx.addPath(CGPath(roundedRect: CGRect(x: rect.midX - w / 2, y: rect.minY + s * 0.215, width: w, height: h),
                       cornerWidth: h / 2, cornerHeight: h / 2, transform: nil))
    ctx.fillPath()
}

func render(_ size: Int, opaque: Bool, draw: (CGContext, CGFloat) -> Void) -> CGImage {
    let info = opaque ? CGImageAlphaInfo.noneSkipLast.rawValue : CGImageAlphaInfo.premultipliedLast.rawValue
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: info)!
    draw(ctx, CGFloat(size))
    return ctx.makeImage()!
}

func writePNG(_ image: CGImage, _ url: URL) {
    let rep = NSBitmapImageRep(cgImage: image)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

let out = root.appendingPathComponent("icon")
writePNG(render(1024, opaque: true) { ctx, s in
    ctx.setFillColor(ground); ctx.fill(CGRect(x: 0, y: 0, width: s, height: s))
    mark(ctx, CGRect(x: 0, y: 0, width: s, height: s))
}, root.appendingPathComponent("ios/app/Inkling/Assets.xcassets/AppIcon.appiconset/icon-1024.png"))

// macOS: an 824/1024 rounded square, centred, with a hairline edge.
func macIcon(_ ctx: CGContext, _ s: CGFloat) {
    let inset = s * 100 / 1024, side = s - 2 * inset
    let rect = CGRect(x: inset, y: inset, width: side, height: side)
    let path = CGPath(roundedRect: rect, cornerWidth: side * 0.225, cornerHeight: side * 0.225, transform: nil)
    ctx.setFillColor(ground); ctx.addPath(path); ctx.fillPath()
    ctx.setStrokeColor(hairline); ctx.setLineWidth(max(1, s / 512)); ctx.addPath(path); ctx.strokePath()
    mark(ctx, rect)
}
let iconset = FileManager.default.temporaryDirectory.appendingPathComponent("Inkling.iconset")
try? FileManager.default.removeItem(at: iconset)
try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for base in [16, 32, 128, 256, 512] {
    writePNG(render(base, opaque: false, draw: macIcon), iconset.appendingPathComponent("icon_\(base)x\(base).png"))
    writePNG(render(base * 2, opaque: false, draw: macIcon), iconset.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}
let p = Process()
p.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
p.arguments = ["-c", "icns", iconset.path, "-o", out.appendingPathComponent("Inkling.icns").path]
try! p.run(); p.waitUntilExit()
guard p.terminationStatus == 0 else { fatalError("iconutil failed") }

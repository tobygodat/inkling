#if os(macOS)
import AppKit
import Foundation
import InklingCore
import SwiftUI
import Testing

@testable import InklingUI

/// Renders the native app's screens offscreen into `INKLING_SNAP_OUT` (apple/snap.sh sets it to
/// docs/screenshots/apple). The window never becomes key and sits off every screen, and the
/// process never activates, so nothing takes focus.
@MainActor @Suite(.serialized) struct Snapshots {
    @Test func theLibraryAndTheReaderInLightAndDark() async throws {
        let data = FileManager.default.temporaryDirectory.appending(path: "inkling-snap-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: data) }
        let shelf = Shelf(data: data)

        let empty = try await render(RootView().environment(shelf), name: "library-empty")
        #expect(empty.colours > 4)

        let tide = try await shelf.save(html: Fixture.tide, from: "https://shore.example/tide", fetcher: Pictures())
        let light = try await shelf.save(html: Fixture.light, from: "https://harbour.example/light", fetcher: Pictures())
        await shelf.keep(light.id, chapter: 0, fraction: 0.4)
        #expect(shelf.items.map(\.title).sorted() == ["Notes on the tide", "The harbour light"])
        #expect(shelf.items.first { $0.id == tide.id }?.kind == .article)

        let book = try await shelf.book(tide.id)
        #expect(Blocks.parse(book.chapters[0].text).contains { $0.kind == .note(label: "1") })
        #expect(Blocks.parse(book.chapters[0].text).contains { block in
            if case .image = block.kind { return true }
            return false
        })

        for dark in [false, true] {
            let reading = try await render(RootView(open: tide.id).environment(shelf), name: dark ? "reader-dark" : "reader", dark: dark)
            #expect(reading.colours > 40, "the reader drew next to nothing")
        }
    }

    /// Hosts `view` in an offscreen window, lets it load, and writes it as a PNG.
    private func render(_ view: some View, name: String, dark: Bool = false) async throws -> Shot {
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        Typeface.register()

        let size = CGSize(width: 1280, height: 800)
        let host = NSHostingView(rootView: view.frame(width: size.width, height: size.height))
        host.frame = CGRect(origin: .zero, size: size)
        let window = Offscreen(contentRect: CGRect(origin: CGPoint(x: -20_000, y: -20_000), size: size),
                               styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        window.contentView = host
        window.orderFrontRegardless()
        defer { window.orderOut(nil) }

        // The reader opens its book and pictures off the main thread; give it time to settle.
        for _ in 0..<20 {
            try await Task.sleep(for: .milliseconds(100))
            host.layoutSubtreeIfNeeded()
        }

        let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let png = try #require(bitmap.representation(using: .png, properties: [:]))
        if let out = ProcessInfo.processInfo.environment["INKLING_SNAP_OUT"], !out.isEmpty {
            let folder = URL(fileURLWithPath: out, isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try png.write(to: folder.appending(path: "\(name).png"))
        }
        return Shot(bitmap: bitmap)
    }
}

/// A window that may sit off every screen and never becomes key.
private final class Offscreen: NSWindow {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}

private struct Shot {
    let bitmap: NSBitmapImageRep

    /// How many different colours a grid across the image finds: a blank render finds one or two.
    var colours: Int {
        var seen = Set<UInt32>()
        for y in stride(from: 0, to: bitmap.pixelsHigh, by: 7) {
            for x in stride(from: 0, to: bitmap.pixelsWide, by: 7) {
                guard let colour = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
                let r = UInt32(colour.redComponent * 255), g = UInt32(colour.greenComponent * 255), b = UInt32(colour.blueComponent * 255)
                seen.insert(r << 16 | g << 8 | b)
            }
        }
        return seen.count
    }
}

/// Pictures for the saved pages, drawn here so the test never touches the network.
private final class Pictures: Fetcher {
    let png: Data

    init() {
        let image = NSImage(size: NSSize(width: 640, height: 320), flipped: false) { rect in
            NSGradient(starting: NSColor(hex: 0x8DB3C7), ending: NSColor(hex: 0xE9D9B8))?.draw(in: rect, angle: 90)
            NSColor(hex: 0x5C6168).setFill()
            NSBezierPath(rect: NSRect(x: 0, y: 0, width: rect.width, height: rect.height * 0.3)).fill()
            return true
        }
        let tiff = image.tiffRepresentation ?? Data()
        png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) ?? Data()
    }

    func fetch(url: String) -> Data? { png }
}

enum Fixture {
    static let tide = """
    <html><head><title>Notes on the tide</title><meta name="author" content="Ada Marsh"></head><body>
    <nav><a href="/">Home</a> <a href="/about">About</a></nav>
    <article>
    <h1>Notes on the tide</h1>
    <p>The tide came in <em>slowly</em> over the flats, and the birds lifted as it reached them. \
    Nobody on the shore was in a hurry; the water would arrive when it arrived, and the light would \
    go with it.<sup><a href="#fn1" class="footnote-ref">1</a></sup></p>
    <p><img src="/flats.png" alt="The flats at low water"></p>
    <h2>What the water carries</h2>
    <p>Weed, mostly, and the small wreckage of the week: a float, a length of blue rope, a shoe that \
    had walked a long way without its owner. The <a href="https://shore.example/birds">gulls</a> \
    went through all of it first.</p>
    <blockquote><p>The sea gives back what it is given, but never to the one who gave it.</p></blockquote>
    <ul><li>Bladderwrack and kelp</li><li>Rope, in three colours</li><li>One shoe</li></ul>
    <p>By evening the flats were gone, and the birds had settled on the posts of the old pier, \
    facing into the wind, waiting for the water to go out again so the whole thing could start over.</p>
    <div class="footnotes"><ol><li id="fn1" class="footnote">The light goes first, then the water.</li></ol></div>
    </article>
    <footer>Subscribe for more notes from the shore.</footer>
    </body></html>
    """

    static let light = """
    <html><head><title>The harbour light</title></head><body><article>
    <h1>The harbour light</h1>
    <p>The light at the end of the harbour wall had been automatic for thirty years, but someone still \
    walked out to it every evening, as if it might forget. It never did, and the walk was the point.</p>
    <p>On clear nights you could see it from the hill above the town, a small regular blink against \
    the dark, and it was easy to believe it was keeping time for everything else.</p>
    </article></body></html>
    """
}
#endif

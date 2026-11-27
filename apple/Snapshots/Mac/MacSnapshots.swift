import AppKit
import InklingCore
import SwiftUI
import Testing

/// The Mac app's screens rendered offscreen, in Light and Dark, into `INKLING_SNAP_OUT`, with the
/// behaviour they show asserted on the way. Nothing is put on screen and nothing takes focus.
@Suite(.serialized, .enabled(if: ProcessInfo.processInfo.environment["INKLING_DEMO"] != nil, "apple/snap.sh sets INKLING_DEMO and INKLING_FIXTURES"))
final class MacSnapshots {
    private let out: URL
    private let shelf: Shelf
    private let data: URL

    init() throws {
        let environment = ProcessInfo.processInfo.environment
        let demo = environment["INKLING_DEMO"] ?? ""
        let fixtures = environment["INKLING_FIXTURES"] ?? ""
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        Fonts.register()
        out = URL(filePath: environment["INKLING_SNAP_OUT"] ?? NSTemporaryDirectory()).appending(path: "mac")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        data = FileManager.default.temporaryDirectory.appending(path: "inkling-snap-\(UUID().uuidString)")
        shelf = try Shelf(folder: data)
        Demo.seed(shelf.library, from: URL(filePath: demo), books: URL(filePath: fixtures))
        shelf.reload()
    }

    deinit {
        try? FileManager.default.removeItem(at: data)
    }

    @Test func everyScreenInLightAndDark() async throws {
        #expect(shelf.queue.map(\.title) == ["Walking", "An Apology for Idlers", "Self-Reliance"], "the Queue is the articles still to read, newest first")
        #expect(shelf.archive.map(\.title) == ["Resistance to Civil Government"])
        #expect(shelf.books.count == 3)
        let walking = try #require(shelf.queue.first)
        #expect(walking.picture.map { FileManager.default.fileExists(atPath: $0) } == true, "an article's first picture is its row's")
        #expect(walking.excerpt?.isEmpty == false)
        #expect(shelf.queue.first { $0.title == "Self-Reliance" }?.picture == nil, "an article with no picture has none")
        let walden = try #require(shelf.books.first { $0.title == "Walden" })
        #expect(walden.progress == 0, "a book not yet read opens at its start")

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            for section in Section.allCases {
                let navigator = Navigator()
                navigator.section = section
                if section == .queue { navigator.selection = shelf.queue[0].id }
                if section == .library { navigator.selection = shelf.books.first { $0.title.hasPrefix(name == "light" ? "Walden" : "Pride") }?.id }
                try await shoot(root(navigator), "\(name)-\(section.rawValue)", appearance)
            }
            try await shoot(SaveLinkSheet().environment(shelf).environment(\.palette, palette(appearance)), "\(name)-save-link", appearance, size: CGSize(width: 460, height: 150), titled: false)
            try await shoot(SettingsView().environment(shelf), "\(name)-settings", appearance, size: CGSize(width: 440, height: 400))
        }
    }

    @Test func aLookIsKeptAndRedrawsTheReader() async throws {
        var look = shelf.look
        look.tone = .sepia
        look.face = "Charter"
        look.textSize = 40
        look.lineSpacing = 160
        shelf.setLook(look)
        #expect(shelf.look.textSize == 32, "core clamps the size")
        #expect(try Shelf(folder: data).look == shelf.look, "the look is kept through a relaunch")

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.selection = shelf.queue[1].id
            try await shoot(root(navigator), "\(name)-reader-sepia", appearance)
            try await shoot(AppearancePanel().environment(shelf), "\(name)-appearance", appearance, size: CGSize(width: 380, height: 300), titled: false)
        }
    }

    @Test func archivingHighlightsAndThePlaceOutliveARelaunch() async throws {
        let idlers = try #require(shelf.queue.first { $0.title == "An Apology for Idlers" })
        shelf.setArchived(idlers, true)
        #expect(shelf.archive.first?.id == idlers.id, "the newest archived comes first")
        shelf.setArchived(idlers, false)

        let book = try await shelf.open(idlers.id)
        let set = Typesetting(face: "Source Serif 4", size: 21, lineHeight: 1.75, titleSize: 36, palette: palette(.aqua), column: 700)
        let rendered = Renderer.render(book, set: set, folder: URL(filePath: shelf.library.folder()), byline: nil)
        let model = ReaderModel(item: idlers, rendered: rendered, shelf: shelf)
        let words = "the work-house is too good for you"
        let at = (rendered.text.string as NSString).range(of: words)
        #expect(at.location != NSNotFound)
        model.highlight(at)
        let passage = try #require(model.highlights.first)
        #expect(passage.words == words)
        let markdown = book.chapters[0].text.utf8
        let start = markdown.index(markdown.startIndex, offsetBy: Int(passage.start))
        let end = markdown.index(markdown.startIndex, offsetBy: Int(passage.end))
        #expect(String(markdown[start..<end]) == words, "the passage is kept as the bytes of its chapter's Markdown")
        model.scrolled(toCharacter: rendered.chapters[0].location + rendered.chapters[0].length / 2, atEnd: false)
        shelf.flushPlaces()

        let relaunched = try Shelf(folder: data)
        let item = try #require(relaunched.item(idlers.id))
        #expect(abs(item.fraction - 0.5) < 0.01, "the place is kept")
        let again = ReaderModel(item: item, rendered: rendered, shelf: relaunched)
        #expect(again.highlightRanges == [at], "the highlight is drawn where it was made")
        let middle = rendered.chapters[0].location + rendered.chapters[0].length / 2
        #expect(again.jump.map { abs($0 - middle) <= 1 } == true, "the reader opens at the place, to the character a stored f32 fraction keeps")
    }

    @Test func chaptersAndFootnotes() async throws {
        let walden = try #require(shelf.books.first { $0.title == "Walden" })
        let book = try await shelf.open(walden.id)
        let set = Typesetting(face: "Source Serif 4", size: 21, lineHeight: 1.75, titleSize: 36, palette: palette(.aqua), column: 700)
        let rendered = Renderer.render(book, set: set, folder: URL(filePath: shelf.library.folder()), byline: nil)
        let text = rendered.text.string as NSString
        #expect(rendered.titles == ["Where I Lived, and What I Lived For", "Economy"])

        #expect(!text.contains("[^") && !text.contains("\u{1}"), "no footnote mark shows its Markdown")
        let mark = text.range(of: "teach.1").location + "teach.".count
        let offset = { (at: Int) in rendered.text.attribute(.baselineOffset, at: at, effectiveRange: nil) as? CGFloat ?? 0 }
        #expect(offset(mark) > offset(mark - 1) + 5, "the mark is raised")
        #expect(rendered.text.attribute(.foregroundColor, at: mark, effectiveRange: nil) as? NSColor == set.palette.link, "the mark is in Link")
        #expect(text.range(of: "1\tWritten at the pond in the spring of 1846.").location != NSNotFound, "the note hangs from its label")

        let model = ReaderModel(item: walden, rendered: rendered, shelf: shelf)
        let markdown = book.chapters[0].text.utf8
        for words in ["Written at the pond", "see if I could not learn what it had to teach"] {
            model.highlight(text.range(of: words))
            let passage = try #require(model.highlights.first { $0.words == words })
            let start = markdown.index(markdown.startIndex, offsetBy: Int(passage.start))
            let end = markdown.index(markdown.startIndex, offsetBy: Int(passage.end))
            #expect(String(markdown[start..<end]) == words, "a passage near a mark is kept as its chapter's own bytes")
            model.remove(passage)
        }

        let next = text.range(of: "Next: Economy")
        #expect(NSLocationInRange(next.location, rendered.chapters[0]), "\"Next: …\" ends the chapter before")
        let link = try #require(rendered.text.attribute(.link, at: next.location, effectiveRange: nil) as? URL)
        #expect(Renderer.chapter(of: link) == 1)
        #expect(text.range(of: "Next:", options: [], range: rendered.chapters[1]).location == NSNotFound, "the last chapter leads nowhere")
        model.go(toChapter: 1, .glide)
        #expect(model.jump == rendered.chapters[1].location && model.motion == .glide)
        model.jump = nil
        model.go(toChapter: 2, .fade)
        #expect(model.jump == nil, "there is no chapter after the last")

        #expect(ProcessInfo.processInfo.environment["INKLING_DATA_DIR"] != nil || dataFolder() == NSHomeDirectory() + "/Library/Application Support/Inkling", "the app opens the GPUI app's data folder")

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0)
            shelf.reload()
            let navigator = Navigator()
            navigator.section = .library
            navigator.selection = walden.id
            try await shoot(root(navigator), "\(name)-reader-footnotes", appearance)
            try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0.62)
            shelf.reload()
            try await shoot(root(navigator), "\(name)-reader-chapters", appearance)
        }
        try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0)
        shelf.reload()
    }

    @Test func aBookFileAndALinkAreAddedThroughTheBridge() async throws {
        let fixtures = URL(filePath: ProcessInfo.processInfo.environment["INKLING_FIXTURES"] ?? "")
        let folder = data.appending(path: "added")
        let added = try Shelf(folder: folder, session: Pages.session)
        await added.addBook(at: fixtures.appending(path: "walden.epub"))
        #expect(added.failure == nil)
        #expect(added.books.map(\.title) == ["Walden"])

        let item = try await added.save(link: "pages.test/tide")
        #expect(added.queue.first?.id == item.id, "a saved link goes to the top of the Queue")
        #expect(item.site == "pages.test")
        let book = try await added.open(item.id)
        let pictures = try FileManager.default.contentsOfDirectory(atPath: added.library.folder() + "/images/\(item.id)")
        #expect(pictures.count == 1, "the https picture is fetched and kept; the file: one is left out")
        #expect(book.chapters[0].text.contains("The flats at dawn"))
        await #expect(throws: SaveError.self) { try await added.save(link: "ftp://pages.test/tide") }
    }

    private func root(_ navigator: Navigator) -> some View {
        RootView().environment(shelf).environment(navigator)
    }

    private func palette(_ appearance: NSAppearance.Name) -> Palette {
        Chrome(shelf.looks).palette(appearance == .darkAqua ? .dark : .light)
    }

    /// Renders `view` in a window ordered in far off the screen, never key and never active, and
    /// takes the window server's own image of it, so the sidebar's glass and the toolbar draw as
    /// they do on screen.
    private func shoot(_ view: some View, _ name: String, _ appearance: NSAppearance.Name, size: CGSize = CGSize(width: 1280, height: 800), titled: Bool = true) async throws {
        let style: NSWindow.StyleMask = titled ? [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView] : [.titled, .fullSizeContentView]
        let window = Offscreen(contentRect: CGRect(origin: .zero, size: size), styleMask: style, backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: appearance)
        window.isReleasedWhenClosed = false
        if !titled { window.titlebarAppearsTransparent = true; window.titleVisibility = .hidden }
        let host = NSHostingController(rootView: view.preferredColorScheme(appearance == .darkAqua ? .dark : .light))
        host.sceneBridgingOptions = .all
        window.contentViewController = host
        window.setContentSize(size)
        window.setFrameOrigin(CGPoint(x: -40000, y: -40000))
        window.orderFrontRegardless()
        try await Task.sleep(for: .seconds(2.5))
        let image = try #require(Capture.window(window.windowNumber), "the window server gave no image of the window")
        let rep = NSBitmapImageRep(cgImage: image)
        let png = try #require(rep.representation(using: .png, properties: [:]))
        try png.write(to: out.appending(path: "\(name).png"))
        window.orderOut(nil)
        window.close()
    }
}

/// Serves a saved page and its pictures in place of the network.
nonisolated private final class Pages: URLProtocol {
    static let session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [Pages.self]
        return URLSession(configuration: configuration)
    }()

    private static let words = String(repeating: "The tide came in slowly over the flats, and the birds lifted as it reached them. ", count: 12)
    private static let page = """
    <html><head><title>The tide</title><meta name="author" content="Ana Lima"></head><body><article><h1>The tide</h1>
    <p>\(words)</p><figure><img src="/flats.png" alt="The flats at dawn"><figcaption>The flats at dawn</figcaption></figure>
    <p><img src="file:///etc/hosts.png" alt="Not fetched"></p><p>\(words)</p></article></body></html>
    """

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host() == "pages.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        let body = url.path().hasSuffix(".png") ? Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")! : Data(Self.page.utf8)
        let type = url.path().hasSuffix(".png") ? "image/png" : "text/html; charset=utf-8"
        let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": type])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

/// A window the system may put anywhere, so it can sit off every screen.
private final class Offscreen: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

/// `CGWindowListCreateImage`, which the SDK no longer declares; a process may still take its own
/// windows without the Screen Recording permission.
private enum Capture {
    private typealias Create = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
    private static let create: Create? = {
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else { return nil }
        return unsafeBitCast(symbol, to: Create.self)
    }()

    static func window(_ number: Int) -> CGImage? {
        let includingWindow: UInt32 = 1 << 3
        let ignoreFraming: UInt32 = 1 << 0, bestResolution: UInt32 = 1 << 3
        return create?(.null, includingWindow, UInt32(number), ignoreFraming | bestResolution)?.takeRetainedValue()
    }
}

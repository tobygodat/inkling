import Foundation
import InklingCore
import Observation
import SwiftUI

/// The library as the screens see it: every item, the sections made from them, and the calls that
/// change them. Every call into Rust that touches the disk runs off the main thread.
@Observable
final class Shelf {
    let folder: URL
    let library: Library
    let looks: Looks
    /// Fetches saved pages and their pictures.
    let session: URLSession
    private(set) var items: [Item]
    /// A failure to show the reader, in Rust's own words.
    var failure: String?
    @ObservationIgnored private var places: [String: (chapter: Int, fraction: Double)] = [:]
    @ObservationIgnored private var keeping: Task<Void, Never>?

    init(folder: URL, session: URLSession = .shared) throws {
        self.folder = folder
        self.session = session
        library = try Library.open(data: folder.path)
        looks = Looks.open(data: folder.path)
        items = library.items()
    }

    /// The data folder: `INKLING_DATA_DIR` when set, as every headless run sets it, else the
    /// folder the GPUI app keeps on the Mac, or the app's own on iPhone.
    static func launch() -> Shelf {
        let environment = ProcessInfo.processInfo.environment
        let folder = environment["INKLING_DATA_DIR"].map { URL(filePath: $0) }
            ?? URL.applicationSupportDirectory.appending(path: "Inkling")
        do {
            let shelf = try Shelf(folder: folder)
            if let demo = environment["INKLING_DEMO"], shelf.items.isEmpty {
                Demo.seed(shelf.library, from: URL(filePath: demo), books: environment["INKLING_FIXTURES"].map { URL(filePath: $0) })
                shelf.reload()
            }
            return shelf
        } catch {
            fatalError("The library in \(folder.path) couldn’t be opened: \(error)")
        }
    }

    var queue: [Item] { items.filter { $0.kind == .article && $0.archived == 0 } }
    var books: [Item] { items.filter { $0.kind != .article && $0.archived == 0 } }
    var archive: [Item] {
        items.enumerated().filter { $0.element.archived > 0 }
            .sorted { ($0.element.archived, -$0.offset) > ($1.element.archived, -$1.offset) }
            .map(\.element)
    }

    func item(_ id: String?) -> Item? {
        items.first { $0.id == id }
    }

    func count(_ section: Section) -> Int? {
        switch section {
        case .queue: queue.count
        case .library: books.count + queue.count
        case .archive: archive.count
        case .inbox, .notebook: nil
        }
    }

    func reload() {
        items = library.items()
    }

    func setArchived(_ item: Item, _ archived: Bool) {
        attempt { try library.setArchived(id: item.id, archived: archived) }
        withAnimation(.snappy(duration: 0.28)) { reload() }
    }

    /// Adds an EPUB or a PDF the reader picked. A file outside the app's sandbox is read inside
    /// its security scope.
    func addBook(at url: URL) async {
        let library = library
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            _ = try await Self.offMain { try library.addFile(path: url.path) }
            withAnimation(.snappy(duration: 0.28)) { reload() }
        } catch {
            failure = Self.words(error)
        }
    }

    /// Fetches the page at `link` and saves it as an article, its pictures fetched over http(s).
    func save(link: String) async throws -> Item {
        let text = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: text.contains("://") ? text : "https://\(text)"),
              url.scheme == "https" || url.scheme == "http", url.host() != nil else {
            throw SaveError("“\(text)” isn’t a web address.")
        }
        var request = URLRequest(url: url, timeoutInterval: 20)
        request.setValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await session.data(for: request)
        if let status = (response as? HTTPURLResponse)?.statusCode, !(200..<300).contains(status) {
            throw SaveError("The page answered \(status), so it couldn’t be saved.")
        }
        let encoding = (response.textEncodingName).flatMap { name in
            let cf = CFStringConvertIANACharSetNameToEncoding(name as CFString)
            return cf == kCFStringEncodingInvalidId ? nil : String.Encoding(rawValue: CFStringConvertEncodingToNSStringEncoding(cf))
        } ?? .utf8
        guard let html = String(data: data, encoding: encoding) ?? String(data: data, encoding: .isoLatin1) else {
            throw SaveError("The page couldn’t be read as text.")
        }
        let library = library
        let address = (response.url ?? url).absoluteString
        let pictures = PictureFetcher(session: session)
        do {
            let item = try await Self.offMain { try library.saveArticle(url: address, html: html, fetcher: pictures) }
            if item.archived > 0 { try? library.setArchived(id: item.id, archived: false) }
            withAnimation(.snappy(duration: 0.28)) { reload() }
            return item
        } catch {
            throw SaveError(Self.words(error))
        }
    }

    func open(_ id: String) async throws -> Book {
        let library = library
        let book = try await Self.offMain { try library.book(id: id) }
        try? library.opened(id: id)
        return book
    }

    /// Notes the reader's place at once and keeps it on disk once scrolling pauses.
    func keepPlace(_ id: String, chapter: Int, fraction: Double) {
        places[id] = (chapter, fraction)
        keeping?.cancel()
        keeping = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            self?.flushPlaces()
        }
        if let at = items.firstIndex(where: { $0.id == id }) {
            items[at].chapter = UInt32(chapter)
            items[at].fraction = Float(fraction)
            let chapters = max(Double(items[at].chapters), 1)
            items[at].progress = Float(min(1, (Double(chapter) + fraction) / chapters))
        }
    }

    /// Writes every place not yet kept, as the reader closes and the app goes to the background.
    func flushPlaces() {
        for (id, place) in places {
            try? library.setPosition(id: id, chapter: UInt32(place.chapter), fraction: Float(place.fraction))
        }
        places = [:]
    }

    func highlights(_ id: String) -> [Passage] {
        library.highlights(id: id)
    }

    func setHighlights(_ id: String, _ all: [Passage]) {
        attempt { try library.setHighlights(id: id, all: all) }
    }

    private func attempt(_ work: () throws -> Void) {
        do { try work() } catch { failure = Self.words(error) }
    }

    nonisolated static func offMain<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await Task.detached(priority: .userInitiated) { try work() }.value
    }

    nonisolated static func words(_ error: any Error) -> String {
        switch error {
        case let error as InklingError:
            switch error {
            case .Storage(let message), .Import(let message): message
            case .NotFound: "That item is no longer in the library."
            }
        case let error as SaveError: error.message
        default: error.localizedDescription
        }
    }
}

struct SaveError: Error {
    let message: String
    init(_ message: String) { self.message = message }
}

extension Item {
    /// Minutes to read at 230 words a minute, or nil when the length isn't known.
    var minutes: Int? {
        words > 0 ? max(1, Int((Double(words) / 230).rounded())) : nil
    }

    var isFinished: Bool { progress >= 0.98 }
}

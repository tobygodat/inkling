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
    let documents: DocumentStore
    let ai: AiEngine
    let account: Account
    /// Fetches saved pages and their pictures.
    let session: URLSession
    private(set) var items: [Item]
    /// The writer's documents, newest first.
    private(set) var docs: [DocInfo]
    /// How reading looks, kept in `settings.json` beside the GPUI app's.
    private(set) var look: Look
    /// How writing looks, kept in `settings.json` beside the GPUI app's.
    private(set) var writingLook: Look
    /// The document currently open in `WritingView`, so sync flushes it before fetching and reloads it if updated.
    @ObservationIgnored weak var activeWriter: WritingModel?
    /// The document removed last, held until the next action for Undo.
    private var deletedDoc: (info: DocInfo, text: String)?
    /// A failure to show the reader or writer, in Rust's own words.
    var failure: String?
    @ObservationIgnored private var places: [String: (chapter: Int, fraction: Double)] = [:]
    @ObservationIgnored private var keeping: Task<Void, Never>?

    init(folder: URL, session: URLSession = .shared, fakeServer: URL? = nil) throws {
        self.folder = folder
        self.session = session
        library = try Library.open(data: folder.path)
        looks = Looks.open(data: folder.path)
        documents = try DocumentStore.open(data: folder.path)
        ai = AiEngine.open(data: folder.path)
        #if os(macOS)
        let deviceKind = "mac"
        #else
        let deviceKind = "phone"
        #endif
        let syncClient = if let fakeServer {
            SyncClient.openFake(data: folder.path, deviceKind: deviceKind, serverFile: fakeServer.path)
        } else {
            SyncClient.open(data: folder.path, deviceKind: deviceKind)
        }
        account = Account(client: syncClient)
        look = looks.reading()
        writingLook = looks.writing()
        items = library.items()
        docs = documents.list()
        account.attach(shelf: self)
    }

    /// The data folder core names: `INKLING_DATA_DIR` when set, as every headless run sets it, else
    /// the folder the GPUI app keeps on the Mac, or the app's own on iPhone.
    static func launch() -> Shelf {
        let environment = ProcessInfo.processInfo.environment
        let folder = URL(filePath: dataFolder(), directoryHint: .isDirectory)
        do {
            let shelf = try Shelf(folder: folder)
            if let demo = environment["INKLING_DEMO"] {
                if shelf.items.isEmpty {
                    Demo.seed(shelf.library, from: URL(filePath: demo), books: environment["INKLING_FIXTURES"].map { URL(filePath: $0) })
                    shelf.reload()
                }
                if shelf.docs.isEmpty {
                    Demo.seedDocuments(shelf.documents)
                    shelf.reloadDocuments()
                }
            }
            return shelf
        } catch {
            fatalError("The library in \(folder.path) couldn’t be opened: \(error)")
        }
    }

    var queue: [Item] { items.filter { $0.kind == .article && !$0.inbox && $0.archived == 0 } }
    var inbox: [Item] { items.filter { $0.inbox && $0.archived == 0 } }
    var books: [Item] { items.filter { $0.kind != .article && !$0.inbox && $0.archived == 0 } }
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
        case .inbox: inbox.count
        case .library: books.count + queue.count
        case .archive: archive.count
        case .writing: docs.count
        case .notebook: nil
        }
    }

    func reload() {
        items = library.items()
    }

    // MARK: - Documents

    var canUndoDeleteDocument: Bool { deletedDoc != nil }

    func reloadDocuments() {
        docs = documents.list()
    }

    func lastOpenDocumentId() -> String? {
        documents.lastOpen()
    }

    func setLastOpenDocument(_ id: String?) {
        try? documents.setLastOpen(id: id)
    }

    func readDocument(_ id: String) -> String {
        (try? documents.read(id: id)) ?? ""
    }

    func saveDocument(id: String, text: String, triggerSync: Bool = true) throws {
        try documents.write(id: id, text: text)
        reloadDocuments()
        if triggerSync { account.syncSoon() }
    }

    func createDocument() -> String {
        deletedDoc = nil
        let id = documents.newId()
        try? documents.write(id: id, text: "")
        try? documents.setLastOpen(id: id)
        reloadDocuments()
        return id
    }

    func deleteDocument(_ doc: DocInfo) {
        let text = readDocument(doc.id)
        attempt { try documents.remove(id: doc.id) }
        deletedDoc = (doc, text)
        withAnimation(.snappy(duration: 0.25)) { reloadDocuments() }
        account.syncSoon()
    }

    func undoDeleteDocument() -> String? {
        guard let removed = deletedDoc else { return nil }
        deletedDoc = nil
        attempt { try documents.restore(id: removed.info.id, text: removed.text, changedMs: removed.info.changedMs) }
        withAnimation(.snappy(duration: 0.25)) { reloadDocuments() }
        account.syncSoon()
        return removed.info.id
    }

    /// The first non-empty body line after the title, stripped of Markdown marks for the list row.
    func documentExcerpt(_ id: String) -> String? {
        let text = readDocument(id)
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        guard lines.count > 1 else { return nil }
        let raw = lines[1]
            .replacingOccurrences(of: "**", with: "")
            .replacingOccurrences(of: "__", with: "")
            .replacingOccurrences(of: "*", with: "")
            .replacingOccurrences(of: "`", with: "")
            .trimmingCharacters(in: CharacterSet(charactersIn: "#>- "))
        return raw.isEmpty ? nil : raw
    }

    func setArchived(_ item: Item, _ archived: Bool) {
        attempt { try library.setArchived(id: item.id, archived: archived) }
        withAnimation(.snappy(duration: 0.28)) { reload() }
        account.syncSoon()
    }

    func setInbox(_ item: Item, _ inbox: Bool) {
        attempt { try library.setInbox(id: item.id, inbox: inbox) }
        withAnimation(.snappy(duration: 0.28)) { reload() }
        account.syncSoon()
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
            account.syncSoon()
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
            account.syncSoon()
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
    func flushPlaces(triggerSync: Bool = true) {
        guard !places.isEmpty else { return }
        for (id, place) in places {
            try? library.setPosition(id: id, chapter: UInt32(place.chapter), fraction: Float(place.fraction))
        }
        places = [:]
        if triggerSync { account.syncSoon() }
    }

    func highlights(_ id: String) -> [Passage] {
        library.highlights(id: id)
    }

    func setHighlights(_ id: String, _ all: [Passage]) {
        attempt { try library.setHighlights(id: id, all: all) }
        account.syncSoon()
    }

    /// Keeps reading's look, as core clamps it.
    func setLook(_ look: Look) {
        attempt { try looks.setReading(look: look) }
        self.look = looks.reading()
        self.writingLook = looks.writing()
    }

    /// Keeps writing's look, as core clamps it.
    func setWritingLook(_ look: Look) {
        attempt { try looks.setWriting(look: look) }
        self.writingLook = looks.writing()
        self.look = looks.reading()
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
            case .Storage(let message), .Import(let message), .InvalidRequest(let message), .Keychain(let message), .Sync(let message): message
            case .MissingKey(let provider): "Add your \(provider) API key in Settings before asking."
            case .NotFound: "That item is no longer here."
            case .SignedOut: "The sign-in has ended. Sign in again."
            case .Cancelled: "The sign-in wasn’t finished."
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

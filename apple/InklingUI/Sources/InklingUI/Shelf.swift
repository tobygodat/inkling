import Foundation
import InklingCore
import Observation

/// The library as the screens see it. Every call into Rust runs off the main thread; the rows it
/// shows are refreshed after each change.
@MainActor @Observable
public final class Shelf {
    public private(set) var items: [Item] = []
    /// Why the library couldn't be opened, if it couldn't.
    public private(set) var problem: String?
    public let folder: URL

    private let library: Library?

    /// Opens the library in `data`, the folder the GPUI app keeps too (see `defaultData`).
    public init(data: URL) {
        var library: Library?
        var problem: String?
        do {
            try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
            library = try Library.open(data: data.path(percentEncoded: false))
        } catch {
            problem = Shelf.describe(error)
        }
        self.library = library
        self.folder = URL(fileURLWithPath: library?.folder() ?? data.path(percentEncoded: false), isDirectory: true)
        self.problem = problem
        self.items = Shelf.ordered(library?.items() ?? [])
    }

    /// `INKLING_DATA_DIR` when set, else Application Support/Inkling: on the Mac the same folder as
    /// `inkling_core::Store`, so the library the GPUI app built shows up here.
    public nonisolated static func defaultData() -> URL {
        if let dir = ProcessInfo.processInfo.environment["INKLING_DATA_DIR"], !dir.isEmpty {
            return URL(fileURLWithPath: dir, isDirectory: true)
        }
        return URL.applicationSupportDirectory.appending(path: "Inkling", directoryHint: .isDirectory)
    }

    public func refresh() async {
        guard let library else { return }
        items = Shelf.ordered(await Task.detached { library.items() }.value)
    }

    public func item(_ id: String) -> Item? {
        items.first { $0.id == id }
    }

    /// Fetches the page at `link` and saves it as an article.
    public func save(link: String) async throws -> Item {
        let address = try Shelf.address(link)
        var request = URLRequest(url: address)
        request.timeoutInterval = 30
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw Problem.unreadable }
        guard (200..<300).contains(http.statusCode) else { throw Problem.status(http.statusCode) }
        let html = String(data: data, encoding: .utf8) ?? String(decoding: data, as: UTF8.self)
        return try await save(html: html, from: (http.url ?? address).absoluteString)
    }

    /// Saves a page already fetched from `url`. Its pictures are fetched once, here, through
    /// `fetcher`.
    public func save(html: String, from url: String, fetcher: any Fetcher = PictureFetcher()) async throws -> Item {
        let library = try open()
        let item = try await Task.detached {
            try library.saveArticle(url: url, html: html, fetcher: fetcher)
        }.value
        await refresh()
        return item
    }

    /// Adds an EPUB or a PDF, which the library copies in.
    public func add(file: URL) async throws -> Item {
        let library = try open()
        let scoped = file.startAccessingSecurityScopedResource()
        defer { if scoped { file.stopAccessingSecurityScopedResource() } }
        let path = file.path(percentEncoded: false)
        let item = try await Task.detached { try library.addFile(path: path) }.value
        await refresh()
        return item
    }

    /// Opens a book or an article to read, and notes that it was opened.
    public func book(_ id: String) async throws -> Book {
        let library = try open()
        return try await Task.detached {
            let book = try library.book(id: id)
            try library.opened(id: id)
            return book
        }.value
    }

    /// Keeps the reader's place: the chapter, from 0, and how far through it, from 0 to 1.
    public func keep(_ id: String, chapter: Int, fraction: Double) async {
        guard let library else { return }
        let chapter = UInt32(max(0, chapter))
        let fraction = Float(min(1, max(0, fraction)))
        try? await Task.detached { try library.setPosition(id: id, chapter: chapter, fraction: fraction) }.value
        await refresh()
    }

    private func open() throws -> Library {
        guard let library else { throw Problem.closed(problem) }
        return library
    }

    /// Kept reading first, then the newest added.
    nonisolated static func ordered(_ items: [Item]) -> [Item] {
        items.sorted { max($0.opened, $0.added) > max($1.opened, $1.added) }
    }

    nonisolated static func address(_ link: String) throws -> URL {
        let trimmed = link.trimmingCharacters(in: .whitespacesAndNewlines)
        let written = trimmed.contains("://") ? trimmed : "https://" + trimmed
        guard let url = URL(string: written), let scheme = url.scheme?.lowercased(),
              scheme == "https" || scheme == "http", let host = url.host(), host.contains(".") else {
            throw Problem.badLink
        }
        return url
    }

    /// What went wrong, in words the reader can be shown.
    public nonisolated static func describe(_ error: any Error) -> String {
        switch error {
        case let error as InklingError:
            switch error {
            case .Storage(let message), .Import(let message): return message
            case .NotFound: return "This is no longer in the library."
            }
        case let problem as Problem:
            return problem.message
        case let error as URLError:
            return "The page couldn’t be fetched: \(error.localizedDescription.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".")))."
        default:
            return error.localizedDescription
        }
    }

    enum Problem: Error {
        case badLink
        case status(Int)
        case unreadable
        case closed(String?)

        var message: String {
            switch self {
            case .badLink: "That doesn’t look like a web address."
            case .status(let code): "The page answered with an error (\(code))."
            case .unreadable: "The page couldn’t be read."
            case .closed(let why): why ?? "The library couldn’t be opened."
            }
        }
    }
}

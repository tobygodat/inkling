import Foundation
import InklingCore

/// Fills an empty library for the snapshot tests and the renders: the articles in `apple/Demo`,
/// saved from their pages offline, and the fixture books `books/fetch-fixtures.sh` downloads.
/// Only a launch with `INKLING_DEMO` set, always on a temp data folder, comes here.
enum Demo {
    private struct Article: Decodable {
        let url: String
        let file: String
        var fraction: Float?
        var archived: Bool?
    }

    private struct Shelf: Decodable {
        let articles: [Article]
        let books: [String: Float]
    }

    private final class Offline: Fetcher {
        func fetch(url: String) -> Data? { nil }
    }

    static func seed(_ library: Library, from folder: URL, books: URL?) {
        guard let data = try? Data(contentsOf: folder.appending(path: "demo.json")),
              let shelf = try? JSONDecoder().decode(Shelf.self, from: data) else { return }
        for (name, fraction) in shelf.books.sorted(by: { $0.key < $1.key }) {
            guard let books, let item = try? library.addFile(path: books.appending(path: name).path), fraction > 0 else { continue }
            let through = Float(item.chapters) * fraction
            let chapter = min(UInt32(through), max(item.chapters, 1) - 1)
            try? library.setPosition(id: item.id, chapter: chapter, fraction: through - Float(chapter))
        }
        for article in shelf.articles.reversed() {
            guard let html = try? String(contentsOf: folder.appending(path: article.file), encoding: .utf8),
                  let item = try? library.saveArticle(url: article.url, html: html, fetcher: Offline()) else { continue }
            if let fraction = article.fraction { try? library.setPosition(id: item.id, chapter: 0, fraction: fraction) }
            if article.archived == true { try? library.setArchived(id: item.id, archived: true) }
        }
    }
}

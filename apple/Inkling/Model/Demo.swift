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

    /// Serves the demo's pictures from `pictures/` for `https://demo.inkling/<name>`, and nothing else,
    /// so seeding never reaches the network.
    private final class Pictures: Fetcher {
        let folder: URL
        init(_ folder: URL) { self.folder = folder }

        func fetch(url: String) -> Data? {
            guard let url = URL(string: url), url.host() == "demo.inkling" else { return nil }
            return try? Data(contentsOf: folder.appending(path: "pictures").appending(path: url.lastPathComponent))
        }
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
                  let item = try? library.saveArticle(url: article.url, html: html, fetcher: Pictures(folder)) else { continue }
            if let fraction = article.fraction { try? library.setPosition(id: item.id, chapter: 0, fraction: fraction) }
            if article.archived == true { try? library.setArchived(id: item.id, archived: true) }
        }
    }

    /// Seeds a few Markdown documents for the native writing page's snapshots and taste renders.
    /// Only runs when `INKLING_DEMO` is set on a temp data folder.
    static func seedDocuments(_ store: DocumentStore) {
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        let day: UInt64 = 86_400_000
        let samples: [(id: String, offset: UInt64, text: String)] = [
            (
                "quiet-desk",
                0,
                """
                # On Keeping a Quiet Desk

                A good desk holds almost nothing. Morning light crosses teh grain of the oak while the kettle settles in the next room, and i dont want a single chrome bar fighting the sentence I am trying to finish.

                We read **for rhythm** as much as for argument, and we write *for clarity* in the exact same posture. Even a short note in `plain.md` deserves the quiet of a printed page, wich holds a [small note on craft](https://example.com/craft) tucked neatly into the margin.

                ## Notes from the morning

                > Good tools disappear while you work and return only when you ask them to look closely at a draft.

                - Clear the table before the first paragraph
                - Keep one warm lamp over the paper
                - Leave every change for the writer to accept or refuse

                ```swift
                let prose = Draft.open("quiet-desk.md")
                prose.review(oneChangeAtATime: true)
                ```
                """
            ),
            (
                "coast-notes",
                day,
                """
                # Field Notes from the Coast

                The tide stayed low past noon, leaving shallow pools between the basalt shelves where the sky sat still as glass.

                ## Harbor log

                - East pier cleared at 07:30
                - Two trawlers waiting on the outer swell
                """
            ),
            (
                "winter-reading",
                day * 4,
                """
                # Reading List for Winter

                Essays and notebooks to revisit once the evenings draw in early.

                1. *Walden* — Henry David Thoreau
                2. *Virginibus Puerisque* — Robert Louis Stevenson
                3. *Self-Reliance* — Ralph Waldo Emerson
                """
            ),
        ]
        for sample in samples {
            try? store.restore(id: sample.id, text: sample.text, changedMs: now - sample.offset)
        }
        try? store.setLastOpen(id: "quiet-desk")
    }
}

import Foundation
import Testing

@testable import InklingCore

/// Leaves every picture out, so the test never touches the network.
final class NoPictures: Fetcher {
    func fetch(url: String) -> Data? { nil }
}

/// The bridge end to end, from Swift: Rust parses, stores and reads back what Swift handed it.
@Test func anArticleSavedFromSwiftReadsBackWithItsPlaceAndHighlights() throws {
    let data = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: data) }
    let library = try Library.open(data: data.path)
    #expect(library.items().isEmpty)

    let words = String(repeating: "The tide came in slowly over the flats, and the birds lifted as it reached them. ", count: 8)
    let html = "<html><head><title>Notes on the tide</title></head><body><nav>Home</nav><article><h1>Notes on the tide</h1><p>\(words)</p><p>\(words)</p></article></body></html>"
    let item = try library.saveArticle(url: "https://shore.example/tide", html: html, fetcher: NoPictures())
    #expect(item.kind == .article)
    #expect(item.title == "Notes on the tide")

    let book = try library.book(id: item.id)
    #expect(book.source?.url == "https://shore.example/tide")
    #expect(book.chapters.first?.text.hasPrefix("# Notes on the tide\n") == true)

    try library.setPosition(id: item.id, chapter: 0, fraction: 0.5)
    let passage = Passage(chapter: 0, start: 2, end: 19, words: "Notes on the tide", made: 1)
    try library.setHighlights(id: item.id, all: [passage])

    let reopened = try Library.open(data: data.path)
    #expect(reopened.items().first?.fraction == 0.5)
    #expect(reopened.highlights(id: item.id) == [passage])
    #expect(throws: InklingError.self) { try reopened.book(id: "nope") }
}

@Test func documentsAndInlineReviewRoundTripFromSwift() throws {
    let data = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: data) }
    let docs = try DocumentStore.open(data: data.path)
    #expect(docs.list().isEmpty)

    let id = docs.newId()
    let draft = "# Morning desk\n\nWe walked to teh quiet pier and i dont think anyone saw"
    try docs.write(id: id, text: draft)
    try docs.setLastOpen(id: id)

    let reopened = try DocumentStore.open(data: data.path)
    #expect(reopened.lastOpen() == id)
    #expect(reopened.list().first?.title == "Morning desk")
    #expect(try reopened.read(id: id) == draft)

    let colors = Looks.open(data: data.path).colors(tone: .light)
    #expect(colors.addedText == 0x116329)
    #expect(colors.removedFill == 0xFFEBE9)
}

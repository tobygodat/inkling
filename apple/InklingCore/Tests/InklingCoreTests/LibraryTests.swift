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

@Test func syncRoundTripsDocumentsArticlesFeedsAndReadwiseFromSwift() throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "inkling-sync-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }

    let serverFile = root.appending(path: "fake-server.json").path
    let macDir = root.appending(path: "mac").path
    let phoneDir = root.appending(path: "phone").path

    let macLib = try Library.open(data: macDir)
    let macDocs = try DocumentStore.open(data: macDir)
    let macSync = SyncClient.openFake(data: macDir, deviceKind: "mac", serverFile: serverFile)

    let phoneLib = try Library.open(data: phoneDir)
    let phoneDocs = try DocumentStore.open(data: phoneDir)
    let phoneSync = SyncClient.openFake(data: phoneDir, deviceKind: "phone", serverFile: serverFile)

    for client in [macSync, phoneSync] {
        let session = try client.beginSignIn(redirectUri: "inkling://callback", clientName: "Swift Test")
        let callback = try #require(client.fakeCallbackUrl(authorizeUrl: session.url()))
        try client.finishSignIn(session: session, callbackUrl: callback)
        #expect(client.isSignedIn())
    }

    let words = String(repeating: "The tide came in slowly over the flats, and the birds lifted as it reached them. ", count: 8)
    let feedHtml = "<!doctype html><html><head><title>Feed Dispatch</title><meta property=\"og:site_name\" content=\"Shoreline\"></head><body><article><h1>Feed Dispatch</h1><p>\(words)</p></article></body></html>"
    _ = stageFakeIncoming(serverFile: serverFile, source: "feed", destination: "inbox", url: "https://shore.example/feed-post", title: "Feed Dispatch", html: feedHtml)

    let docId = macDocs.newId()
    try macDocs.write(id: docId, text: "# Cross-device draft\n\nSent from Mac.")

    let macPending = try macSync.fetch()
    let macReport = try macSync.apply(pending: macPending, library: macLib, documents: macDocs, openDoc: nil, heldDocs: [])
    #expect(macReport.importedCount == 1)
    let inboxItem = try #require(macLib.items().first { $0.title == "Feed Dispatch" })
    #expect(inboxItem.inbox)

    // Second sync pushes the parsed article up to the server so Phone gets it.
    let pushPending = try macSync.fetch()
    _ = try macSync.apply(pending: pushPending, library: macLib, documents: macDocs, openDoc: nil, heldDocs: [])

    let phonePending = try phoneSync.fetch()
    let phoneReport = try phoneSync.apply(pending: phonePending, library: phoneLib, documents: phoneDocs, openDoc: nil, heldDocs: [])
    #expect(phoneReport.touchedDocs == [docId])
    #expect(try phoneDocs.read(id: docId) == "# Cross-device draft\n\nSent from Mac.")
    #expect(phoneLib.items().first { $0.title == "Feed Dispatch" }?.inbox == true)

    let feed = try macSync.followFeed(url: "https://daringfireball.net/feeds/main")
    #expect(try macSync.feeds().map(\.id) == [feed.id])
    try macSync.unfollowFeed(id: feed.id)
    #expect(try macSync.feeds().isEmpty)

    #expect(try macSync.connectReadwise(token: "rw-swift-token") == true)
    #expect(try macSync.disconnectReadwise() == false)
    try macSync.signOut()
    #expect(!macSync.isSignedIn())
}

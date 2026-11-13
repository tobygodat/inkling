import UIKit
import UniformTypeIdentifiers

/// The host's side of Inkling's library on iPhone. The Rust library (`ios/src/lib.rs`) asks for
/// book files, for one more folder to search, and for the folders to search; the shared views do
/// the rest. Ported from the reader's `BookFolders.swift` (TOB-460).
@MainActor
final class Books: NSObject, UIDocumentPickerDelegate {
    static let shared = Books()

    /// The controller the pickers are shown over.
    weak var presenter: UIViewController?
    private let folders = BookFolders()
    /// The picker on screen and the token its answer carries.
    private var open: (picker: UIDocumentPickerViewController, token: UInt64, folder: Bool)?

    /// The app's own Documents, which the Files app shows as "Inkling". Always searched.
    static var documents: URL {
        let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    /// Add: EPUBs and PDFs, several at once, copied into the app so no access has to be held.
    func pickBooks(token: UInt64) {
        let types = [UTType.epub, UTType.pdf]
        present(UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: true), token: token, folder: false, multiple: true)
    }

    /// Search another folder: one folder, remembered as a bookmark for later searches.
    func pickFolder(token: UInt64) {
        present(UIDocumentPickerViewController(forOpeningContentTypes: [.folder], asCopy: false), token: token, folder: true, multiple: false)
    }

    /// Documents, then every remembered folder that still exists, opened until the next call.
    func searchFolders() -> [URL] {
        [Self.documents] + folders.open()
    }

    private func present(_ picker: UIDocumentPickerViewController, token: UInt64, folder: Bool, multiple: Bool) {
        guard let presenter, open == nil else {
            answer(token, with: [])
            return
        }
        picker.delegate = self
        picker.allowsMultipleSelection = multiple
        open = (picker, token, folder)
        presenter.present(picker, animated: true)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let (_, token, folder) = open else { return }
        open = nil
        if folder, let url = urls.first {
            folders.add(url)
        }
        answer(token, with: urls)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        guard let (_, token, _) = open else { return }
        open = nil
        answer(token, with: [])
    }

    private func answer(_ token: UInt64, with urls: [URL]) {
        if urls.isEmpty {
            inkling_ios_picked(token, nil)
        } else {
            urls.map(\.path).joined(separator: "\n").withCString { inkling_ios_picked(token, $0) }
        }
    }
}

@_cdecl("inkling_host_pick_books")
func inklingHostPickBooks(_ token: UInt64) {
    MainActor.assumeIsolated { Books.shared.pickBooks(token: token) }
}

@_cdecl("inkling_host_pick_folder")
func inklingHostPickFolder(_ token: UInt64) {
    MainActor.assumeIsolated { Books.shared.pickFolder(token: token) }
}

/// One path per line, in a `malloc`ed string the Rust side frees.
@_cdecl("inkling_host_book_folders")
func inklingHostBookFolders() -> UnsafeMutablePointer<CChar>? {
    let paths = MainActor.assumeIsolated { Books.shared.searchFolders().map(\.path) }
    return strdup(paths.joined(separator: "\n"))
}

/// The folders the reader allowed Inkling to search, remembered across launches as bookmarks in
/// `book-folders.plist` beside the data folder. iOS lets the app read a folder, and the books
/// chosen from it, only while its access is held, so `open` holds it until the next search.
/// iCloud files not yet downloaded are hidden placeholders (`.name.icloud`), which the search
/// passes over like any hidden file.
@MainActor
final class BookFolders {
    private let file = Launch.dataDirectory().appendingPathComponent("book-folders.plist")
    private var bookmarks: [Data]
    private var held: [URL] = []

    init() {
        let saved = (try? Data(contentsOf: file)).flatMap { try? PropertyListDecoder().decode([Data].self, from: $0) }
        bookmarks = saved ?? []
    }

    /// Remembers a folder the picker returned, unless it is already remembered.
    func add(_ folder: URL) {
        let scoped = folder.startAccessingSecurityScopedResource()
        defer { if scoped { folder.stopAccessingSecurityScopedResource() } }
        guard let bookmark = try? folder.bookmarkData(options: .minimalBookmark) else { return }
        let known = bookmarks.compactMap(Self.resolve).map { $0.url.standardizedFileURL.path }
        guard !known.contains(folder.standardizedFileURL.path) else { return }
        bookmarks.append(bookmark)
        save()
    }

    /// Opens every folder that still exists and returns them, holding access until the next call.
    /// New access starts before the old stops, so an import still reading from a folder goes on.
    func open() -> [URL] {
        var opened: [URL] = []
        var kept: [Data] = []
        for bookmark in bookmarks {
            guard let (url, stale) = Self.resolve(bookmark), url.startAccessingSecurityScopedResource() else { continue }
            opened.append(url)
            kept.append(stale ? (try? url.bookmarkData(options: .minimalBookmark)) ?? bookmark : bookmark)
        }
        held.forEach { $0.stopAccessingSecurityScopedResource() }
        held = opened
        if kept != bookmarks {
            bookmarks = kept
            save()
        }
        return opened
    }

    private func save() {
        try? PropertyListEncoder().encode(bookmarks).write(to: file, options: .atomic)
    }

    private static func resolve(_ bookmark: Data) -> (url: URL, stale: Bool)? {
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: bookmark, bookmarkDataIsStale: &stale) else { return nil }
        return (url, stale)
    }
}

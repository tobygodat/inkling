import Foundation
import InklingCore
import Observation
import SwiftUI

/// The live state of one open document in the native writing page: its Markdown source, autosave
/// status, caret/selection in UTF-8 bytes, and any active inline review session.
@Observable
final class WritingModel {
    enum SavedState: Equatable {
        case saved
        case editing
        case failed(String)

        var label: String {
            switch self {
            case .saved: "Saved"
            case .editing: "Editing"
            case .failed(let message): "Couldn’t save (\(message))"
            }
        }
    }

    let id: String
    private let shelf: Shelf
    /// Stored off the observation graph so keystrokes never trigger SwiftUI body rebuilds; the
    /// TextKit 2 view owns the live text and syncs on autosave, review, or close.
    @ObservationIgnored private(set) var text: String
    private(set) var title: String
    private(set) var saved: SavedState = .saved
    private(set) var wordCount: Int
    /// Incremented whenever `text` changes from outside the editor (e.g. finishing a review), so
    /// the TextKit 2 text view knows to reload without echoing ordinary keystrokes back.
    private(set) var externalRevision: Int = 0
    /// The current caret or selection as UTF-8 byte offsets into `text`, read on demand.
    @ObservationIgnored var selectionBytes: Range<Int> = 0..<0
    /// Attached by `WritingTextView` so `WritingModel` pulls the text and selection only on
    /// autosave or AI ask, keeping the keystroke path O(1) with zero 35 KB string bridging.
    @ObservationIgnored var readEditorText: (() -> String)?
    @ObservationIgnored var readEditorSelection: (() -> NSRange)?

    // Inline review state (backed by `inkling_core::Revision` through `ReviewSession`).
    private(set) var versionNumber: Int = 1
    private(set) var snapshot: RevisionSnapshot?
    var selectedChangeId: UInt32?
    var notice: String?
    var missingKeyProvider: String?

    @ObservationIgnored private var saveTask: Task<Void, Never>?
    @ObservationIgnored private var wordCountTask: Task<Void, Never>?
    @ObservationIgnored private var session: ReviewSession?

    init(id: String, text: String, shelf: Shelf) {
        self.id = id
        self.text = text
        self.shelf = shelf
        let initialTitle = documentTitle(text: text)
        self.title = initialTitle.isEmpty ? "Untitled" : initialTitle
        self.wordCount = Self.countWords(text)
    }

    var isAsking: Bool {
        if case .asking = snapshot?.state { return true }
        return false
    }

    var isReviewing: Bool {
        if case .reviewing = snapshot?.state { return true }
        return false
    }

    var nextPendingId: UInt32? {
        session?.nextPending(after: nil)
    }

    /// O(1) hot-path notification from TextKit 2 on every keystroke. Avoids bridging the whole
    /// document string or triggering SwiftUI re-renders while typing.
    func markEdited() {
        if saved != .editing {
            saved = .editing
        }
        if notice != nil {
            notice = nil
        }
        scheduleAutosave()
        scheduleWordCount()
    }

    /// Programmatic text update (used by unit tests and external callers without a live text view).
    func textDidChange(_ newText: String) {
        guard newText != text else { return }
        text = newText
        let t = documentTitle(text: newText)
        let resolved = t.isEmpty ? "Untitled" : t
        if title != resolved { title = resolved }
        markEdited()
    }

    /// Pulls the latest text and selection from the live TextKit 2 editor, if attached.
    func syncFromEditor() {
        if let read = readEditorText {
            text = read()
        }
        if let readSel = readEditorSelection {
            selectionBytes = TextOffsets.utf8Range(for: readSel(), in: text)
        }
        let t = documentTitle(text: text)
        let resolved = t.isEmpty ? "Untitled" : t
        if title != resolved { title = resolved }
    }

    /// Writes pending edits immediately (on close, document switch, or app backgrounding).
    func saveNow(triggerSync: Bool = true) {
        saveTask?.cancel()
        saveTask = nil
        guard saved != .saved else { return }
        syncFromEditor()
        do {
            try shelf.saveDocument(id: id, text: text, triggerSync: triggerSync)
            saved = .saved
        } catch {
            saved = .failed(Shelf.words(error))
        }
    }

    /// Replaces the open document's text when sync brought a newer version from another device.
    func reloadFromSync(_ newText: String) {
        guard saved == .saved, !isAsking, !isReviewing, newText != text else { return }
        text = newText
        let t = documentTitle(text: newText)
        title = t.isEmpty ? "Untitled" : t
        wordCount = Self.countWords(newText)
        externalRevision += 1
    }

    /// Replaces `text` programmatically (e.g. applying accepted review edits) and saves it.
    func applyExternalEdits(_ edits: [TextEdit]) {
        guard !edits.isEmpty else { return }
        var bytes = Array(text.utf8)
        for edit in edits {
            let start = min(Int(edit.start), bytes.count)
            let end = min(max(Int(edit.end), start), bytes.count)
            bytes.replaceSubrange(start..<end, with: edit.text.utf8)
        }
        text = String(decoding: bytes, as: UTF8.self)
        let t = documentTitle(text: text)
        title = t.isEmpty ? "Untitled" : t
        wordCount = Self.countWords(text)
        externalRevision += 1
        saved = .editing
        do {
            try shelf.saveDocument(id: id, text: text)
            saved = .saved
        } catch {
            saved = .failed(Shelf.words(error))
        }
    }

    // MARK: - Inline Review (backed by core's Revision & Review)

    func askFixGrammar() { startAsk(.fixGrammar) }

    func askFindWord() {
        syncFromEditor()
        let range = selectionBytes
        startAsk(.findWord(start: UInt64(range.lowerBound), end: UInt64(range.upperBound)))
    }

    func askNextLine() {
        syncFromEditor()
        let at = selectionBytes.upperBound
        startAsk(.nextLine(at: UInt64(at)))
    }

    func startAsk(_ kind: AskKind) {
        syncFromEditor()
        saveNow()
        cancelReview()
        notice = nil
        missingKeyProvider = nil

        let bridge = ListenerBridge { [weak self] update in
            Task { @MainActor [weak self] in
                self?.handleUpdate(update)
            }
        }
        do {
            let newSession = try shelf.ai.startAsk(draft: text, ask: kind, listener: bridge)
            session = newSession
            versionNumber += 1
            snapshot = newSession.snapshot()
        } catch let error as InklingError {
            switch error {
            case .MissingKey(let provider):
                missingKeyProvider = provider
                notice = "Add your \(provider) API key in Settings before asking."
            case .InvalidRequest(let message):
                notice = message
            default:
                notice = Shelf.words(error)
            }
        } catch {
            notice = Shelf.words(error)
        }
    }

    func cancelReview() {
        session?.cancel()
        session = nil
        snapshot = nil
        selectedChangeId = nil
    }

    func acceptChange(_ id: UInt32) {
        guard let session else { return }
        let updated = session.accept(id: id)
        applyReviewSnapshot(updated, afterId: id)
    }

    func rejectChange(_ id: UInt32) {
        guard let session else { return }
        let updated = session.reject(id: id)
        applyReviewSnapshot(updated, afterId: id)
    }

    func undoChange(_ id: UInt32) {
        guard let session else { return }
        snapshot = session.undo(id: id)
        selectedChangeId = id
    }

    func chooseWording(_ id: UInt32, wording: UInt32) {
        guard let session else { return }
        snapshot = session.choose(id: id, wording: wording)
    }

    func acceptNextPending() {
        guard let session else { return }
        if let next = session.nextPending(after: nil) {
            acceptChange(next)
        } else {
            finishReview()
        }
    }

    func acceptRest() {
        guard let session else { return }
        let updated = session.acceptRest()
        applyReviewSnapshot(updated, afterId: nil)
    }

    func finishReview() {
        guard let session else { return }
        let edits = session.finish()
        self.session = nil
        snapshot = nil
        selectedChangeId = nil
        applyExternalEdits(edits)
    }

    func selectStep(_ delta: Int) {
        guard case .reviewing(_, let segments) = snapshot?.state else { return }
        let ids: [UInt32] = segments.compactMap {
            if case .change(let c) = $0 { return c.id }
            return nil
        }
        guard !ids.isEmpty else { return }
        let current = selectedChangeId.flatMap { ids.firstIndex(of: $0) } ?? 0
        let next = (current + delta % ids.count + ids.count) % ids.count
        selectedChangeId = ids[next]
    }

    private func handleUpdate(_ update: RevisionSnapshot) {
        guard session != nil else { return }
        switch update.state {
        case .reviewing(let stats, _):
            if stats.total == 0 {
                session = nil
                snapshot = nil
                selectedChangeId = nil
                notice = "No changes suggested."
                return
            }
            snapshot = update
            if selectedChangeId == nil {
                selectedChangeId = session?.nextPending(after: nil)
            }
        case .failed(let message):
            session = nil
            snapshot = nil
            selectedChangeId = nil
            notice = message
        default:
            snapshot = update
        }
    }

    private func applyReviewSnapshot(_ updated: RevisionSnapshot, afterId: UInt32?) {
        if case .reviewing(let stats, _) = updated.state, stats.pending == 0 {
            finishReview()
            return
        }
        snapshot = updated
        if let afterId {
            selectedChangeId = session?.nextPending(after: afterId) ?? afterId
        }
    }

    private func scheduleAutosave() {
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(500))
            guard !Task.isCancelled else { return }
            self?.saveNow()
        }
    }

    private func scheduleWordCount() {
        wordCountTask?.cancel()
        wordCountTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(450))
            guard !Task.isCancelled, let self else { return }
            self.syncFromEditor()
            let count = Self.countWords(self.text)
            if self.wordCount != count {
                self.wordCount = count
            }
        }
    }

    private static func countWords(_ text: String) -> Int {
        var count = 0
        var inWord = false
        for byte in text.utf8 {
            let space = byte == 0x20 || byte == 0x0A || byte == 0x0D || byte == 0x09
            if space {
                inWord = false
            } else if !inWord {
                inWord = true
                count += 1
            }
        }
        return count
    }
}

/// Bridges UniFFI's `RevisionListener` callback off the main thread into `WritingModel`.
private final class ListenerBridge: RevisionListener, @unchecked Sendable {
    private let onUpdateHandler: @Sendable (RevisionSnapshot) -> Void

    init(_ onUpdate: @escaping @Sendable (RevisionSnapshot) -> Void) {
        self.onUpdateHandler = onUpdate
    }

    func onUpdate(snapshot: RevisionSnapshot) {
        onUpdateHandler(snapshot)
    }
}

/// UTF-16 `NSRange` <-> UTF-8 byte `Range<Int>` conversion for `core::Ask`.
nonisolated enum TextOffsets {
    static func utf8Range(for utf16Range: NSRange, in text: String) -> Range<Int> {
        let utf16 = text.utf16
        let startOffset = min(max(utf16Range.location, 0), utf16.count)
        let endOffset = min(max(NSMaxRange(utf16Range), startOffset), utf16.count)
        let startIdx = utf16.index(utf16.startIndex, offsetBy: startOffset)
        let endIdx = utf16.index(utf16.startIndex, offsetBy: endOffset)
        let startByte = text.utf8.distance(from: text.startIndex, to: startIdx)
        let endByte = text.utf8.distance(from: text.startIndex, to: endIdx)
        return startByte..<endByte
    }
}

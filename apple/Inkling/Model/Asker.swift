import Foundation
import InklingCore
import Observation

/// What the Ask bar is asked about: the screen on show.
enum AskPlace {
    case screen(Section)
    case reading(ReaderModel)
    case writing(WritingModel)
}

/// What the + sheet offers to do with the screen, besides attaching.
enum AskSuggestion: String, Identifiable, CaseIterable {
    case fixGrammar, findWord, nextLine, explain, summarize

    var id: Self { self }

    var title: String {
        switch self {
        case .fixGrammar: "Fix grammar"
        case .findWord: "Find a word"
        case .nextLine: "Next line"
        case .explain: "Explain"
        case .summarize: "Summarize"
        }
    }

    var symbol: String {
        switch self {
        case .fixGrammar: "checkmark.seal"
        case .findWord: "character.textbox"
        case .nextLine: "text.append"
        case .explain: "lightbulb"
        case .summarize: "text.line.first.and.arrowtriangle.forward"
        }
    }
}

/// The Ask bar on every screen and its one conversation (TOB-559). A question goes to the
/// provider with what is on screen and the passages of the books it names, and its answer cites
/// them. In Writing, an instruction to change the draft goes to the inline review instead. Every
/// turn is kept on disk through core's `Conversation`, so it carries across tabs and launches.
@Observable
final class Asker {
    @ObservationIgnored weak var shelf: Shelf?
    @ObservationIgnored private let conversation: Conversation
    private(set) var turns: [Turn]
    /// The turn on its way: what was asked, where, and the answer so far.
    private(set) var pending: Turn?
    /// Why the last question got no answer, in words to show.
    private(set) var problem: String?
    private(set) var missingKey = false
    var draft = ""
    /// The composer is open: a sheet on iPhone, the bar grown upward on the Mac.
    var composing = false
    /// The + sheet is open.
    var adding = false
    var choosingProvider = false
    var place: AskPlace = .screen(.inbox) {
        didSet { selected = nil }
    }
    /// Books and articles attached from the + sheet, whose passages the question draws on.
    var attached: [Item] = []
    /// Whether the selection on screen goes with the question.
    var withSelection = true
    @ObservationIgnored private var session: AnswerSession?

    init(folder: URL) {
        conversation = Conversation.open(data: folder.path)
        turns = conversation.turns()
    }

    var busy: Bool { pending != nil }

    /// Where a question is asked from, as its turn shows it.
    var label: String {
        switch place {
        case .screen(let section): section.title
        case .reading(let model):
            model.chapters > 1 ? "\(model.item.title) · \(model.rendered.titles[model.chapter])" : model.item.title
        case .writing(let model): model.title
        }
    }

    /// The bar's own words: what it would be asked about.
    var prompt: String {
        switch place {
        case .screen: "Ask"
        case .reading(let model): "Ask about \(model.item.title)"
        case .writing: "Ask about this draft"
        }
    }

    /// The words selected on screen when the composer or the + sheet opened.
    private(set) var selected: String?

    func openComposer() {
        selected = readSelection()
        composing = true
    }

    func openAdd() {
        if !composing { selected = readSelection() }
        adding = true
    }

    private func readSelection() -> String? {
        let words: String
        switch place {
        case .screen: return nil
        case .reading(let model):
            guard model.selection.length > 0, NSMaxRange(model.selection) <= model.rendered.text.length else { return nil }
            words = (model.rendered.text.string as NSString).substring(with: model.selection)
        case .writing(let model):
            model.syncFromEditor()
            let range = model.selectionBytes
            guard !range.isEmpty else { return nil }
            let bytes = Array(model.text.utf8)
            guard range.upperBound <= bytes.count else { return nil }
            words = String(decoding: bytes[range], as: UTF8.self)
        }
        let trimmed = words.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    var suggestions: [AskSuggestion] {
        switch place {
        case .screen: []
        case .reading: [.explain, .summarize]
        case .writing: [.fixGrammar, .findWord, .nextLine]
        }
    }

    /// The words the question is about: the selection when it goes along, else what is on
    /// screen: the passage at the reader's place, or the draft.
    private var about: AskAbout? {
        if withSelection, let selected {
            return AskAbout(label: label, text: String(selected.prefix(4000)))
        }
        switch place {
        case .screen: return nil
        case .reading(let model):
            let text = model.rendered.text.string as NSString
            let start = model.character(chapter: model.chapter, fraction: model.fraction)
            let length = min(1200, max(0, text.length - start))
            return AskAbout(label: label, text: text.substring(with: NSRange(location: start, length: length)))
        case .writing(let model):
            model.syncFromEditor()
            return AskAbout(label: label, text: String(model.text.prefix(8000)))
        }
    }

    func send() {
        let asked = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !asked.isEmpty, !busy else { return }
        draft = ""
        ask(asked)
    }

    func run(_ suggestion: AskSuggestion) {
        adding = false
        guard case .writing(let writer) = place else {
            composing = true
            ask(suggestion == .explain ? "Explain this passage." : "Summarize this passage.")
            return
        }
        switch suggestion {
        case .fixGrammar: writer.askFixGrammar()
        case .findWord: writer.askFindWord()
        default: writer.askNextLine()
        }
        recordEdit(suggestion.title, in: writer)
    }

    func ask(_ asked: String) {
        guard let shelf else { return }
        problem = nil
        missingKey = false
        if case .writing(let writer) = place, shelf.library.wantsEdit(asked: asked) {
            writer.startAsk(.freeform(instruction: asked))
            recordEdit(asked, in: writer)
            return
        }
        let about = about
        let attached = attached.map(\.id)
        self.attached = []
        let library = shelf.library
        let ai = shelf.ai
        pending = Turn(asked: asked, context: label, answer: "", citations: [], edit: false, made: Self.now())
        Task { [weak self] in
            let sources = (try? await Shelf.offMain { library.sources(asked: asked, about: about?.text, attached: attached) }) ?? []
            let bridge = AnswerBridge { [weak self] state in
                Task { @MainActor [weak self] in self?.update(state) }
            }
            guard let self else { return }
            do {
                self.session = try ai.startQuestion(asked: asked, about: about, sources: sources, listener: bridge)
            } catch {
                self.fail(error)
            }
        }
    }

    func cancel() {
        session?.cancel()
        session = nil
        pending = nil
    }

    /// Starts the conversation over, on disk too.
    func clear() {
        cancel()
        try? conversation.clear()
        turns = conversation.turns()
    }

    private func update(_ state: AnswerState) {
        guard var turn = pending else { return }
        switch state {
        case .asking(let text):
            turn.answer = text
            pending = turn
        case .answered(let text, let citations):
            turn.answer = text
            turn.citations = citations
            keep(turn)
        case .failed(let message):
            session = nil
            pending = nil
            problem = message
        }
    }

    private func fail(_ error: any Error) {
        session = nil
        pending = nil
        if case InklingError.MissingKey = error { missingKey = true }
        problem = Shelf.words(error)
    }

    /// An edit goes to the inline review; the turn says where it went.
    private func recordEdit(_ asked: String, in writer: WritingModel) {
        let answer = writer.notice ?? "Version \(writer.versionNumber) is in the draft to review."
        if writer.missingKeyProvider != nil { missingKey = true }
        composing = false
        keep(Turn(asked: asked, context: label, answer: answer, citations: [], edit: true, made: Self.now()))
    }

    private func keep(_ turn: Turn) {
        session = nil
        pending = nil
        do {
            try conversation.add(turn: turn)
        } catch {
            problem = Shelf.words(error)
        }
        turns = conversation.turns()
    }

    private static func now() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1000)
    }
}

/// Bridges UniFFI's `AnswerListener` callback off the main thread into `Asker`.
private final class AnswerBridge: AnswerListener, @unchecked Sendable {
    private let handler: @Sendable (AnswerState) -> Void

    init(_ handler: @escaping @Sendable (AnswerState) -> Void) {
        self.handler = handler
    }

    func onAnswer(state: AnswerState) {
        handler(state)
    }
}

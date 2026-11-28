import AppKit
import InklingCore
import SwiftUI
import Testing

/// The Mac app's screens rendered offscreen, in Light and Dark, into `INKLING_SNAP_OUT`, with the
/// behaviour they show asserted on the way. Nothing is put on screen and nothing takes focus.
@Suite(.serialized, .enabled(if: ProcessInfo.processInfo.environment["INKLING_DEMO"] != nil, "apple/snap.sh sets INKLING_DEMO and INKLING_FIXTURES"))
final class MacSnapshots {
    private let out: URL
    private let shelf: Shelf
    private let data: URL

    init() throws {
        let environment = ProcessInfo.processInfo.environment
        let demo = environment["INKLING_DEMO"] ?? ""
        let fixtures = environment["INKLING_FIXTURES"] ?? ""
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        Fonts.register()
        out = URL(filePath: environment["INKLING_SNAP_OUT"] ?? NSTemporaryDirectory()).appending(path: "mac")
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        data = FileManager.default.temporaryDirectory.appending(path: "inkling-snap-\(UUID().uuidString)")
        shelf = try Shelf(folder: data)
        Demo.seed(shelf.library, from: URL(filePath: demo), books: URL(filePath: fixtures))
        Demo.seedDocuments(shelf.documents)
        shelf.reload()
        shelf.reloadDocuments()
    }

    deinit {
        try? FileManager.default.removeItem(at: data)
    }

    @Test func everyScreenInLightAndDark() async throws {
        #expect(shelf.inbox.map(\.title) == ["Walking", "An Apology for Idlers", "Self-Reliance"], "the Inbox is the articles still to read, newest first")
        #expect(shelf.inbox(.saved) == shelf.inbox && shelf.inbox(.feeds).isEmpty, "a saved link is Saved, not from a feed")
        #expect(shelf.archive.map(\.title) == ["Resistance to Civil Government"])
        #expect(shelf.books.count == 3)
        let walking = try #require(shelf.inbox.first)
        #expect(walking.picture.map { FileManager.default.fileExists(atPath: $0) } == true, "an article's first picture is its row's")
        #expect(walking.excerpt?.isEmpty == false)
        let selfReliance = try #require(shelf.inbox.first { $0.title == "Self-Reliance" })
        #expect(selfReliance.picture == nil, "an article with no lead picture has none")
        #expect(selfReliance.icon.map { FileManager.default.fileExists(atPath: $0) } == true, "an article without a lead picture keeps the site's icon for its cover")
        let walden = try #require(shelf.books.first { $0.title == "Walden" })
        #expect(walden.progress == 0, "a book not yet read opens at its start")

        #expect(shelf.look.density == .compact, "tight two-line cards are the default")
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            for section in Section.sidebar {
                let navigator = Navigator()
                navigator.section = section
                if section == .inbox { navigator.selection = shelf.inbox[0].id }
                if section == .library { navigator.selection = shelf.books.first { $0.title.hasPrefix(name == "light" ? "Walden" : "Pride") }?.id }
                if section == .writing { navigator.document = shelf.docs.first?.id }
                let captured = try await shoot(root(navigator), "\(name)-\(section.rawValue)", appearance)
                if section == .inbox {
                    #expect(Self.colouredShare(in: captured, region: CGRect(x: 0.52, y: 0.62, width: 0.40, height: 0.32)) > 0.2, "Walking's painting draws in the \(name) reader")
                }
            }
            try await shoot(SaveLinkSheet().environment(shelf).environment(\.palette, palette(appearance)), "\(name)-save-link", appearance, size: CGSize(width: 460, height: 150), titled: false)
            try await shoot(SettingsView(initialTab: .reading).environment(shelf).environment(Navigator()), "\(name)-settings", appearance, size: CGSize(width: 660, height: 420))
            try await shoot(SettingsView(initialTab: .profile).environment(shelf).environment(Navigator()), "\(name)-settings-profile", appearance, size: CGSize(width: 660, height: 460))
            for (pushed, view) in [("archive", AnyView(ArchiveView())), ("highlights", AnyView(HighlightsView()))] {
                let page = NavigationStack { view }
                    .frame(width: 640, height: 400)
                    .environment(shelf)
                    .environment(Navigator())
                    .environment(\.palette, palette(appearance))
                try await shoot(page, "\(name)-settings-\(pushed)", appearance, size: CGSize(width: 660, height: 460))
            }
        }
        var comfortable = shelf.look
        comfortable.density = .comfortable
        shelf.setLook(comfortable)
        #expect(try Shelf(folder: data).look.density == .comfortable, "list density is kept with appearance settings")
        do {
            let navigator = Navigator()
            navigator.section = .inbox
            navigator.selection = shelf.inbox[0].id
            try await shoot(root(navigator), "light-inbox-comfortable", .aqua)
        }
        comfortable.density = .compact
        shelf.setLook(comfortable)
    }

    @Test func theAskBarFoldsWhileThePageScrollsOn() async throws {
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.section = .inbox
            navigator.selection = shelf.inbox[0].id
            try await shoot(root(navigator), "\(name)-ask-folded", appearance) { window in
                let reader = try #require(Self.scrollViews(in: window.contentView).first { $0.documentView is NSTextView }, "the reader scrolls")
                @MainActor func scroll(by dy: CGFloat) async throws {
                    let clip = reader.contentView
                    clip.scroll(to: CGPoint(x: 0, y: max(0, clip.bounds.origin.y + dy)))
                    reader.reflectScrolledClipView(clip)
                    try await Task.sleep(for: .milliseconds(500))
                }
                try await scroll(by: 1)
                #expect(!navigator.askFolded)
                try await scroll(by: 600)
                #expect(navigator.askFolded, "reading on folds the bar")
                try await scroll(by: -200)
                #expect(!navigator.askFolded, "scrolling back opens it out")
                try await scroll(by: 300)
            }
            #expect(navigator.askFolded)
        }
    }

    private static func scrollViews(in view: NSView?) -> [NSScrollView] {
        guard let view else { return [] }
        let own = (view as? NSScrollView).map { [$0] } ?? []
        return own + view.subviews.flatMap { scrollViews(in: $0) }
    }

    @Test func aCancelledQuestionNeverAnswersTheNextOne() async throws {
        let asker = shelf.asker
        let idlers = try #require(shelf.inbox.first { $0.title == "An Apology for Idlers" })
        let walking = try #require(shelf.inbox.first { $0.title == "Walking" })
        asker.attached = [idlers]
        asker.ask("Is idleness a virtue?")
        asker.cancel()
        asker.attached = [walking]
        asker.ask("Why walk in the woods?")
        for _ in 0..<250 where asker.busy { try await Task.sleep(for: .milliseconds(20)) }
        try await Task.sleep(for: .milliseconds(300))
        let turn = try #require(asker.turns.last)
        #expect(asker.turns.count == 1, "the cancelled question keeps no turn")
        #expect(turn.asked == "Why walk in the woods?")
        #expect(turn.answer.hasPrefix("Walking"), "the answer is the second question's, not the cancelled one's: \(turn.answer)")
    }

    @Test func aLookIsKeptAndRedrawsTheReader() async throws {
        var look = shelf.look
        look.tone = .sepia
        look.face = "Charter"
        look.textSize = 40
        look.lineSpacing = 160
        shelf.setLook(look)
        #expect(shelf.look.textSize == 32, "core clamps the size")
        #expect(try Shelf(folder: data).look == shelf.look, "the look is kept through a relaunch")

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.selection = shelf.inbox[1].id
            try await shoot(root(navigator), "\(name)-reader-sepia", appearance)
            try await shoot(AppearancePanel().environment(shelf), "\(name)-appearance", appearance, size: CGSize(width: 380, height: 300), titled: false)
        }
    }

    @Test func archivingHighlightsAndThePlaceOutliveARelaunch() async throws {
        let idlers = try #require(shelf.inbox.first { $0.title == "An Apology for Idlers" })
        shelf.setArchived(idlers, true)
        #expect(shelf.archive.first?.id == idlers.id, "the newest archived comes first")
        shelf.setArchived(idlers, false)

        let book = try await shelf.open(idlers.id)
        let set = Typesetting(face: "Source Serif 4", size: 21, lineHeight: 1.75, titleSize: 36, palette: palette(.aqua), column: 700)
        let rendered = Renderer.render(book, set: set, folder: URL(filePath: shelf.library.folder()), byline: nil)
        let renderedText = rendered.text.string as NSString
        let doneRange = renderedText.range(of: "Done", options: .backwards)
        #expect(doneRange.location != NSNotFound, "an article ends with Done")
        #expect(rendered.text.attribute(.link, at: doneRange.location, effectiveRange: nil) as? URL == Renderer.done, "Done carries the archive link")
        let model = ReaderModel(item: idlers, rendered: rendered, shelf: shelf)
        let words = "the work-house is too good for you"
        let at = (rendered.text.string as NSString).range(of: words)
        #expect(at.location != NSNotFound)
        model.highlight(at)
        let passage = try #require(model.highlights.first)
        #expect(passage.words == words)
        let markdown = book.chapters[0].text.utf8
        let start = markdown.index(markdown.startIndex, offsetBy: Int(passage.start))
        let end = markdown.index(markdown.startIndex, offsetBy: Int(passage.end))
        #expect(String(markdown[start..<end]) == words, "the passage is kept as the bytes of its chapter's Markdown")
        model.scrolled(toCharacter: rendered.chapters[0].location + rendered.chapters[0].length / 2, atEnd: false)
        shelf.flushPlaces()

        let relaunched = try Shelf(folder: data)
        let item = try #require(relaunched.item(idlers.id))
        #expect(abs(item.fraction - 0.5) < 0.01, "the place is kept")
        let again = ReaderModel(item: item, rendered: rendered, shelf: relaunched)
        #expect(again.highlightRanges == [at], "the highlight is drawn where it was made")
        let middle = rendered.chapters[0].location + rendered.chapters[0].length / 2
        #expect(again.jump.map { abs($0 - middle) <= 1 } == true, "the reader opens at the place, to the character a stored f32 fraction keeps")
    }

    @Test func chaptersAndFootnotes() async throws {
        let walden = try #require(shelf.books.first { $0.title == "Walden" })
        let book = try await shelf.open(walden.id)
        let set = Typesetting(face: "Source Serif 4", size: 21, lineHeight: 1.75, titleSize: 36, palette: palette(.aqua), column: 700)
        let rendered = Renderer.render(book, set: set, folder: URL(filePath: shelf.library.folder()), byline: nil)
        let text = rendered.text.string as NSString
        #expect(rendered.titles == ["Where I Lived, and What I Lived For", "Economy"])

        #expect(!text.contains("[^") && !text.contains("\u{1}"), "no footnote mark shows its Markdown")
        let mark = text.range(of: "teach.1").location + "teach.".count
        let offset = { (at: Int) in rendered.text.attribute(.baselineOffset, at: at, effectiveRange: nil) as? CGFloat ?? 0 }
        #expect(offset(mark) > offset(mark - 1) + 5, "the mark is raised")
        #expect(rendered.text.attribute(.foregroundColor, at: mark, effectiveRange: nil) as? NSColor == set.palette.link, "the mark is in Link")
        #expect(text.range(of: "1\tWritten at the pond in the spring of 1846.").location != NSNotFound, "the note hangs from its label")

        let model = ReaderModel(item: walden, rendered: rendered, shelf: shelf)
        let markdown = book.chapters[0].text.utf8
        for words in ["Written at the pond", "see if I could not learn what it had to teach"] {
            model.highlight(text.range(of: words))
            let passage = try #require(model.highlights.first { $0.words == words })
            let start = markdown.index(markdown.startIndex, offsetBy: Int(passage.start))
            let end = markdown.index(markdown.startIndex, offsetBy: Int(passage.end))
            #expect(String(markdown[start..<end]) == words, "a passage near a mark is kept as its chapter's own bytes")
            model.remove(passage)
        }

        let next = text.range(of: "Next: Economy")
        #expect(NSLocationInRange(next.location, rendered.chapters[0]), "\"Next: …\" ends the chapter before")
        let link = try #require(rendered.text.attribute(.link, at: next.location, effectiveRange: nil) as? URL)
        #expect(Renderer.chapter(of: link) == 1)
        #expect(text.range(of: "Next:", options: [], range: rendered.chapters[1]).location == NSNotFound, "the last chapter leads nowhere")
        var archives = false
        rendered.text.enumerateAttribute(.link, in: NSRange(location: 0, length: rendered.text.length)) { value, _, _ in
            if value as? URL == Renderer.done { archives = true }
        }
        #expect(!archives, "a book ends without Done, which would archive it")
        model.go(toChapter: 1, .glide)
        #expect(model.jump == rendered.chapters[1].location && model.motion == .glide)
        model.jump = nil
        model.go(toChapter: 2, .fade)
        #expect(model.jump == nil, "there is no chapter after the last")

        #expect(ProcessInfo.processInfo.environment["INKLING_DATA_DIR"] != nil || dataFolder() == NSHomeDirectory() + "/Library/Application Support/Inkling", "the app opens the GPUI app's data folder")

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0)
            shelf.reload()
            let navigator = Navigator()
            navigator.section = .library
            navigator.selection = walden.id
            try await shoot(root(navigator), "\(name)-reader-footnotes", appearance)
            try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0.62)
            shelf.reload()
            try await shoot(root(navigator), "\(name)-reader-chapters", appearance)
            try await shoot(ContentsPanel(model: model), "\(name)-contents", appearance, size: CGSize(width: 360, height: 420), titled: false)
        }
        try shelf.library.setPosition(id: walden.id, chapter: 0, fraction: 0)
        shelf.reload()
    }

    @Test func aBookFileAndALinkAreAddedThroughTheBridge() async throws {
        let fixtures = URL(filePath: ProcessInfo.processInfo.environment["INKLING_FIXTURES"] ?? "")
        let folder = data.appending(path: "added")
        let added = try Shelf(folder: folder, session: Pages.session)
        await added.addBook(at: fixtures.appending(path: "walden.epub"))
        #expect(added.failure == nil)
        #expect(added.books.map(\.title) == ["Walden"])

        let item = try await added.save(link: "pages.test/tide")
        #expect(added.inbox(.saved).first?.id == item.id, "a saved link goes to the top of the Inbox")
        #expect(item.site == "pages.test")
        let book = try await added.open(item.id)
        let pictures = try FileManager.default.contentsOfDirectory(atPath: added.library.folder() + "/images/\(item.id)")
        #expect(pictures.count == 1, "the https picture is fetched and kept; the file: one is left out")
        #expect(book.chapters[0].text.contains("The flats at dawn"))
        await #expect(throws: SaveError.self) { try await added.save(link: "ftp://pages.test/tide") }
    }

    @Test func writingLiveMarkdownDocumentsAndInlineReview() async throws {
        // 1. Live Markdown attributes and TextKit 2 layout on a real NSTextView.
        let doc = try #require(shelf.docs.first { $0.id == "quiet-desk" })
        let draft = shelf.readDocument(doc.id)
        let style = WritingStyle.make(look: shelf.writingLook, palette: palette(.aqua), column: 700, isPhone: false)
        let styler = LiveMarkdownStyler(style: style)
        let textView = WritingNSTextView(usingTextLayoutManager: true)
        #expect(textView.textLayoutManager != nil, "writing page uses TextKit 2")
        let storage = try #require(textView.textStorage)
        storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: draft)
        styler.restyleAll(storage)

        let ns = storage.string as NSString
        // Heading: `#` is quieter than the title text, and title uses titleSize.
        let hashRange = ns.range(of: "# On Keeping")
        let h1Font = try #require(storage.attribute(.font, at: hashRange.location + 2, effectiveRange: nil) as? NSFont)
        #expect(abs(h1Font.pointSize - style.titleSize) < 0.5, "H1 uses titleSize")
        let hashColor = try #require(storage.attribute(.foregroundColor, at: hashRange.location, effectiveRange: nil) as? NSColor)
        let titleColor = try #require(storage.attribute(.foregroundColor, at: hashRange.location + 2, effectiveRange: nil) as? NSColor)
        #expect(hashColor != titleColor, "heading mark is quieter than heading text")

        // Bold, italic, inline code, link, blockquote, list, code block.
        let boldAt = ns.range(of: "for rhythm").location
        let boldFont = try #require(storage.attribute(.font, at: boldAt, effectiveRange: nil) as? NSFont)
        #expect(boldFont.fontName.contains("Semibold") || boldFont.fontDescriptor.symbolicTraits.contains(.bold))
        let italicAt = ns.range(of: "for clarity").location
        let italicFont = try #require(storage.attribute(.font, at: italicAt, effectiveRange: nil) as? NSFont)
        #expect(italicFont.fontName.contains("It") || italicFont.fontDescriptor.symbolicTraits.contains(.italic))
        let codeAt = ns.range(of: "plain.md").location
        #expect(storage.attribute(.backgroundColor, at: codeAt, effectiveRange: nil) as? NSColor == style.palette.segment)
        let linkAt = ns.range(of: "small note on craft").location
        #expect(storage.attribute(.foregroundColor, at: linkAt, effectiveRange: nil) as? NSColor == style.palette.link)
        #expect(storage.attribute(.underlineStyle, at: linkAt, effectiveRange: nil) as? Int == NSUnderlineStyle.single.rawValue)
        let quoteAt = ns.range(of: "> Good tools").location
        #expect(storage.attribute(.inklingBlock, at: quoteAt, effectiveRange: nil) as? BlockDecoration == .blockquote)
        let fenceAt = ns.range(of: "let prose").location
        if case .codeBlock = storage.attribute(.inklingBlock, at: fenceAt, effectiveRange: nil) as? BlockDecoration {} else {
            Issue.record("fenced code block carries codeBlock decoration")
        }

        // 2. Document create / edit / save / delete / undo through core's DocumentStore.
        let freshId = shelf.createDocument()
        let freshModel = WritingModel(id: freshId, text: "", shelf: shelf)
        freshModel.textDidChange("# Scratchpad\n\nSaved through core.")
        freshModel.saveNow()
        let reloaded = try Shelf(folder: data)
        #expect(reloaded.readDocument(freshId) == "# Scratchpad\n\nSaved through core.", "edits persist to core's documents folder")
        let freshInfo = try #require(shelf.docs.first { $0.id == freshId })
        shelf.deleteDocument(freshInfo)
        #expect(!shelf.docs.contains { $0.id == freshId })
        _ = shelf.undoDeleteDocument()
        #expect(shelf.docs.contains { $0.id == freshId }, "undo restores removed document")
        shelf.deleteDocument(freshInfo)

        // 3. Inline review through core's review model (with INKLING_FAKE_AI=1).
        let reviewModel = WritingModel(id: doc.id, text: draft, shelf: shelf)
        reviewModel.askFixGrammar()
        for _ in 0..<200 where !reviewModel.isReviewing {
            try await Task.sleep(for: .milliseconds(100))
        }
        #expect(reviewModel.isReviewing, "Fix grammar finishes into an inline review")
        guard case .reviewing(let stats, let segments) = reviewModel.snapshot?.state else {
            Issue.record("expected reviewing state")
            return
        }
        #expect(stats.total >= 3, "fake grammar pass finds multiple fixes")
        #expect(segments.contains { if case .change(let c) = $0 { return c.removed == "teh" && c.added == "the" }; return false })

        // Accept first change so the screenshot shows both pending (`✓`/`✕`) and decided (`✓ kept · Undo`) states.
        reviewModel.acceptChange(0)
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let view = WritingView(documentId: doc.id, initialModel: reviewModel)
                .safeAreaInset(edge: .bottom, spacing: 0) { MacAskDock() }
                .environment(shelf)
                .environment(Navigator())
            try await shoot(view, "\(name)-writing-review", appearance)
        }

        // Reject change 1, accept the rest, and confirm accepted edits land on disk.
        reviewModel.rejectChange(1)
        reviewModel.acceptRest()
        #expect(!reviewModel.isReviewing, "accepting the rest finishes the review")
        let finalDraft = try Shelf(folder: data).readDocument(doc.id)
        #expect(finalDraft.contains("crosses the grain"), "accepted change 0 is saved to disk")
        #expect(finalDraft.contains("i dont want"), "rejected change 1 stays untouched")
        #expect(finalDraft.contains("which holds"), "accept-the-rest applied remaining pending change 2")
        // Restore demo doc for any subsequent test.
        try shelf.saveDocument(id: doc.id, text: draft)
    }

    @Test func typingStaysSmoothIn5000WordDraft() async throws {
        // Same 5,000-word draft and 50 characters as ios/bench.sh.
        var paragraphs: [String] = []
        paragraphs.reserveCapacity(100)
        for _ in 0..<100 {
            let words = (0..<50).map { $0 % 9 == 4 ? "**river**" : "light" }.joined(separator: " ")
            paragraphs.append(words)
        }
        let draft = paragraphs.joined(separator: "\n\n") + "\n\n"
        let typed = "the light held over the river and the light held  "

        let style = WritingStyle.make(look: shelf.writingLook, palette: palette(.aqua), column: 700, isPhone: false)
        let styler = LiveMarkdownStyler(style: style)
        let layoutDelegate = InklingLayoutDelegate(palette: style.palette)
        let textView = WritingNSTextView(usingTextLayoutManager: true)
        textView.column = style.column
        textView.textLayoutManager?.delegate = layoutDelegate
        let storage = try #require(textView.textStorage)

        final class Delegate: NSObject, NSTextStorageDelegate {
            let styler: LiveMarkdownStyler
            var restyling = false
            init(_ styler: LiveMarkdownStyler) { self.styler = styler }
            func textStorage(_ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorageEditActions, range editedRange: NSRange, changeInLength delta: Int) {
                guard !restyling, editedMask.contains(.editedCharacters) else { return }
                restyling = true
                styler.restyleEdited(textStorage, editedRange: editedRange)
                restyling = false
            }
        }

        let storageDelegate = Delegate(styler)
        storage.delegate = storageDelegate

        let scroll = NSScrollView(frame: CGRect(x: 0, y: 0, width: 900, height: 760))
        textView.frame = scroll.bounds
        textView.isVerticallyResizable = true
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        scroll.documentView = textView

        let window = Offscreen(contentRect: scroll.bounds, styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = scroll
        window.setFrameOrigin(CGPoint(x: -40000, y: -40000))
        window.orderFrontRegardless()
        defer {
            window.orderOut(nil)
            window.close()
        }

        let probe = TypingProbe()
        var samples: [Int] = []
        probe.onSample = { samples.append($0) }
        textView.probe = probe

        storageDelegate.restyling = true
        storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: draft)
        styler.restyleAll(storage)
        storageDelegate.restyling = false
        textView.setSelectedRange(NSRange(location: 0, length: 0))
        window.displayIfNeeded()

        for ch in typed {
            textView.insertText(String(ch), replacementRange: NSRange(location: NSNotFound, length: 0))
            await Task.yield()
        }
        try await Task.sleep(for: .milliseconds(50))

        #expect(textView.string.hasPrefix(typed), "all 50 typed characters landed in the draft")
        #expect(samples.count == typed.count, "every keystroke produced a painted-frame sample: \(samples.count)")
        let sorted = samples.sorted()
        let medianMs = Double(sorted[sorted.count / 2]) / 1000.0
        let worstMs = Double(sorted.last ?? 0) / 1000.0
        let fastestMs = Double(sorted.first ?? 0) / 1000.0
        if let outPath = ProcessInfo.processInfo.environment["INKLING_PROBE_OUT"] {
            let body = samples.map(String.init).joined(separator: "\n") + "\n"
            try body.write(toFile: outPath, atomically: true, encoding: .utf8)
        }
        fputs(String(format: "MAC_TYPING_BENCH samples=%d fastest=%.2fms median=%.2fms worst=%.2fms\n", samples.count, fastestMs, medianMs, worstMs), stderr)
        #expect(medianMs < 16.6, "median keystroke-to-painted time stays under one 60fps frame: \(medianMs) ms")
    }

    @Test func theAskBarAnswersFromTheLibraryAndKeepsOneConversation() async throws {
        let asker = shelf.asker
        asker.clear()
        let walden = try #require(shelf.books.first { $0.title == "Walden" })
        let article = try #require(shelf.inbox.first)

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.selection = article.id
            try await shoot(root(navigator), "\(name)-ask-bar", appearance)
            asker.composing = true
            try await shoot(root(navigator), "\(name)-ask-open", appearance)
            asker.composing = false
            navigator.section = .writing
            navigator.document = shelf.docs.first?.id
            asker.adding = true
            try await shoot(root(navigator), "\(name)-ask-add", appearance)
            #expect(asker.suggestions == [.fixGrammar, .findWord, .nextLine], "in Writing, + suggests the writing actions")
            asker.adding = false
        }

        // Ask from Walden's reader; the answer cites the passages it drew from Walden.
        let book = try await shelf.open(walden.id)
        let set = Typesetting(face: "Source Serif 4", size: 21, lineHeight: 1.75, titleSize: 36, palette: palette(.aqua), column: 700)
        let reader = ReaderModel(item: walden, rendered: Renderer.render(book, set: set, folder: URL(filePath: shelf.library.folder()), byline: nil), shelf: shelf)
        asker.place = .reading(reader)
        #expect(asker.suggestions == [.explain, .summarize], "in the reader, + suggests Explain and Summarize")
        asker.draft = "What does Walden say about simplicity?"
        asker.send()
        for _ in 0..<300 where asker.turns.isEmpty { try await Task.sleep(for: .milliseconds(100)) }
        let turn = try #require(asker.turns.last, "the question is answered")
        #expect(turn.context.hasPrefix("Walden"), "the turn shows where it was asked: \(turn.context)")
        let citation = try #require(turn.citations.first, "the answer cites a passage: \(turn.answer)")
        #expect(citation.item == walden.id && citation.words.localizedCaseInsensitiveContains("simplicity"))
        let markdown = book.chapters[Int(citation.chapter)].text.utf8
        let start = markdown.index(markdown.startIndex, offsetBy: Int(citation.start))
        let end = markdown.index(markdown.startIndex, offsetBy: Int(citation.end))
        #expect(String(markdown[start..<end]) == citation.words, "a citation is its chapter's own bytes, so it opens where it is")

        // Over in the Inbox, the same conversation is there, and it is kept through a relaunch.
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.section = .inbox
            navigator.selection = article.id
            asker.composing = true
            try await shoot(root(navigator), "\(name)-ask-answer", appearance)
            #expect(asker.turns.last == turn, "the conversation survives a tab switch")
            asker.composing = false
        }
        #expect(try Shelf(folder: data).asker.turns == asker.turns, "the conversation is kept on disk")

        let navigator = Navigator()
        navigator.open(walden, at: Passage(chapter: citation.chapter, start: citation.start, end: citation.end, words: citation.words, made: 0))
        #expect(navigator.section == .library && navigator.selection == walden.id && navigator.passage?.id == walden.id, "a citation opens its book at the passage")

        // In Writing, an instruction goes to the inline review, and the turn says so.
        let doc = try #require(shelf.docs.first { $0.id == "quiet-desk" })
        let writer = WritingModel(id: doc.id, text: shelf.readDocument(doc.id), shelf: shelf)
        asker.place = .writing(writer)
        asker.draft = "Tighten the opening paragraph"
        asker.send()
        #expect(asker.turns.last?.edit == true && writer.isAsking, "an edit asked through the bar opens a review, not an answer")
        for _ in 0..<200 where !writer.isReviewing { try await Task.sleep(for: .milliseconds(100)) }
        #expect(writer.isReviewing, "the edit arrives through the inline review")
        writer.cancelReview()
        asker.clear()
    }

    @Test func signInSyncInboxFeedsAndReadwise() async throws {
        let syncRoot = data.appending(path: "sync-proof")
        let serverFile = syncRoot.appending(path: "fake-server.json")
        let macDir = syncRoot.appending(path: "mac")
        let phoneDir = syncRoot.appending(path: "phone")
        try FileManager.default.createDirectory(at: syncRoot, withIntermediateDirectories: true)

        let macShelf = try Shelf(folder: macDir, fakeServer: serverFile)
        let phoneShelf = try Shelf(folder: phoneDir, fakeServer: serverFile)
        #expect(!macShelf.account.signedIn, "starts signed out")

        // 1. Render signed-out Account settings in Light and Dark.
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try await shoot(SettingsView(initialTab: .account).environment(macShelf).environment(Navigator()), "\(name)-sign-in", appearance, size: CGSize(width: 660, height: 420))
        }

        // 2. Stage Worker articles (feeds -> Inbox, newsletter -> Inbox, saved link -> Queue).
        let body = String(repeating: "Software that keeps your data on your own device remains fast offline and respects the reader's attention. ", count: 8)
        let feedOne = """
        <!doctype html><html><head><title>Notes on Local-First Software</title>
        <meta property="og:site_name" content="Ink &amp; Switch"><meta name="author" content="Martin Kleppmann">
        <meta name="description" content="Keeping your writing and reading library on your own devices while syncing quietly."></head>
        <body><article><h1>Notes on Local-First Software</h1><p>\(body)</p><p>\(body)</p></article></body></html>
        """
        let feedTwo = """
        <!doctype html><html><head><title>On Keeping a Commonplace Book</title>
        <meta property="og:site_name" content="The Marginalian"><meta name="author" content="Maria Popova">
        <meta name="description" content="Collecting passages and reflections over years of attentive reading."></head>
        <body><article><h1>On Keeping a Commonplace Book</h1><p>\(body)</p></article></body></html>
        """
        let newsletter = """
        <!doctype html><html><head><title>Sunday Reading Dispatch</title>
        <meta property="og:site_name" content="Weekly Letters"><meta name="author" content="Weekly Letters">
        <meta name="description" content="Three essays from the week worth reading without hurry."></head>
        <body><article><h1>Sunday Reading Dispatch</h1><p>\(body)</p></article></body></html>
        """
        let saved = """
        <!doctype html><html><head><title>A Field Guide to Quiet Tools</title>
        <meta property="og:site_name" content="Craft Notes"><meta name="author" content="Toby">
        <meta name="description" content="Why restrained interfaces leave room for thought."></head>
        <body><article><h1>A Field Guide to Quiet Tools</h1><p>\(body)</p></article></body></html>
        """
        _ = stageFakeIncoming(serverFile: serverFile.path, source: "feed", destination: "inbox", url: "https://inkandswitch.example/local-first", title: "Notes on Local-First Software", html: feedOne)
        _ = stageFakeIncoming(serverFile: serverFile.path, source: "feed", destination: "inbox", url: "https://marginalian.example/commonplace", title: "On Keeping a Commonplace Book", html: feedTwo)
        _ = stageFakeIncoming(serverFile: serverFile.path, source: "newsletter", destination: "inbox", url: "https://tobygodat.com/newsletters/sunday", title: "Sunday Reading Dispatch", html: newsletter)
        _ = stageFakeIncoming(serverFile: serverFile.path, source: "save", destination: "queue", url: "https://craft.example/quiet-tools", title: "A Field Guide to Quiet Tools", html: saved)

        // 3. Sign in against the fake server; sync parses staged articles into Inbox and Queue.
        let signedIn = await macShelf.account.signInAsync()
        #expect(signedIn && macShelf.account.signedIn, "Mac signs in with Google via fake server")
        #expect(macShelf.inbox(.feeds).map(\.title).sorted() == ["Notes on Local-First Software", "On Keeping a Commonplace Book"], "feed items land in the Inbox as Feeds: \(macShelf.inbox.map(\.title))")
        #expect(macShelf.inbox(.newsletters).map(\.title) == ["Sunday Reading Dispatch"], "a newsletter lands in the Inbox as a Newsletter")
        #expect(macShelf.inbox(.saved).map(\.title) == ["A Field Guide to Quiet Tools"], "a saved link lands in the Inbox as Saved")

        // 4. Render Inbox with feed items in Light and Dark.
        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let navigator = Navigator()
            navigator.section = .inbox
            navigator.filter = .feeds
            navigator.selection = macShelf.inbox.first { $0.title == "Notes on Local-First Software" }?.id
            try await shoot(RootView().environment(macShelf).environment(navigator), "\(name)-inbox-feeds", appearance)
        }

        // 5. Follow/unfollow feeds and connect Readwise (never storing the Readwise token on device).
        #expect(await macShelf.account.followFeed(url: "https://daringfireball.net/feeds/main"))
        #expect(await macShelf.account.followFeed(url: "https://stratechery.com/feed/"))
        #expect(await macShelf.account.followFeed(url: "https://temporary.example/rss.xml"))
        #expect(macShelf.account.feeds.count == 3)
        let tempFeed = try #require(macShelf.account.feeds.first { $0.title == "temporary.example" })
        await macShelf.account.unfollowFeed(tempFeed)
        #expect(macShelf.account.feeds.count == 2 && !macShelf.account.feeds.contains { $0.id == tempFeed.id })

        let secretToken = "rw-mac-secret-token-never-stored-locally"
        #expect(await macShelf.account.connectReadwise(token: secretToken))
        #expect(macShelf.account.readwiseConnected)
        for file in Self.allFiles(under: macDir) {
            let bytes = (try? Data(contentsOf: file)) ?? Data()
            #expect(!String(decoding: bytes, as: UTF8.self).contains(secretToken), "Readwise token must never be stored on device: \(file.path)")
        }

        for (name, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            try await shoot(SettingsView(initialTab: .account).environment(macShelf).environment(Navigator()), "\(name)-settings-account", appearance, size: CGSize(width: 660, height: 440))
            try await shoot(SettingsView(initialTab: .feeds).environment(macShelf).environment(Navigator()), "\(name)-settings-feeds", appearance, size: CGSize(width: 660, height: 440))
        }

        // 6. Round-trip a document, article, reading position and highlight to a second device via the fake server.
        let docId = macShelf.createDocument()
        let firstDraft = "# Synced Essay\n\nWritten on Mac and delivered to iPhone."
        try macShelf.saveDocument(id: docId, text: firstDraft, triggerSync: false)
        let savedArticle = try #require(macShelf.inbox.first { $0.title == "A Field Guide to Quiet Tools" })
        macShelf.keepPlace(savedArticle.id, chapter: 0, fraction: 0.45)
        macShelf.flushPlaces(triggerSync: false)
        let passage = Passage(chapter: 0, start: 2, end: 29, words: "A Field Guide to Quiet Tools", made: 1_700_000_000_000)
        macShelf.setHighlights(savedArticle.id, [passage])
        _ = await macShelf.account.syncNowAsync()

        #expect(await phoneShelf.account.signInAsync())
        #expect(phoneShelf.readDocument(docId) == firstDraft, "document written on device A arrives on device B")
        #expect(phoneShelf.inbox(.feeds).count == 2 && phoneShelf.inbox(.newsletters).count == 1, "Inbox articles sync across devices as feeds and newsletters")
        let phoneArticle = try #require(phoneShelf.inbox(.saved).first { $0.id == savedArticle.id })
        #expect(abs(phoneArticle.fraction - 0.45) < 0.01, "reading position syncs across devices")
        #expect(phoneShelf.highlights(savedArticle.id) == [passage], "highlights sync across devices")
        #expect(phoneShelf.account.readwiseConnected, "Readwise connection status syncs")

        // Edit on device B and round-trip back to device A.
        let editedDraft = "# Synced Essay\n\nUpdated on iPhone and synced back to Mac."
        try phoneShelf.saveDocument(id: docId, text: editedDraft, triggerSync: false)
        _ = await phoneShelf.account.syncNowAsync()
        _ = await macShelf.account.syncNowAsync()
        #expect(macShelf.readDocument(docId) == editedDraft, "document edit round-trips back to device A")

        // Archive an Inbox item and bring it back, disconnect Readwise, and sign out.
        let feedItem = try #require(macShelf.inbox.first { $0.title == "Notes on Local-First Software" })
        macShelf.setArchived(feedItem, true)
        #expect(!macShelf.inbox.contains { $0.id == feedItem.id } && macShelf.archive.contains { $0.id == feedItem.id })
        macShelf.setArchived(feedItem, false)
        #expect(macShelf.inbox(.feeds).contains { $0.id == feedItem.id }, "brought back, it is a feed post again")
        await macShelf.account.disconnectReadwise()
        #expect(!macShelf.account.readwiseConnected)
        macShelf.account.signOut()
        #expect(!macShelf.account.signedIn)
    }

    private static func allFiles(under folder: URL) -> [URL] {
        guard let enumerator = FileManager.default.enumerator(at: folder, includingPropertiesForKeys: [.isRegularFileKey]) else { return [] }
        return enumerator.compactMap { $0 as? URL }.filter { (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true }
    }

    private func root(_ navigator: Navigator) -> some View {
        RootView().environment(shelf).environment(navigator)
    }

    private func palette(_ appearance: NSAppearance.Name) -> Palette {
        Chrome(shelf.looks).palette(appearance == .darkAqua ? .dark : .light)
    }

    /// Renders `view` in a window ordered in far off the screen, never key and never active, and
    /// takes the window server's own image of it, so the sidebar's glass and the toolbar draw as
    /// they do on screen.
    @discardableResult
    private func shoot(_ view: some View, _ name: String, _ appearance: NSAppearance.Name, size: CGSize = CGSize(width: 1280, height: 800), titled: Bool = true, before: @MainActor (NSWindow) async throws -> Void = { _ in }) async throws -> CGImage {
        let style: NSWindow.StyleMask = titled ? [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView] : [.titled, .fullSizeContentView]
        let window = Offscreen(contentRect: CGRect(origin: .zero, size: size), styleMask: style, backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: appearance)
        window.isReleasedWhenClosed = false
        if !titled { window.titlebarAppearsTransparent = true; window.titleVisibility = .hidden }
        let host = NSHostingController(rootView: view.preferredColorScheme(appearance == .darkAqua ? .dark : .light))
        host.sceneBridgingOptions = .all
        window.contentViewController = host
        window.setContentSize(size)
        window.setFrameOrigin(CGPoint(x: -40000, y: -40000))
        window.orderFrontRegardless()
        try await Task.sleep(for: .seconds(2.5))
        try await before(window)
        let image = try #require(Capture.window(window.windowNumber), "the window server gave no image of the window")
        let rep = NSBitmapImageRep(cgImage: image)
        let png = try #require(rep.representation(using: .png, properties: [:]))
        try png.write(to: out.appending(path: "\(name).png"))
        window.orderOut(nil)
        window.close()
        return image
    }

    /// Share of sampled pixels in `region` (normalised `0...1`) that carry real colour rather than
    /// achromatic paper or ink, used to assert that an inline painting actually drew.
    private static func colouredShare(in image: CGImage, region: CGRect) -> Double {
        let rect = CGRect(
            x: (CGFloat(image.width) * region.minX).rounded(),
            y: (CGFloat(image.height) * region.minY).rounded(),
            width: max(1, (CGFloat(image.width) * region.width).rounded()),
            height: max(1, (CGFloat(image.height) * region.height).rounded())
        )
        guard let crop = image.cropping(to: rect) else { return 0 }
        let width = crop.width, height = crop.height
        var rgba = [UInt8](repeating: 0, count: width * height * 4)
        guard let context = CGContext(data: &rgba, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return 0 }
        context.draw(crop, in: CGRect(x: 0, y: 0, width: width, height: height))
        var coloured = 0, total = 0
        for y in stride(from: 0, to: height, by: 4) {
            for x in stride(from: 0, to: width, by: 4) {
                let i = (y * width + x) * 4
                let r = Int(rgba[i]), g = Int(rgba[i + 1]), b = Int(rgba[i + 2])
                if max(r, g, b) - min(r, g, b) > 18 { coloured += 1 }
                total += 1
            }
        }
        return total > 0 ? Double(coloured) / Double(total) : 0
    }
}

/// Serves a saved page and its pictures in place of the network.
nonisolated private final class Pages: URLProtocol {
    static let session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [Pages.self]
        return URLSession(configuration: configuration)
    }()

    private static let words = String(repeating: "The tide came in slowly over the flats, and the birds lifted as it reached them. ", count: 12)
    private static let page = """
    <html><head><title>The tide</title><meta name="author" content="Ana Lima"></head><body><article><h1>The tide</h1>
    <p>\(words)</p><figure><img src="/flats.png" alt="The flats at dawn"><figcaption>The flats at dawn</figcaption></figure>
    <p><img src="file:///etc/hosts.png" alt="Not fetched"></p><p>\(words)</p></article></body></html>
    """

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host() == "pages.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        let body = url.path().hasSuffix(".png") ? Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")! : Data(Self.page.utf8)
        let type = url.path().hasSuffix(".png") ? "image/png" : "text/html; charset=utf-8"
        let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": type])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

/// A window the system may put anywhere, so it can sit off every screen.
private final class Offscreen: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
    /// Drawn as the focused window a person looks at, without taking focus: an inactive window
    /// greys its toolbar, which no one using the app sees.
    override var isKeyWindow: Bool { true }
    override var isMainWindow: Bool { true }
}

/// `CGWindowListCreateImage`, which the SDK no longer declares; a process may still take its own
/// windows without the Screen Recording permission.
private enum Capture {
    private typealias Create = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
    private static let create: Create? = {
        guard let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "CGWindowListCreateImage") else { return nil }
        return unsafeBitCast(symbol, to: Create.self)
    }()

    static func window(_ number: Int) -> CGImage? {
        let includingWindow: UInt32 = 1 << 3
        let ignoreFraming: UInt32 = 1 << 0, bestResolution: UInt32 = 1 << 3
        return create?(.null, includingWindow, UInt32(number), ignoreFraming | bestResolution)?.takeRetainedValue()
    }
}

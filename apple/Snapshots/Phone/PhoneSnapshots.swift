import XCTest

/// The iPhone app on the Simulator: every tab in Light and Dark, swiping to archive, the Ask bar
/// answering with a citation and keeping its conversation across tabs, and a highlight made in the
/// reader that is still there after the app is launched again. Each screenshot is an attachment
/// that apple/snap.sh exports.
nonisolated final class PhoneSnapshots: XCTestCase {
    private var environment: [String: String] { ProcessInfo.processInfo.environment }
    /// The appearance the app is launched in. The Simulator's own setting doesn't reach apps on a
    /// Simulator booted headless, so the app takes it from `INKLING_APPEARANCE`.
    private var appearance = "light"

    override func setUp() {
        continueAfterFailure = false
    }


    @MainActor func testEveryScreenInLightAndDark() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let app = launch(data: "\(base)/\(name)", demo: true)
            XCTAssertEqual(app.tabBars.buttons.count, 4, "four tabs")
            for tab in ["Inbox", "Library", "Writing", "Profile"] {
                XCTAssertTrue(app.tabBars.buttons[tab].exists, "\(tab) is a tab")
            }
            XCTAssertTrue(app.buttons["ask-bar"].waitForExistence(timeout: 5), "the Ask bar sits above the tabs")
            XCTAssertTrue(app.searchFields.firstMatch.isHittable, "search is at the top of the Inbox")
            shoot("\(name)-inbox")

            row(app, "Walking").tap()
            let walking = app.textViews["reader"]
            XCTAssertTrue(walking.waitForExistence(timeout: 10))
            sleep(1)
            walking.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.65))
                .press(forDuration: 0.05, thenDragTo: walking.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.35)))
            sleep(1)
            XCTAssertGreaterThan(colouredShare(in: CGRect(x: 0.15, y: 0.15, width: 0.7, height: 0.7)), 0.12, "Walking's painting draws in the \(name) reader")
            shoot("\(name)-reader-picture")
            showChrome(app)
            app.navigationBars.buttons.element(boundBy: 0).tap()
            XCTAssertTrue(row(app, "An Apology for Idlers").waitForExistence(timeout: 5))

            row(app, "An Apology for Idlers").tap()
            let reader = app.textViews["reader"]
            XCTAssertTrue(reader.waitForExistence(timeout: 10))
            sleep(1)
            shoot("\(name)-reader")

            highlight(app, in: reader)
            shoot("\(name)-reader-highlight")
            XCTAssertEqual(highlights(app), 1, "the passage is highlighted")

            reader.swipeUp(velocity: .slow)
            sleep(1)
            XCTAssertFalse(app.buttons["appearance"].isHittable, "the bars get out of the way as the reader reads on")
            XCTAssertFalse(app.buttons["ask-bar"].isHittable, "the Ask bar hides in the immersive reader")
            shoot("\(name)-reader-immersed")
            showChrome(app)

            app.buttons["appearance"].tap()
            let sepia = app.buttons["tone-sepia"]
            XCTAssertTrue(sepia.waitForExistence(timeout: 5))
            sepia.tap()
            sleep(1)
            shoot("\(name)-reader-appearance")
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)).tap()
            sleep(1)
            shoot("\(name)-reader-sepia")
            showChrome(app)
            app.navigationBars.buttons.element(boundBy: 0).tap()

            row(app, "Self-Reliance").swipeLeft()
            let archive = app.collectionViews.buttons["Archive"]
            XCTAssertTrue(archive.waitForExistence(timeout: 5))
            shoot("\(name)-inbox-swipe")
            archive.tap()
            XCTAssertTrue(row(app, "Self-Reliance").waitForNonExistence(timeout: 3), "archiving takes it out of the Inbox")

            app.buttons["add"].tap()
            XCTAssertTrue(app.textFields["link"].waitForExistence(timeout: 5))
            shoot("\(name)-save-link")
            app.buttons["Cancel"].tap()
            sleep(1)

            tapTab(app, "Library")
            sleep(1)
            XCTAssertTrue(row(app, "Walden").exists && !row(app, "Walking").exists, "the Library holds books only")
            shoot("\(name)-library")

            tapTab(app, "Profile")
            sleep(1)
            shoot("\(name)-profile")
            app.buttons["profile-appearance"].tap()
            XCTAssertTrue(app.segmentedControls["list-density"].waitForExistence(timeout: 5), "Profile's Appearance holds the Compact / Comfortable density setting")
            app.segmentedControls["list-density"].buttons["Comfortable"].tap()
            app.navigationBars.buttons.element(boundBy: 0).tap()
            tapTab(app, "Inbox")
            sleep(1)
            shoot("\(name)-inbox-comfortable")
            tapTab(app, "Profile")
            app.buttons["profile-appearance"].tap()
            XCTAssertTrue(app.segmentedControls["list-density"].waitForExistence(timeout: 5))
            app.segmentedControls["list-density"].buttons["Compact"].tap()
            app.navigationBars.buttons.element(boundBy: 0).tap()
            app.buttons["profile-archive"].tap()
            XCTAssertTrue(row(app, "Self-Reliance").waitForExistence(timeout: 5), "what was archived is in the Archive, in Profile")
            shoot("\(name)-archive")
            app.navigationBars.buttons.element(boundBy: 0).tap()
            app.buttons["profile-highlights"].tap()
            XCTAssertTrue(app.staticTexts["An Apology for Idlers"].waitForExistence(timeout: 5), "the highlight is in Highlights, in Profile, under its article")
            shoot("\(name)-highlights")
            app.navigationBars.buttons.element(boundBy: 0).tap()
            tapTab(app, "Inbox")

            app.terminate()
            let again = launch(data: "\(base)/\(name)", demo: false)
            row(again, "An Apology for Idlers").tap()
            XCTAssertTrue(again.textViews["reader"].waitForExistence(timeout: 10))
            XCTAssertEqual(highlights(again), 1, "the highlight is kept through a relaunch")
            showChrome(again)
            again.buttons["appearance"].tap()
            XCTAssertTrue(again.buttons["tone-sepia"].waitForExistence(timeout: 5))
            XCTAssertTrue(again.buttons["tone-sepia"].isSelected, "the paper is kept through a relaunch")
            again.terminate()
        }
    }

    /// The Ask bar in a book: + suggests for the screen, a question naming a book is answered with
    /// a citation that opens its passage, and the conversation is still there in another tab.
    @MainActor func testAskBarAnswersWithACitationAcrossTabs() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let app = launch(data: "\(base)/ask-\(name)", demo: true)
            app.tabBars.buttons["Library"].tap()
            row(app, "Walden").tap()
            XCTAssertTrue(app.textViews["reader"].waitForExistence(timeout: 10))
            sleep(1)
            XCTAssertEqual(app.buttons["ask-bar"].label, "Ask about Walden", "the bar knows what is on screen")
            shoot("\(name)-ask-bar")

            app.buttons["ask-add"].tap()
            XCTAssertTrue(app.buttons["suggest-explain"].waitForExistence(timeout: 5), "in the reader, + suggests Explain and Summarize")
            XCTAssertTrue(app.buttons["suggest-summarize"].exists && app.buttons["attach-item"].exists)
            sleep(1)
            shoot("\(name)-ask-add")
            app.buttons["ask-add-close"].tap()
            sleep(1)

            app.buttons["ask-bar"].tap()
            let field = app.descendants(matching: .any).matching(identifier: "ask-field").firstMatch
            XCTAssertTrue(field.waitForExistence(timeout: 5), "tapping the bar opens the composer")
            sleep(1)
            shoot("\(name)-ask-open")
            field.typeText("What does Walden say about simplicity?")
            app.buttons["ask-send"].tap()
            let citation = app.buttons["citation-1"]
            XCTAssertTrue(citation.waitForExistence(timeout: 30), "the answer cites a passage of Walden")
            sleep(1)
            shoot("\(name)-ask-answer")

            citation.tap()
            sleep(2)
            XCTAssertTrue(place(app).hasPrefix("Chapter 1 of 2"), "the citation opens its passage: \(place(app))")

            app.tabBars.buttons["Inbox"].tap()
            sleep(1)
            XCTAssertEqual(app.buttons["ask-bar"].label, "Ask", "in the Inbox the bar asks about nothing in particular")
            app.buttons["ask-bar"].tap()
            XCTAssertTrue(app.buttons["citation-1"].waitForExistence(timeout: 5), "the conversation survives a tab switch")
            app.terminate()
        }
    }

    /// On Auto paper, choosing Light or Dark in Appearance recolours the reader that is open.
    @MainActor func testAutoPaperFollowsTheAppearanceChosenWhileReading() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        appearance = ""
        let app = launch(data: "\(base)/picker", demo: true)
        row(app, "An Apology for Idlers").tap()
        XCTAssertTrue(app.textViews["reader"].waitForExistence(timeout: 10))
        choose(app, "Light")
        XCTAssertGreaterThan(ground(), 0.8, "Light draws the paper light")
        choose(app, "Dark")
        XCTAssertLessThan(ground(), 0.2, "Dark recolours the open reader")
        choose(app, "System")
        app.terminate()
    }

    /// A book's chapters: its footnote marks raised, "Next: …" at a chapter's end going on to the next,
    /// and Contents going back.
    @MainActor func testChaptersAndFootnotes() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let app = launch(data: "\(base)/chapters-\(name)", demo: true)
            app.tabBars.buttons["Library"].tap()
            row(app, "Walden").tap()
            let reader = app.textViews["reader"]
            XCTAssertTrue(reader.waitForExistence(timeout: 10))
            sleep(1)
            shoot("\(name)-reader-footnotes")
            XCTAssertTrue(place(app).hasPrefix("Chapter 1 of 2"), place(app))

            let next = reader.links.matching(NSPredicate(format: "label BEGINSWITH 'Next: Economy'")).firstMatch
            XCTAssertTrue(next.waitForExistence(timeout: 5), "the chapter ends with \"Next: …\"")
            for _ in 0..<4 where !next.isHittable {
                reader.swipeUp(velocity: .slow)
                sleep(1)
            }
            shoot("\(name)-reader-next")
            next.tap()
            sleep(2)
            XCTAssertTrue(place(app).hasPrefix("Chapter 2 of 2"), "\"Next: …\" goes to the next chapter: \(place(app))")
            shoot("\(name)-reader-next-chapter")

            app.buttons["contents"].tap()
            let first = app.buttons["Where I Lived, and What I Lived For"]
            XCTAssertTrue(first.waitForExistence(timeout: 5), "Contents lists the chapters")
            shoot("\(name)-reader-contents")
            first.tap()
            sleep(2)
            XCTAssertTrue(place(app).hasPrefix("Chapter 1 of 2"), "a chapter chosen in Contents opens: \(place(app))")
            app.terminate()
        }
    }

    /// The native writing page and inline review in Light and Dark: documents list, live Markdown
    /// canvas, Fix grammar inline review, accepting/rejecting changes and persisting across relaunch.
    @MainActor func testWritingPage() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let dir = "\(base)/writing-\(name)"
            let app = launch(data: dir, demo: true)
            let writingTab = app.tabBars.buttons["Writing"]
            XCTAssertTrue(writingTab.waitForExistence(timeout: 5), "Writing tab exists")
            writingTab.tap()
            sleep(1)
            shoot("\(name)-documents")

            let doc = app.buttons["doc-On Keeping a Quiet Desk"].firstMatch
            XCTAssertTrue(doc.waitForExistence(timeout: 5), "demo document appears in Writing list")
            doc.tap()
            let editor = app.textViews["editor"]
            XCTAssertTrue(editor.waitForExistence(timeout: 10), "TextKit 2 writing editor opens")
            sleep(1)
            shoot("\(name)-writing")

            // Ask Fix grammar (INKLING_FAKE_AI=1) and review inline.
            XCTAssertFalse(app.buttons["fix-grammar"].exists || app.buttons["ask-send"].exists, "the page has no chips or field of its own")
            app.buttons["ask-add"].tap()
            let fix = app.buttons["suggest-fixGrammar"]
            XCTAssertTrue(fix.waitForExistence(timeout: 5), "in Writing, + suggests Fix grammar")
            fix.tap()
            let useButton = app.buttons["review-use"]
            XCTAssertTrue(useButton.waitForExistence(timeout: 15), "inline review opens with Use/Keep controls")
            sleep(1)
            shoot("\(name)-writing-review")

            // Accept the first change, reject the second, then accept the rest.
            useButton.tap()
            sleep(1)
            let keepButton = app.buttons["review-keep"]
            XCTAssertTrue(keepButton.waitForExistence(timeout: 5))
            keepButton.tap()
            sleep(1)
            app.buttons["accept-rest"].tap()
            XCTAssertTrue(editor.waitForExistence(timeout: 5), "finishing review returns to the live editor")
            sleep(1)

            // Verify accepted edits persisted to disk across a relaunch.
            app.terminate()
            let again = launch(data: dir, demo: false)
            again.tabBars.buttons["Writing"].tap()
            again.buttons["doc-On Keeping a Quiet Desk"].firstMatch.tap()
            let reloadedEditor = again.textViews["editor"]
            XCTAssertTrue(reloadedEditor.waitForExistence(timeout: 10))
            let text = (reloadedEditor.value as? String) ?? ""
            XCTAssertTrue(text.contains("crosses the grain"), "accepted change survives relaunch")
            XCTAssertTrue(text.contains("i dont want"), "rejected change stays untouched")
            XCTAssertTrue(text.contains("which holds"), "remaining accepted changes survive relaunch")
            again.terminate()
        }
    }

    /// The dev feedback handle (TOB-571): signed in, read for a few seconds, open the panel from the
    /// right edge, attach the last 10 seconds and send. Nothing reaches the fake server before Send;
    /// after it, the record holds the words, the reader's place and a clip that is an MP4.
    @MainActor func testDevFeedbackSendsTheLastTenSeconds() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let dir = "\(base)/feedback-\(name)"
            let serverPath = "\(dir)/fake-server.json"
            let app = launch(data: dir, demo: true, devFeedback: true)
            let handle = app.buttons["feedback-handle"]
            XCTAssertTrue(handle.waitForExistence(timeout: 5), "a dev build shows the handle")
            XCTAssertGreaterThan(handle.frame.maxX, app.frame.maxX - 2, "the handle sits on the right edge")
            tapTab(app, "Profile")
            app.buttons["sign-in-google"].tap()
            XCTAssertTrue(app.buttons["copy-newsletter-address"].waitForExistence(timeout: 10), "signed in")
            tapTab(app, "Inbox")
            sleep(1)
            shoot("\(name)-feedback-handle")

            row(app, "An Apology for Idlers").tap()
            let reader = app.textViews["reader"]
            XCTAssertTrue(reader.waitForExistence(timeout: 10))
            sleep(1)
            reader.swipeUp(velocity: .slow)
            sleep(1)
            reader.swipeDown(velocity: .slow)
            sleep(1)

            handle.tap()
            XCTAssertTrue(app.otherElements["feedback-panel"].waitForExistence(timeout: 5), "the handle opens the panel")
            sleep(1)
            let attach = app.switches["feedback-attach"]
            attach.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
            if !isOn(attach) {
                attach.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
            }
            XCTAssertTrue(isOn(attach), "Attach is on")
            let clip = app.staticTexts["feedback-clip"]
            XCTAssertTrue(clip.waitForExistence(timeout: 5))
            XCTAssertTrue(wait(for: clip, label: "touch", timeout: 30), "the clip is made, with its touches: \(clip.label)")
            attach.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
            XCTAssertTrue(clip.waitForNonExistence(timeout: 3), "Attach off drops the clip")
            attach.coordinate(withNormalizedOffset: CGVector(dx: 0.92, dy: 0.5)).tap()
            XCTAssertTrue(isOn(attach))
            XCTAssertTrue(clip.waitForExistence(timeout: 5))
            XCTAssertTrue(wait(for: clip, label: "touch", timeout: 30), "Attach on again makes the same capture's clip: \(clip.label)")
            let field = app.textFields["feedback-text"]
            field.tap()
            field.typeText("The reader's bar hides too soon after a slow scroll.")
            app.staticTexts["Feedback"].tap()
            sleep(1)
            XCTAssertTrue(try server(serverPath).feedback.isEmpty, "nothing reaches the server before Send")
            shoot("\(name)-feedback-panel")

            app.buttons["feedback-send"].tap()
            XCTAssertTrue(app.otherElements["feedback-sent"].waitForExistence(timeout: 10), "Send says it was sent")
            sleep(1)
            shoot("\(name)-feedback-sent")

            let (records, files) = try server(serverPath)
            XCTAssertEqual(records.count, 1, "one record reached the server")
            let record = records.first ?? [:]
            XCTAssertEqual(record["text"] as? String, "The reader's bar hides too soon after a slow scroll.")
            XCTAssertEqual(record["screen"] as? String, "reader")
            XCTAssertTrue((record["place"] as? String)?.contains("An Apology for Idlers") == true, "the record says where it was sent from")
            let sent = record["clip"] as? [String: Any] ?? [:]
            let seconds = sent["seconds"] as? Double ?? 0
            XCTAssertTrue(seconds > 3 && seconds <= 10.01, "the clip covers the seconds before the panel opened, at most 10")
            XCTAssertGreaterThan(sent["touches"] as? Int ?? 0, 0, "the clip counts the touches")
            let mp4 = Data(base64Encoded: files["feedback/\(record["id"] as? String ?? "")/clip.mp4"] ?? "") ?? Data()
            XCTAssertEqual(String(decoding: mp4.dropFirst(4).prefix(4), as: UTF8.self), "ftyp", "the clip is an MP4")

            app.buttons["feedback-done"].tap()
            XCTAssertTrue(handle.waitForExistence(timeout: 5), "Done puts the handle back")
            app.terminate()
        }
    }

    /// Waits up to 3 seconds for `toggle` to be on.
    @MainActor private func isOn(_ toggle: XCUIElement) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == '1'"), object: toggle)
        return XCTWaiter.wait(for: [expectation], timeout: 3) == .completed
    }

    /// Waits up to `timeout` seconds for `element`'s label to contain `words`.
    @MainActor private func wait(for element: XCUIElement, label words: String, timeout: TimeInterval) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS %@", words), object: element)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    /// The feedback records and files in the fake server's file, or none before its first request.
    private func server(_ path: String) throws -> (feedback: [[String: Any]], files: [String: String]) {
        guard FileManager.default.fileExists(atPath: path) else { return ([], [:]) }
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(filePath: path))) as? [String: Any] ?? [:]
        return (json["feedback"] as? [[String: Any]] ?? [], json["files"] as? [String: String] ?? [:])
    }

    /// Headless proof of Google sign-in, fake-server sync, Inbox with feed items, follow/unfollow
    /// feeds, Readwise connection (token never stored on device) and document round-trip in Light and Dark.
    @MainActor func testSignInSyncInboxFeedsAndReadwise() throws {
        guard let base = environment["INKLING_SNAP_DATA"] else { throw XCTSkip("apple/snap.sh sets INKLING_SNAP_DATA") }
        for name in ["light", "dark"] {
            appearance = name
            let dir = "\(base)/sync-\(name)"
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
            let serverPath = "\(dir)/fake-server.json"
            try seedFakeServer(at: serverPath)

            let app = launch(data: dir, demo: false)
            app.tabBars.buttons["Profile"].tap()

            // 1. Signed-out state.
            let signIn = app.buttons["sign-in-google"]
            XCTAssertTrue(signIn.waitForExistence(timeout: 5), "Sign in with Google button shows when signed out")
            sleep(1)
            shoot("\(name)-sign-in")

            // 2. Sign in against the fake server (auto-syncs staged articles and remote doc).
            signIn.tap()
            let newsletter = app.buttons["copy-newsletter-address"]
            XCTAssertTrue(newsletter.waitForExistence(timeout: 10), "signed in, Profile has the newsletter address and Copy")
            sleep(1)
            shoot("\(name)-profile-signed-in")
            app.buttons["settings"].tap()
            let signOut = app.buttons["sign-out"]
            XCTAssertTrue(signOut.waitForExistence(timeout: 10), "Sign in succeeds and shows signed-in settings")

            // 3. Connect Readwise (token goes only to the server).
            let rwToken = "rw-iphone-test-token-never-local"
            app.buttons["readwise-connect"].tap()
            let tokenField = app.secureTextFields["readwise-token-field"]
            XCTAssertTrue(tokenField.waitForExistence(timeout: 5))
            tokenField.tap()
            tokenField.typeText(rwToken)
            app.buttons["readwise-submit"].tap()
            XCTAssertTrue(app.buttons["readwise-disconnect"].waitForExistence(timeout: 5), "Readwise connects")

            // 4. Follow two feeds and unfollow one.
            let feedField = app.textFields["feed-url-field"]
            XCTAssertTrue(feedField.waitForExistence(timeout: 5))
            feedField.tap()
            feedField.typeText("https://daringfireball.net/feeds/main")
            app.buttons["follow-feed"].tap()
            XCTAssertTrue(app.buttons["unfollow-feed-daringfireball.net"].waitForExistence(timeout: 5), "first feed appears")

            feedField.tap()
            feedField.typeText("https://temp-feed.example/rss.xml")
            app.buttons["follow-feed"].tap()
            let unfollowTemp = app.buttons["unfollow-feed-temp-feed.example"]
            XCTAssertTrue(unfollowTemp.waitForExistence(timeout: 5), "second feed appears")
            if !unfollowTemp.isHittable || unfollowTemp.frame.maxY > app.frame.height - 190 {
                app.collectionViews.firstMatch.swipeUp()
                sleep(1)
            }
            unfollowTemp.tap()
            XCTAssertTrue(unfollowTemp.waitForNonExistence(timeout: 5), "unfollowed feed is removed")

            app.collectionViews.firstMatch.swipeDown()
            sleep(1)
            shoot("\(name)-settings-account")

            // Verify Readwise token is not stored anywhere in the app's data folder.
            let files = FileManager.default.enumerator(atPath: dir)?.compactMap { $0 as? String } ?? []
            for rel in files where rel != "fake-server.json" {
                let path = "\(dir)/\(rel)"
                if let data = FileManager.default.contents(atPath: path) {
                    XCTAssertFalse(String(decoding: data, as: UTF8.self).contains(rwToken), "Readwise token must never be stored in \(rel)")
                }
            }

            app.navigationBars.buttons.element(boundBy: 0).tap()
            sleep(1)

            // 5. Feed posts, the newsletter and the saved link are in the Inbox, each under its filter.
            tapTab(app, "Inbox")
            XCTAssertTrue(row(app, "Notes on Local-First Software").waitForExistence(timeout: 10), "feed item lands in Inbox")
            XCTAssertTrue(row(app, "Sunday Reading Dispatch").exists, "newsletter lands in Inbox")
            XCTAssertTrue(row(app, "A Field Guide to Quiet Tools").exists, "saved link lands in Inbox")
            app.segmentedControls.buttons["Newsletters"].tap()
            XCTAssertTrue(row(app, "Sunday Reading Dispatch").waitForExistence(timeout: 5) && !row(app, "Notes on Local-First Software").exists, "Newsletters shows only newsletters")
            app.segmentedControls.buttons["Feeds"].tap()
            XCTAssertTrue(row(app, "Notes on Local-First Software").waitForExistence(timeout: 5) && !row(app, "Sunday Reading Dispatch").exists, "Feeds shows only feed posts")
            sleep(1)
            shoot("\(name)-inbox-feeds")
            app.segmentedControls.buttons["All"].tap()

            // 7. Verify remote document arrived in Writing, then write a new document and sync it back to fake-server.json.
            tapTab(app, "Writing")
            XCTAssertTrue(app.buttons["doc-Synced from Mac"].firstMatch.waitForExistence(timeout: 5), "remote document arrives on iPhone")
            app.buttons["new-document"].tap()
            let editor = app.textViews["editor"]
            XCTAssertTrue(editor.waitForExistence(timeout: 10))
            editor.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.25)).tap()
            editor.typeText("# Written on iPhone\n\nRound-trip through the fake server.")
            sleep(2)
            app.navigationBars.buttons.element(boundBy: 0).tap()
            sleep(1)
            tapTab(app, "Profile")
            app.buttons["settings"].firstMatch.tap()
            let syncNow = app.buttons["sync-now"]
            XCTAssertTrue(syncNow.waitForExistence(timeout: 5))
            syncNow.tap()
            sleep(2)

            let serverJSON = try String(contentsOfFile: serverPath, encoding: .utf8)
            XCTAssertTrue(serverJSON.contains("Written on iPhone"), "document written on iPhone round-trips to fake-server.json")

            app.terminate()
        }
    }

    private func seedFakeServer(at path: String) throws {
        let body = String(repeating: "Software that keeps your data on your own device remains fast offline and respects the reader's attention. ", count: 8)
        let feedHTML = """
        <!doctype html><html><head><title>Notes on Local-First Software</title>
        <meta property="og:site_name" content="Ink &amp; Switch"><meta name="author" content="Martin Kleppmann">
        <meta name="description" content="Keeping your writing and reading library on your own devices while syncing quietly."></head>
        <body><article><h1>Notes on Local-First Software</h1><p>\(body)</p></article></body></html>
        """
        let newsletterHTML = """
        <!doctype html><html><head><title>Sunday Reading Dispatch</title>
        <meta property="og:site_name" content="Weekly Letters"><meta name="author" content="Weekly Letters">
        <meta name="description" content="Three essays from the week worth reading without hurry."></head>
        <body><article><h1>Sunday Reading Dispatch</h1><p>\(body)</p></article></body></html>
        """
        let savedHTML = """
        <!doctype html><html><head><title>A Field Guide to Quiet Tools</title>
        <meta property="og:site_name" content="Craft Notes"><meta name="author" content="Toby">
        <meta name="description" content="Why restrained interfaces leave room for thought."></head>
        <body><article><h1>A Field Guide to Quiet Tools</h1><p>\(body)</p></article></body></html>
        """
        let payload: [String: Any] = [
            "documents": [
                "remote-mac-note": [
                    "text": "# Synced from Mac\n\nThis draft crossed the fake server.",
                    "saved_at": 1_700_000_000_000,
                    "device": "mac-demo",
                    "version": 1,
                    "deleted": false,
                ],
            ],
            "files": [
                "incoming/feed-local-first": Data(feedHTML.utf8).base64EncodedString(),
                "incoming/newsletter-sunday": Data(newsletterHTML.utf8).base64EncodedString(),
                "incoming/save-quiet-tools": Data(savedHTML.utf8).base64EncodedString(),
            ],
            "incoming": [
                [
                    "id": "feed-local-first",
                    "source": "feed",
                    "destination": "inbox",
                    "url": "https://inkandswitch.example/local-first",
                    "title": "Notes on Local-First Software",
                    "state": "waiting",
                ],
                [
                    "id": "newsletter-sunday",
                    "source": "newsletter",
                    "destination": "inbox",
                    "url": "https://tobygodat.com/newsletters/sunday",
                    "title": "Sunday Reading Dispatch",
                    "state": "waiting",
                ],
                [
                    "id": "save-quiet-tools",
                    "source": "save",
                    "destination": "queue",
                    "url": "https://craft.example/quiet-tools",
                    "title": "A Field Guide to Quiet Tools",
                    "state": "waiting",
                ],
            ],
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.prettyPrinted])
        try data.write(to: URL(filePath: path))
    }

    /// Measures keystroke-to-painted-frame latency typing 50 characters into a 5,000-word draft on
    /// the Simulator (apples-to-apples with `ios/bench.sh`). Driven by `apple/bench.sh`.
    @MainActor func testTypingBenchmark5000Words() throws {
        guard let dir = environment["INKLING_BENCH_DATA"],
              let probeOut = environment["INKLING_PROBE_OUT"] else {
            throw XCTSkip("apple/bench.sh sets INKLING_BENCH_DATA and INKLING_PROBE_OUT")
        }
        appearance = "light"
        let app = XCUIApplication()
        app.launchEnvironment = [
            "INKLING_DATA_DIR": dir,
            "INKLING_FAKE_AI": "1",
            "INKLING_APPEARANCE": "light",
            "INKLING_PROBE_TYPING": "1",
            "INKLING_PROBE_OUT": probeOut,
        ]
        app.launch()
        let writingTab = app.tabBars.buttons["Writing"]
        XCTAssertTrue(writingTab.waitForExistence(timeout: 10))
        writingTab.tap()
        let benchDoc = app.buttons["doc-Benchmark"].firstMatch
        XCTAssertTrue(benchDoc.waitForExistence(timeout: 5))
        benchDoc.tap()
        let editor = app.textViews["editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 10))
        editor.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.25)).tap()
        sleep(1)

        // Clear any stray sample before typing starts.
        try? FileManager.default.removeItem(atPath: probeOut)
        let typed = "the light held over the river and the light held  "
        for ch in typed {
            editor.typeText(String(ch))
        }
        sleep(1)
        app.terminate()

        let saved = try String(contentsOfFile: "\(dir)/documents/bench.md", encoding: .utf8)
        XCTAssertTrue(saved.localizedCaseInsensitiveContains("the light held over the river and"), "typed text persisted to bench.md")
    }

    /// The place in the reader's bar, the bars brought back first.
    @MainActor private func place(_ app: XCUIApplication) -> String {
        showChrome(app)
        return app.staticTexts["place"].label
    }

    /// Brings the reader's bars back with a tap if reading put them away.
    @MainActor private func showChrome(_ app: XCUIApplication) {
        guard !app.buttons["appearance"].isHittable else { return }
        app.textViews["reader"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        sleep(1)
    }

    /// Opens Appearance, picks Auto paper and `mode` for the app, and closes it again.
    @MainActor private func choose(_ app: XCUIApplication, _ mode: String) {
        app.buttons["appearance"].tap()
        XCTAssertTrue(app.buttons["tone-auto"].waitForExistence(timeout: 5))
        app.buttons["tone-auto"].tap()
        app.segmentedControls.buttons[mode].tap()
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2)).tap()
        sleep(2)
    }

    /// How light the reader's paper is, from 0 to 1, sampled in the margin beside the column.
    @MainActor private func ground() -> CGFloat {
        let image = XCUIScreen.main.screenshot().image
        guard let cg = image.cgImage else { return -1 }
        let x = Int(CGFloat(cg.width) * 0.02), y = cg.height / 2
        guard let pixel = cg.cropping(to: CGRect(x: x, y: y, width: 1, height: 1)) else { return -1 }
        var rgba = [UInt8](repeating: 0, count: 4)
        let context = CGContext(data: &rgba, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        context?.draw(pixel, in: CGRect(x: 0, y: 0, width: 1, height: 1))
        return (0.299 * CGFloat(rgba[0]) + 0.587 * CGFloat(rgba[1]) + 0.114 * CGFloat(rgba[2])) / 255
    }

    /// Share of sampled pixels in `region` (normalised `0...1`) that carry real colour rather than
    /// achromatic paper or ink, used to assert that an inline painting actually drew.
    @MainActor private func colouredShare(in region: CGRect) -> Double {
        guard let cg = XCUIScreen.main.screenshot().image.cgImage else { return 0 }
        let rect = CGRect(
            x: (CGFloat(cg.width) * region.minX).rounded(),
            y: (CGFloat(cg.height) * region.minY).rounded(),
            width: max(1, (CGFloat(cg.width) * region.width).rounded()),
            height: max(1, (CGFloat(cg.height) * region.height).rounded())
        )
        guard let crop = cg.cropping(to: rect) else { return 0 }
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

    @MainActor private func launch(data: String, demo: Bool, devFeedback: Bool = false) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment = ["INKLING_DATA_DIR": data, "INKLING_FAKE_AI": "1", "INKLING_APPEARANCE": appearance]
        if devFeedback {
            app.launchEnvironment["INKLING_DEV_FEEDBACK"] = "1"
        }
        if demo {
            app.launchEnvironment["INKLING_DEMO"] = environment["INKLING_DEMO"]
            app.launchEnvironment["INKLING_FIXTURES"] = environment["INKLING_FIXTURES"]
        }
        app.launch()
        return app
    }

    @MainActor private func tapTab(_ app: XCUIApplication, _ title: String) {
        let target = app.tabBars.buttons[title]
        if !target.waitForExistence(timeout: 2) {
            app.tabBars.buttons.firstMatch.tap()
            if !target.waitForExistence(timeout: 2) {
                app.collectionViews.firstMatch.swipeDown()
                _ = target.waitForExistence(timeout: 3)
            }
        }
        target.tap()
    }

    @MainActor private func row(_ app: XCUIApplication, _ title: String) -> XCUIElement {
        app.buttons["item-\(title)"].firstMatch
    }

    /// Presses and holds a word of the reader's text and chooses Highlight from the edit menu.
    @MainActor private func highlight(_ app: XCUIApplication, in reader: XCUIElement) {
        reader.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.62)).press(forDuration: 1.2)
        let item = app.menuItems["Highlight"].exists ? app.menuItems["Highlight"] : app.buttons["Highlight"]
        XCTAssertTrue(item.waitForExistence(timeout: 5), "the edit menu offers Highlight")
        item.tap()
        sleep(1)
    }

    /// Opens the reader's Highlights menu, counts the passages in it and closes it.
    @MainActor private func highlights(_ app: XCUIApplication) -> Int {
        sleep(1)
        showChrome(app)
        app.buttons["highlights"].tap()
        _ = app.buttons["passage"].waitForExistence(timeout: 3)
        let count = app.buttons.matching(identifier: "passage").count
        if count == 0 { add(XCTAttachment(string: app.debugDescription)) }
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        sleep(1)
        return count
    }

    @MainActor private func shoot(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "iphone-\(name)"
        shot.lifetime = .keepAlways
        add(shot)
    }
}

import XCTest

/// The iPhone app on the Simulator: every screen in Light and Dark, swiping to archive, and a
/// highlight made in the reader that is still there after the app is launched again. Each
/// screenshot is an attachment that apple/snap.sh exports.
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
            shoot("\(name)-queue")

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
            shoot("\(name)-queue-swipe")
            archive.tap()
            XCTAssertFalse(row(app, "Self-Reliance").waitForExistence(timeout: 1), "archiving takes it out of the Queue")

            app.buttons["add"].tap()
            app.buttons["Save Link…"].tap()
            XCTAssertTrue(app.textFields["link"].waitForExistence(timeout: 5))
            shoot("\(name)-save-link")
            app.buttons["Cancel"].tap()

            app.buttons["archive"].tap()
            XCTAssertTrue(row(app, "Self-Reliance").waitForExistence(timeout: 5), "what was archived is in the Archive, a step inside the Queue")
            shoot("\(name)-archive")
            app.navigationBars.buttons.element(boundBy: 0).tap()

            for tab in ["Library", "Inbox", "Writing", "Search"] {
                app.tabBars.buttons[tab].tap()
                sleep(1)
                if tab == "Library" {
                    shoot("\(name)-library")
                    app.buttons["notebook"].tap()
                    sleep(1)
                    shoot("\(name)-notebook")
                    app.navigationBars.buttons.element(boundBy: 0).tap()
                } else {
                    shoot("\(name)-\(tab.lowercased())")
                }
            }
            XCTAssertTrue(app.searchFields.firstMatch.isHittable, "the Search field shows without pulling down")
            XCTAssertTrue(row(app, "Walking").exists, "Search starts from everything in the library")
            leaveSearch(app)
            app.tabBars.buttons["Queue"].tap()

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
            let fix = app.buttons["fix-grammar"]
            XCTAssertTrue(fix.waitForExistence(timeout: 5), "Fix grammar quick action is visible")
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
            let settingsButton = app.buttons["settings"].firstMatch
            XCTAssertTrue(settingsButton.waitForExistence(timeout: 10), "Settings button is reachable")
            settingsButton.tap()

            // 1. Signed-out state.
            let signIn = app.buttons["sign-in-google"]
            XCTAssertTrue(signIn.waitForExistence(timeout: 5), "Sign in with Google button shows when signed out")
            sleep(1)
            shoot("\(name)-sign-in")

            // 2. Sign in against the fake server (auto-syncs staged articles and remote doc).
            signIn.tap()
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
            unfollowTemp.tap()
            XCTAssertFalse(unfollowTemp.waitForExistence(timeout: 2), "unfollowed feed is removed")

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

            app.buttons["settings-done"].tap()
            sleep(1)

            // 5. Verify feed items landed in Inbox and render Inbox in Light and Dark.
            app.tabBars.buttons["Inbox"].tap()
            XCTAssertTrue(row(app, "Notes on Local-First Software").waitForExistence(timeout: 10), "feed item lands in Inbox")
            XCTAssertTrue(row(app, "Sunday Reading Dispatch").exists, "newsletter lands in Inbox")
            sleep(1)
            shoot("\(name)-inbox-feeds")

            // 6. Verify saved link landed in Queue.
            app.tabBars.buttons["Queue"].tap()
            XCTAssertTrue(row(app, "A Field Guide to Quiet Tools").waitForExistence(timeout: 5), "saved link lands in Queue")

            // 7. Verify remote document arrived in Writing, then write a new document and sync it back to fake-server.json.
            app.tabBars.buttons["Writing"].tap()
            XCTAssertTrue(app.buttons["doc-Synced from Mac"].firstMatch.waitForExistence(timeout: 5), "remote document arrives on iPhone")
            app.buttons["new-document"].tap()
            let editor = app.textViews["editor"]
            XCTAssertTrue(editor.waitForExistence(timeout: 10))
            editor.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.25)).tap()
            editor.typeText("# Written on iPhone\n\nRound-trip through the fake server.")
            sleep(2)
            app.navigationBars.buttons.element(boundBy: 0).tap()
            sleep(1)
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

    /// On some iOS versions the Search tab folds the other tabs into one button; tapping it brings
    /// them back.
    @MainActor private func leaveSearch(_ app: XCUIApplication) {
        guard !app.tabBars.buttons["Queue"].waitForExistence(timeout: 1) else { return }
        app.tabBars.buttons.firstMatch.tap()
        XCTAssertTrue(app.tabBars.buttons["Queue"].waitForExistence(timeout: 5), "the tabs come back from Search")
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

    @MainActor private func launch(data: String, demo: Bool) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment = ["INKLING_DATA_DIR": data, "INKLING_FAKE_AI": "1", "INKLING_APPEARANCE": appearance]
        if demo {
            app.launchEnvironment["INKLING_DEMO"] = environment["INKLING_DEMO"]
            app.launchEnvironment["INKLING_FIXTURES"] = environment["INKLING_FIXTURES"]
        }
        app.launch()
        return app
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

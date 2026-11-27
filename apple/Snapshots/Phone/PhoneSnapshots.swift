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

            for tab in ["Library", "Inbox", "Notebook", "Search"] {
                app.tabBars.buttons[tab].tap()
                sleep(1)
                shoot("\(name)-\(tab.lowercased())")
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

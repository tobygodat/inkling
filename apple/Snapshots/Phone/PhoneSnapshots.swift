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

            for tab in ["Library", "Archive", "Inbox", "Notebook"] {
                app.tabBars.buttons[tab].tap()
                sleep(1)
                shoot("\(name)-\(tab.lowercased())")
                if tab == "Archive" { XCTAssertTrue(row(app, "Self-Reliance").exists, "what was archived is in the Archive") }
            }
            app.tabBars.buttons["Queue"].tap()

            app.terminate()
            let again = launch(data: "\(base)/\(name)", demo: false)
            row(again, "An Apology for Idlers").tap()
            XCTAssertTrue(again.textViews["reader"].waitForExistence(timeout: 10))
            XCTAssertEqual(highlights(again), 1, "the highlight is kept through a relaunch")
            again.terminate()
        }
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

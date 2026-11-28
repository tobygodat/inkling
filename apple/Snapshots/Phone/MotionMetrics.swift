import XCTest

/// How smoothly the iPhone app moves, journey by journey: hitches, frame rate and the app's CPU
/// while the reader scrolls a long book and an article with pictures, the lists scroll, a row
/// swipes, screens push and pop, sheets open and close, and the reader's own gestures run.
/// XCTest's hitch metrics report nothing on the Simulator, so the hitches come from the app's
/// `FrameProbe`. `apple/motion.sh` runs it in Release and prints the numbers; `apple/snap.sh`
/// skips it.
nonisolated final class MotionMetrics: XCTestCase {
    private var data: String?

    override func setUpWithError() throws {
        continueAfterFailure = false
        guard let base = ProcessInfo.processInfo.environment["INKLING_MOTION_DATA"] else {
            throw XCTSkip("apple/motion.sh sets INKLING_MOTION_DATA")
        }
        let target = "\(base)/\(name.filter(\.isLetter))"
        let seed = "\(base)/seed"
        let fm = FileManager.default
        if fm.fileExists(atPath: "\(seed)/library/library.json") {
            try? fm.removeItem(atPath: target)
            try fm.copyItem(atPath: seed, toPath: target)
            try? fm.removeItem(atPath: "\(target)/frames")
        }
        data = target
    }

    private var options: XCTMeasureOptions {
        let options = XCTMeasureOptions()
        options.iterationCount = 5
        return options
    }

    /// Nothing moves: the hitches the machine's own load causes.
    @MainActor func testIdle() throws {
        let app = launch()
        XCTAssertTrue(row(app, "Walking").waitForExistence(timeout: 10))
        measure(metrics: metrics(app), options: options) { settle() }
    }

    @MainActor func testReaderScrollsALongBook() throws {
        let app = launch()
        _ = open(app, "Pride")
        measure(metrics: metrics(app), options: options) {
            fling(app, up: true)
            fling(app, up: true)
            fling(app, up: false)
            settle()
        }
        assertOpened("Pride")
    }

    @MainActor func testReaderScrollsAnArticleWithPictures() throws {
        let app = launch()
        _ = open(app, "Walking")
        measure(metrics: metrics(app), options: options) {
            fling(app, up: true)
            fling(app, up: true)
            fling(app, up: false)
            fling(app, up: false)
            settle()
        }
    }

    @MainActor func testListsScroll() throws {
        let app = launch()
        app.tabBars.buttons["Library"].tap()
        XCTAssertTrue(row(app, "Walking").waitForExistence(timeout: 10))
        measure(metrics: metrics(app), options: options) {
            fling(app, up: true)
            fling(app, up: false)
            settle()
        }
    }

    @MainActor func testRowSwipes() throws {
        let app = launch()
        let item = row(app, "Walking")
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        let frame = item.frame
        let at = { (x: CGFloat) in self.spot(app, CGPoint(x: frame.minX + frame.width * x, y: frame.midY)) }
        measure(metrics: metrics(app), options: options) {
            at(0.9).press(forDuration: 0.05, thenDragTo: at(0.6), withVelocity: .slow, thenHoldForDuration: 0)
            settle()
            at(0.4).press(forDuration: 0.05, thenDragTo: at(0.9), withVelocity: .slow, thenHoldForDuration: 0)
            settle()
        }
    }

    @MainActor func testPushAndPop() throws {
        try pushAndPop("Walking")
    }

    /// Opening a long book a third of the way through, where it was left, and going back.
    @MainActor func testOpenALongBookAtItsPlace() throws {
        try pushAndPop("Pride")
        assertOpened("Pride")
    }

    @MainActor func testSheetsOpenAndClose() throws {
        let app = launch()
        let frame = open(app, "Walking")
        let appearance = spot(app, app.buttons["appearance"])
        let above = spot(app, CGPoint(x: frame.midX, y: frame.minY + frame.height * 0.15))
        measure(metrics: metrics(app), options: options) {
            appearance.tap()
            settle()
            above.tap()
            settle()
        }
    }

    @MainActor func testReaderGestures() throws {
        let app = launch()
        let frame = open(app, "Walden")
        let middle = spot(app, CGPoint(x: frame.midX, y: frame.midY))
        let contents = spot(app, app.buttons["contents"])
        app.buttons["contents"].tap()
        let second = app.buttons["Economy"]
        XCTAssertTrue(second.waitForExistence(timeout: 5))
        let first = app.buttons["Where I Lived, and What I Lived For"]
        XCTAssertTrue(first.waitForExistence(timeout: 5))
        let chapters = [spot(app, second), spot(app, first)]
        middle.tap()
        settle()
        var turn = 0
        measure(metrics: metrics(app), options: options) {
            // A slow read on hides the bars, a tap brings them back, and Contents dips to a
            // chapter, back and forth.
            drag(app, up: true, velocity: .slow)
            settle()
            middle.tap()
            settle()
            contents.tap()
            settle()
            chapters[turn % 2].tap()
            turn += 1
            settle()
        }
    }

    // MARK: -

    private func metrics(_ app: XCUIApplication) -> [any XCTMetric] {
        [FrameMetric(path: probe), XCTCPUMetric(application: app)]
    }

    private var probe: String { "\(data ?? "")/frames" }

    /// Lets the motion finish inside the measured block.
    private func settle() {
        Thread.sleep(forTimeInterval: 1.2)
    }

    @MainActor private func launch() -> XCUIApplication {
        let app = XCUIApplication()
        let environment = ProcessInfo.processInfo.environment
        app.launchEnvironment = [
            "INKLING_DATA_DIR": data ?? "",
            "INKLING_FAKE_AI": "1",
            "INKLING_FRAME_PROBE": probe,
            "INKLING_APPEARANCE": "light",
            "INKLING_DEMO": environment["INKLING_DEMO"] ?? "",
            "INKLING_FIXTURES": environment["INKLING_FIXTURES"] ?? "",
        ]
        app.launch()
        return app
    }

    @MainActor private func row(_ app: XCUIApplication, _ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "item-\(title)")).firstMatch
    }

    /// Pushes the reader for `title` and pops back to the list, from points found before the
    /// measuring starts.
    @MainActor private func pushAndPop(_ title: String) throws {
        let app = launch()
        _ = open(app, title)
        let back = spot(app, app.navigationBars.buttons.element(boundBy: 0))
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let item = row(app, title)
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        let tap = spot(app, item)
        settle()
        measure(metrics: metrics(app), options: options) {
            tap.tap()
            settle()
            settle()
            back.tap()
            settle()
        }
    }

    /// Touches delivered through SpringBoard hit the foreground app by screen coordinates without
    /// triggering `XCTElementSnapshotRequest` on Inkling's `UITextView` (whose `accessibilityElements`
    /// is O(paragraphs × characters) on long books and otherwise pollutes or hangs measured blocks).
    @MainActor private var screen: XCUIApplication {
        XCUIApplication(bundleIdentifier: "com.apple.springboard")
    }

    /// A point on screen, found once, so the measured runs never query the app's elements: a query
    /// makes the app describe every element on its main thread, the whole text of a book included.
    @MainActor private func spot(_ app: XCUIApplication, _ element: XCUIElement) -> XCUICoordinate {
        spot(app, CGPoint(x: element.frame.midX, y: element.frame.midY))
    }

    @MainActor private func spot(_ app: XCUIApplication, _ point: CGPoint) -> XCUICoordinate {
        screen.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: point.x, dy: point.y))
    }

    /// A quick flick that lets go while moving, so the list or the text coasts on.
    @MainActor private func fling(_ app: XCUIApplication, up: Bool) {
        drag(app, up: up, velocity: .fast)
    }

    @MainActor private func drag(_ app: XCUIApplication, up: Bool, velocity: XCUIGestureVelocity) {
        let low = screen.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.75))
        let high = screen.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.3))
        (up ? low : high).press(forDuration: 0.01, thenDragTo: up ? high : low, withVelocity: velocity, thenHoldForDuration: 0)
    }

    @MainActor private func open(_ app: XCUIApplication, _ title: String) -> CGRect {
        if title == "Pride" || title == "Walden" { app.tabBars.buttons["Library"].tap() }
        let item = row(app, title)
        XCTAssertTrue(item.waitForExistence(timeout: 10))
        item.tap()
        XCTAssertTrue(app.buttons["appearance"].waitForExistence(timeout: 20))
        sleep(2)
        return app.frame
    }

    /// Confirms the long book actually opened and wrote its `opened` timestamp to `library.json`.
    private func assertOpened(_ titlePrefix: String) {
        guard let dir = data,
              let raw = try? Data(contentsOf: URL(filePath: "\(dir)/library/library.json")),
              let json = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
              let entries = json["books"] as? [[String: Any]] else {
            XCTFail("library.json missing after opening \(titlePrefix)")
            return
        }
        let opened = entries.first { ($0["title"] as? String)?.hasPrefix(titlePrefix) == true }?["opened"] as? NSNumber
        XCTAssertGreaterThan(opened?.intValue ?? 0, 0, "\(titlePrefix) was opened in the reader")
    }
}

/// The frames `FrameProbe` wrote during one measured run: how many began on time, how many began
/// late (a hitch: later than half a frame after it was due) and by how much in all, and the longest
/// gap between two frames.
nonisolated final class FrameMetric: NSObject, XCTMetric {
    let path: String

    init(path: String) {
        self.path = path
    }

    func copy(with zone: NSZone? = nil) -> Any { FrameMetric(path: path) }

    func reportMeasurements(from start: XCTPerformanceMeasurementTimestamp, to end: XCTPerformanceMeasurementTimestamp) throws -> [XCTPerformanceMeasurement] {
        let from = Double(start.absoluteTimeNanoSeconds) / 1e9, to = Double(end.absoluteTimeNanoSeconds) / 1e9
        let values = try Data(contentsOf: URL(filePath: path)).withUnsafeBytes { Array($0.bindMemory(to: Double.self)) }
        let frames = stride(from: 0, to: values.count - 1, by: 2)
            .map { (began: values[$0], due: values[$0 + 1]) }
            .filter { $0.began >= from && $0.began <= to }
        var late = 0.0, hitches = 0, longest = 0.0
        for (frame, next) in zip(frames, frames.dropFirst()) {
            let period = frame.due - frame.began
            longest = max(longest, next.began - frame.began)
            if next.began - frame.due > period / 2 {
                hitches += 1
                late += next.began - frame.due
            }
        }
        let span = max(to - from, 0.001)
        return [
            XCTPerformanceMeasurement(identifier: "inkling.frames.hitch-ratio", displayName: "Hitch time ratio", doubleValue: late * 1000 / span, unitSymbol: "ms per s"),
            XCTPerformanceMeasurement(identifier: "inkling.frames.hitches", displayName: "Hitches", doubleValue: Double(hitches), unitSymbol: "count"),
            XCTPerformanceMeasurement(identifier: "inkling.frames.longest", displayName: "Longest frame", doubleValue: longest * 1000, unitSymbol: "ms"),
            XCTPerformanceMeasurement(identifier: "inkling.frames.rate", displayName: "Frame rate", doubleValue: Double(frames.count) / span, unitSymbol: "fps"),
        ]
    }
}

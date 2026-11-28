import Testing

@testable import InklingCore

/// Appends at uneven, sometimes out-of-order times over a minute and checks after every append
/// that the buffer holds exactly what was appended in the 10 seconds up to the newest time.
@Test func theBufferKeepsOnlyTheLastTenSeconds() {
    var buffer = RecentBuffer<Int>(span: 10)
    var appended: [(time: Double, element: Int)] = []
    var generator = SystemRandomNumberGenerator()
    var time = 100.0
    for n in 0..<2_000 {
        time += Double.random(in: 0...0.06, using: &generator)
        let at = n.isMultiple(of: 7) ? time - Double.random(in: 0...0.2, using: &generator) : time
        buffer.append(n, at: at)
        appended.append((at, n))
        let newest = appended.map(\.time).max()!
        let expected = appended.filter { $0.time >= newest - 10 }.sorted { $0.time < $1.time }
        #expect(buffer.entries.map(\.element).sorted() == expected.map(\.element).sorted())
        #expect(buffer.entries.map(\.time) == expected.map(\.time))
    }
    #expect(time - 100 > 50, "the run covers more than five spans")

    buffer.trim(to: time + 9)
    #expect(buffer.entries.allSatisfy { $0.time >= time - 1 })
    buffer.trim(to: time + 11)
    #expect(buffer.entries.isEmpty, "nothing older than 10 seconds is kept")
}

@Test func theHandleShowsOnDevBuildsOnly() {
    #expect(devFeedbackShows(debug: true, bundle: "com.tobygodat.inkling.native", environment: [:]))
    #expect(devFeedbackShows(debug: false, bundle: "com.tobygodat.inkling.dev", environment: [:]))
    #expect(!devFeedbackShows(debug: false, bundle: "com.tobygodat.inkling.native", environment: [:]))
    #expect(!devFeedbackShows(debug: false, bundle: "com.tobygodat.inkling", environment: [:]))
    #expect(!devFeedbackShows(debug: true, bundle: "com.tobygodat.inkling.dev", environment: ["INKLING_FAKE_AI": "1"]))
    #expect(devFeedbackShows(debug: false, bundle: nil, environment: ["INKLING_FAKE_AI": "1", "INKLING_DEV_FEEDBACK": "1"]))
}

import Foundation

/// What was appended in the last `span` seconds, oldest first. The dev builds' feedback keeps the
/// screen and the touches in two of these, so a clip holds only the 10 seconds before it was asked
/// for (TOB-571). Times are seconds on one monotonic clock, such as `ProcessInfo.systemUptime`.
public struct RecentBuffer<Element> {
    public let span: Double
    public private(set) var entries: [(time: Double, element: Element)] = []

    public init(span: Double) {
        self.span = span
    }

    /// Adds `element` at `time`, in time order, then drops what is more than `span` older than
    /// the newest entry.
    public mutating func append(_ element: Element, at time: Double) {
        let index = entries.lastIndex { $0.time <= time }.map { $0 + 1 } ?? 0
        entries.insert((time, element), at: index)
        trim(to: entries[entries.count - 1].time)
    }

    /// Drops what is more than `span` older than `now`.
    public mutating func trim(to now: Double) {
        let oldest = now - span
        entries.removeFirst(entries.firstIndex { $0.time >= oldest } ?? entries.count)
    }

    public mutating func removeAll() {
        entries.removeAll()
    }
}

extension RecentBuffer: Sendable where Element: Sendable {}

/// Whether the dev feedback handle shows: on a Debug build or the Inkling Dev bundle, never on a
/// Release build under another id. A headless run (`INKLING_FAKE_AI`) shows it only when it sets
/// `INKLING_DEV_FEEDBACK`, so the other snapshots and measures stay as they are.
public func devFeedbackShows(debug: Bool, bundle: String?, environment: [String: String]) -> Bool {
    if environment["INKLING_FAKE_AI"] != nil {
        return environment["INKLING_DEV_FEEDBACK"] != nil
    }
    return debug || bundle == "com.tobygodat.inkling.dev"
}

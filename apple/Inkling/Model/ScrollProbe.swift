#if os(iOS)
import UIKit

/// Writes, on every frame, where the reader's text sits on screen: the scroll offset, the view's
/// top in the window and one character's line in the layout and on screen. A jump or a drift is a
/// frame where that line moves on screen by more than the scroll moved it. Only a launch with
/// `INKLING_SCROLL_PROBE` set, always a headless run, starts it.
final class ScrollProbe {
    private static var running: ScrollProbe?
    private let file: FileHandle
    private var link: CADisplayLink?
    private weak var view: UITextView?
    private var anchor: Int?
    private var lines: [String] = []

    static var isAsked: Bool { ProcessInfo.processInfo.environment["INKLING_SCROLL_PROBE"] != nil }

    static func watch(_ view: UITextView) {
        if running == nil, let path = ProcessInfo.processInfo.environment["INKLING_SCROLL_PROBE"],
           FileManager.default.createFile(atPath: path, contents: nil),
           let file = FileHandle(forWritingAtPath: path) {
            let probe = ScrollProbe(file: file)
            let link = CADisplayLink(target: probe, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            probe.link = link
            running = probe
        }
        running?.view = view
        running?.anchor = nil
    }

    private init(file: FileHandle) {
        self.file = file
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let view, view.window != nil, let layout = view.textLayoutManager else { return }
        let offset = view.contentOffset.y
        let top = view.convert(CGPoint.zero, to: nil).y + offset
        func fragmentTop(_ character: Int) -> CGFloat? {
            guard let location = layout.location(layout.documentRange.location, offsetBy: character) else { return nil }
            return layout.textLayoutFragment(for: location)?.layoutFragmentFrame.minY
        }
        var fresh = 0
        if let at = anchor, let y = fragmentTop(at), y - offset > 0, y - offset < view.bounds.height {
        } else {
            let middle = CGPoint(x: 1, y: offset + view.bounds.height / 2)
            anchor = layout.textLayoutFragment(for: middle).map { layout.offset(from: layout.documentRange.location, to: $0.rangeInElement.location) }
            fresh = 1
        }
        guard let at = anchor, let y = fragmentTop(at) else { return }
        let screen = top + view.textContainerInset.top + y - offset
        let state = view.isTracking ? "T" : view.isDecelerating ? "D" : "-"
        lines.append(String(format: "%.4f,%.2f,%.2f,%.2f,%d,%.2f,%.2f,%d,%@,%.0f\n", link.timestamp, offset, view.adjustedContentInset.top, top, at, y, screen, fresh, state, view.contentSize.height))
        guard lines.count >= 30 else { return }
        file.write(Data(lines.joined().utf8))
        lines.removeAll(keepingCapacity: true)
    }
}
#endif

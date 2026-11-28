#if os(iOS)
import UIKit

/// Writes when each frame's main-thread work began and when it was due, so `apple/motion.sh` can
/// count the frames that came late. XCTest's own hitch metrics need a device, so on the Simulator
/// this is the measure. Only a launch with `INKLING_FRAME_PROBE` set, always a headless run, starts
/// it.
final class FrameProbe {
    private static var running: FrameProbe?
    static var isActive: Bool { running != nil }
    private let file: FileHandle
    private let writer = DispatchQueue(label: "inkling.frame-probe")
    private var frames: [Double] = []
    private var link: CADisplayLink?

    static func startIfAsked() {
        guard running == nil, let path = ProcessInfo.processInfo.environment["INKLING_FRAME_PROBE"],
              (try? FileManager.default.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)) != nil,
              FileManager.default.createFile(atPath: path, contents: nil),
              let file = FileHandle(forWritingAtPath: path) else { return }
        let probe = FrameProbe(file: file)
        let link = CADisplayLink(target: probe, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        probe.link = link
        running = probe
    }

    private init(file: FileHandle) {
        self.file = file
    }

    /// Each frame as two doubles in CACurrentMediaTime's clock: when it began and when the next
    /// one was due.
    @objc private func tick(_ link: CADisplayLink) {
        frames.append(link.timestamp)
        frames.append(link.targetTimestamp)
        guard frames.count >= 60 else { return }
        let batch = frames
        frames.removeAll(keepingCapacity: true)
        let file = file
        writer.async { batch.withUnsafeBytes { file.write(Data($0)) } }
    }
}
#endif

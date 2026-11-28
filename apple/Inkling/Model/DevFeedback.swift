#if os(iOS)
import AVFoundation
import ImageIO
import InklingCore
import Observation
import ReplayKit
import UIKit
import UniformTypeIdentifiers
import VideoToolbox

#if DEBUG
private let debugBuild = true
#else
private let debugBuild = false
#endif

/// The dev builds' feedback (TOB-571). While the app is active it keeps the last 10 seconds of the
/// screen and every touch on this iPhone. Opening the panel freezes them; "Attach the last 10
/// seconds" makes them a clip with the touches drawn on, still on this iPhone, and only Send
/// uploads anything.
///
/// On an iPhone the screen is ReplayKit's rolling clip buffer: the system records it, so the app
/// draws nothing extra per frame, and iOS asks once per launch whether Inkling Dev may record the
/// screen. The Simulator can't record, so there the window draws itself into a ring of JPEGs
/// five times a second, which costs the app a frame each time.
@Observable
final class DevFeedback {
    static let shows = devFeedbackShows(debug: debugBuild, bundle: Bundle.main.bundleIdentifier, environment: ProcessInfo.processInfo.environment)
    private static let span = 10.0

    enum Phase: Equatable {
        case writing, sending, sent
        case failed(String)
    }

    private(set) var open = false
    var text = ""
    var attach = false {
        didSet { if attach != oldValue { attachChanged() } }
    }
    private(set) var phase: Phase = .writing
    /// The clip's last frame, shown while Attach is on.
    private(set) var preview: UIImage?
    private(set) var clip: ClipFile?
    private(set) var preparing = false
    /// Why the clip couldn't be made.
    private(set) var clipProblem: String?

    @ObservationIgnored private var touches = RecentBuffer<TouchSample>(span: span)
    @ObservationIgnored private var frozen: Task<Frozen, Never>?
    @ObservationIgnored private weak var window: UIWindow?
    @ObservationIgnored private var screen: ScreenSource?
    @ObservationIgnored private var making: Task<ClipFile?, Never>?
    /// The id the text was first sent with, so a retry of the same words is kept once.
    @ObservationIgnored private var attempt: (id: String, text: String)?

    /// Starts keeping the last 10 seconds of `window` and its touches.
    func watch(_ window: UIWindow) {
        guard self.window !== window else { return }
        self.window = window
        window.addGestureRecognizer(TouchLog { [weak self] touch, phase in self?.record(touch, phase) })
        #if targetEnvironment(simulator)
        screen = WindowFrames(window, span: Self.span)
        #else
        screen = SystemRecording()
        #endif
        screen?.start()
    }

    private func record(_ touch: UITouch, _ phase: TouchSample.Phase) {
        guard !open, let window else { return }
        touches.append(TouchSample(touch: ObjectIdentifier(touch).hashValue, point: touch.location(in: window), phase: phase), at: touch.timestamp)
    }

    func show() {
        guard let window else { return }
        let end = ProcessInfo.processInfo.systemUptime
        touches.trim(to: end)
        let touches = touches.entries
        let size = window.bounds.size
        let screen = screen
        frozen = Task { Frozen(screen: await screen?.freeze(at: end) ?? .none, touches: touches, end: end, size: size) }
        if phase == .sent {
            phase = .writing
        }
        open = true
        if attach {
            makeClip()
        }
    }

    func hide() {
        open = false
        if phase == .sent {
            reset()
        }
    }

    private func attachChanged() {
        if attach {
            makeClip()
        } else {
            dropClip()
        }
    }

    private func makeClip() {
        dropClip()
        guard let frozen else { return }
        preparing = true
        let url = FileManager.default.temporaryDirectory.appending(path: "feedback-\(UUID().uuidString).mp4")
        let task = Task { () -> Result<ClipFile, any Error> in
            do {
                return .success(try await ClipWriter.write(await frozen.value, to: url))
            } catch {
                return .failure(error)
            }
        }
        making = Task { try? await task.value.get() }
        let current = making
        Task {
            let made = await task.value
            guard making == current else { return }
            preparing = false
            switch made {
            case .success(let file):
                clip = file
                preview = UIImage(data: file.preview)
            case .failure(let error):
                clipProblem = error.localizedDescription
            }
        }
    }

    private func dropClip() {
        making?.cancel()
        making = nil
        preparing = false
        preview = nil
        clipProblem = nil
        if let clip {
            try? FileManager.default.removeItem(at: clip.url)
        }
        clip = nil
    }

    private func reset() {
        text = ""
        attach = false
        dropClip()
        attempt = nil
        frozen = nil
        phase = .writing
    }

    /// Where the app was when the panel opened: the screen, and the item or document in words.
    static func whereAt(_ shelf: Shelf, _ navigator: Navigator) -> (screen: String, place: String) {
        let section = navigator.section
        switch shelf.asker.place {
        case .reading(let model) where (section == .inbox && !navigator.inboxPath.isEmpty) || (section == .library && !navigator.libraryPath.isEmpty):
            return ("reader", "\(section.title) › \(model.item.title), \(model.placeText)")
        case .writing(let model) where section == .writing:
            return ("writer", "Writing › document \(model.id)")
        default:
            return (section.rawValue, section.title)
        }
    }

    func send(from shelf: Shelf, at navigator: Navigator) {
        let words = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty, phase != .sending, let window else { return }
        phase = .sending
        let id = attempt.flatMap { $0.text == words ? $0.id : nil } ?? newFeedbackId()
        attempt = (id, words)
        let (screen, place) = Self.whereAt(shelf, navigator)
        let info = Bundle.main.infoDictionary ?? [:]
        let record = FeedbackRecord(
            id: id,
            text: words,
            formFactor: "phone",
            windowWidth: UInt32(window.bounds.width),
            windowHeight: UInt32(window.bounds.height),
            screen: screen,
            place: place,
            version: info["CFBundleShortVersionString"] as? String ?? "",
            build: info["CFBundleVersion"] as? String ?? "",
            bundle: Bundle.main.bundleIdentifier ?? ""
        )
        let client = shelf.account.client
        let making = attach ? making : nil
        Task {
            let clip = await making?.value
            if attach && clip == nil {
                phase = .failed("The clip couldn’t be made. Turn Attach off to send without it.")
                return
            }
            let attached = clip.map { FeedbackClip(path: $0.url.path, seconds: $0.seconds, touches: UInt32($0.touches)) }
            do {
                try await Shelf.offMain { try client.sendFeedback(feedback: record, clip: attached) }
                phase = .sent
                if let kept = self.clip { try? FileManager.default.removeItem(at: kept.url) }
                self.clip = nil
                text = ""
                attempt = nil
            } catch {
                phase = .failed(Shelf.words(error))
            }
        }
    }
}

/// One sample of a touch, in window points.
nonisolated struct TouchSample: Sendable {
    enum Phase: Sendable { case began, moved, ended }
    let touch: Int
    let point: CGPoint
    let phase: Phase
}

/// The screen's last seconds as the panel froze them.
nonisolated enum FrozenScreen: Sendable {
    /// Window frames as JPEG, each with its time.
    case frames([(time: Double, element: Data)])
    /// A recording that ends when the panel opened.
    case recording(URL)
    case none
}

/// What the panel froze when it opened.
nonisolated struct Frozen: Sendable {
    let screen: FrozenScreen
    let touches: [(time: Double, element: TouchSample)]
    let end: Double
    /// The window, in points: the clip's size.
    let size: CGSize
}

nonisolated struct ClipFile: Sendable, Equatable {
    let url: URL
    let seconds: Double
    let touches: Int
    /// The last frame as JPEG.
    let preview: Data
}

/// Where the screen's last seconds come from.
private protocol ScreenSource {
    func start()
    func freeze(at end: Double) async -> FrozenScreen
}

/// ReplayKit's rolling clip buffer. iOS asks the first time in each launch, and again after 8
/// minutes in the background. Turned down, the clip carries the touches alone.
private final class SystemRecording: ScreenSource {
    private var observer: NSObjectProtocol?
    private var buffering = false

    func start() {
        buffer()
        observer = NotificationCenter.default.addObserver(forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.buffer() }
        }
    }

    private func buffer() {
        let recorder = RPScreenRecorder.shared()
        guard recorder.isAvailable, !recorder.isRecording else { return }
        recorder.isMicrophoneEnabled = false
        recorder.startClipBuffering { [weak self] error in
            Task { @MainActor in self?.buffering = error == nil }
        }
    }

    func freeze(at end: Double) async -> FrozenScreen {
        let recorder = RPScreenRecorder.shared()
        guard buffering else { return .none }
        let url = FileManager.default.temporaryDirectory.appending(path: "feedback-screen-\(UUID().uuidString).mp4")
        do {
            try await recorder.exportClip(to: url, duration: 10)
            return .recording(url)
        } catch {
            return .none
        }
    }
}

/// The window drawn into a ring of JPEGs five times a second, for the Simulator.
private final class WindowFrames: ScreenSource {
    private weak var window: UIWindow?
    private var frames: RecentBuffer<Data>
    private var timer: Timer?
    private var renderer: (bounds: CGRect, renderer: UIGraphicsImageRenderer)?

    init(_ window: UIWindow, span: Double) {
        self.window = window
        frames = RecentBuffer(span: span)
    }

    func start() {
        let timer = Timer(timeInterval: 0.2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.capture() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func capture() {
        guard let window, UIApplication.shared.applicationState == .active else { return }
        let bounds = window.bounds
        if renderer?.bounds != bounds {
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            format.opaque = true
            renderer = (bounds, UIGraphicsImageRenderer(bounds: bounds, format: format))
        }
        guard let renderer = renderer?.renderer else { return }
        let image = renderer.image { _ in _ = window.drawHierarchy(in: bounds, afterScreenUpdates: false) }
        let time = ProcessInfo.processInfo.systemUptime
        Task.detached(priority: .utility) { [weak self] in
            guard let jpeg = image.jpegData(compressionQuality: 0.6) else { return }
            await self?.keep(jpeg, at: time)
        }
    }

    private func keep(_ jpeg: Data, at time: Double) {
        frames.append(jpeg, at: time)
    }

    func freeze(at end: Double) async -> FrozenScreen {
        frames.trim(to: end)
        return .frames(frames.entries.filter { $0.time <= end })
    }
}

/// Sees every touch in the window without taking any: it never recognizes, never delays and
/// recognizes alongside every other gesture.
private final class TouchLog: UIGestureRecognizer, UIGestureRecognizerDelegate {
    private let seen: (UITouch, TouchSample.Phase) -> Void

    init(_ seen: @escaping (UITouch, TouchSample.Phase) -> Void) {
        self.seen = seen
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
        delegate = self
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        touches.forEach { seen($0, .began) }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        touches.forEach { seen($0, .moved) }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        touches.forEach { seen($0, .ended) }
        settle(event)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        touches.forEach { seen($0, .ended) }
        settle(event)
    }

    /// Fails once every finger is up, so it starts again at the next touch.
    private func settle(_ event: UIEvent) {
        let down = event.allTouches?.contains { $0.phase != .ended && $0.phase != .cancelled } ?? false
        if !down { state = .failed }
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        true
    }
}

/// Makes the frozen screen an MP4 of the window's size in points at 15 frames a second, with each
/// touch drawn where it was: a dot while the finger is down and a short trail behind it.
private nonisolated enum ClipWriter {
    static let fps: Int32 = 15
    static let trail = 0.35

    struct Stroke {
        var samples: [(time: Double, point: CGPoint)]
        var ended: Double?
    }

    @concurrent static func write(_ frozen: Frozen, to url: URL) async throws -> ClipFile {
        var source = try await FrameSource(frozen)
        let start = max(source.start ?? frozen.touches.first?.time ?? frozen.end, frozen.end - 10)
        guard start < frozen.end else { throw CocoaError(.fileReadNoSuchFile) }
        let width = Int(frozen.size.width) & ~1
        let height = Int(frozen.size.height) & ~1
        let strokes = strokes(frozen.touches)
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 1_200_000],
        ])
        input.expectsMediaDataInRealTime = false
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
        ])
        writer.add(input)
        guard writer.startWriting() else { throw writer.error ?? CocoaError(.fileWriteUnknown) }
        writer.startSession(atSourceTime: .zero)

        var last: CGImage?
        let count = max(1, Int((frozen.end - start) * Double(fps)))
        for i in 0..<count {
            try Task.checkCancellation()
            let t = start + Double(i) / Double(fps)
            let image = source.image(at: t)
            while !input.isReadyForMoreMediaData { try await Task.sleep(for: .milliseconds(5)) }
            guard let pool = adaptor.pixelBufferPool else { throw writer.error ?? CocoaError(.fileWriteUnknown) }
            var made: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &made)
            guard let buffer = made else { throw CocoaError(.fileWriteOutOfSpace) }
            last = draw(image, strokes: strokes, at: t, into: buffer, width: width, height: height)
            adaptor.append(buffer, withPresentationTime: CMTime(value: CMTimeValue(i), timescale: fps))
        }
        input.markAsFinished()
        await writer.finishWriting()
        if case .recording(let screen) = frozen.screen { try? FileManager.default.removeItem(at: screen) }
        guard writer.status == .completed else { throw writer.error ?? CocoaError(.fileWriteUnknown) }
        let touched = strokes.filter { $0.samples[0].time >= start }.count
        return ClipFile(url: url, seconds: frozen.end - start, touches: touched, preview: last.flatMap(jpeg) ?? Data())
    }

    /// A recording being read, and where its first frame falls on the touches' clock.
    struct Recording {
        let reader: AVAssetReader
        let output: AVAssetReaderTrackOutput
        let start: Double
        var first: Double?
    }

    /// The screen to show at each time, asked for in order.
    struct FrameSource {
        private var frames: [(time: Double, element: Data)] = []
        private var shown = -1
        private var reader: Recording?
        private var image: CGImage?
        /// When the screen's frames begin, as a time on the touches' clock.
        private(set) var start: Double?

        init(_ frozen: Frozen) async throws {
            switch frozen.screen {
            case .frames(let frames):
                self.frames = frames
                start = frames.first?.time
            case .recording(let url):
                let asset = AVURLAsset(url: url)
                guard let track = try await asset.loadTracks(withMediaType: .video).first else { return }
                let length = try await asset.load(.duration).seconds
                let output = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
                let reader = try AVAssetReader(asset: asset)
                reader.add(output)
                guard reader.startReading() else { return }
                self.reader = Recording(reader: reader, output: output, start: frozen.end - length)
                start = frozen.end - length
            case .none:
                break
            }
        }

        mutating func image(at t: Double) -> CGImage? {
            if let reader {
                while let sample = reader.output.copyNextSampleBuffer() {
                    let pts = sample.presentationTimeStamp.seconds
                    let first = self.reader?.first ?? pts
                    self.reader?.first = first
                    let at = reader.start + pts - first
                    if let pixels = sample.imageBuffer {
                        var made: CGImage?
                        VTCreateCGImageFromCVPixelBuffer(pixels, options: nil, imageOut: &made)
                        image = made ?? image
                    }
                    if at >= t { break }
                }
                return image
            }
            var next = max(shown, 0)
            while next + 1 < frames.count, frames[next + 1].time <= t { next += 1 }
            if next != shown, next < frames.count {
                shown = next
                image = CGImageSourceCreateWithData(frames[next].element as CFData, nil).flatMap { CGImageSourceCreateImageAtIndex($0, 0, nil) }
            }
            return image
        }
    }

    static func jpeg(_ image: CGImage) -> Data? {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.jpeg.identifier as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.7] as CFDictionary)
        return CGImageDestinationFinalize(destination) ? data as Data : nil
    }

    /// Each touch from its first sample to its last.
    static func strokes(_ touches: [(time: Double, element: TouchSample)]) -> [Stroke] {
        var strokes: [Stroke] = []
        var current: [Int: Int] = [:]
        for (time, sample) in touches {
            if sample.phase == .began || current[sample.touch] == nil {
                current[sample.touch] = strokes.count
                strokes.append(Stroke(samples: []))
            }
            guard let index = current[sample.touch] else { continue }
            strokes[index].samples.append((time, sample.point))
            if sample.phase == .ended {
                strokes[index].ended = time
                current[sample.touch] = nil
            }
        }
        return strokes
    }

    /// Draws one frame and gives it back as an image, for the preview.
    static func draw(_ image: CGImage?, strokes: [Stroke], at t: Double, into buffer: CVPixelBuffer, width: Int, height: Int) -> CGImage? {
        CVPixelBufferLockBaseAddress(buffer, [])
        defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
        guard let context = CGContext(
            data: CVPixelBufferGetBaseAddress(buffer), width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        ) else { return nil }
        let frame = CGRect(x: 0, y: 0, width: width, height: height)
        context.setFillColor(CGColor(gray: 0.97, alpha: 1))
        context.fill(frame)
        if let image {
            context.interpolationQuality = .medium
            context.draw(image, in: frame)
        }
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 1, y: -1)
        context.setLineCap(.round)
        context.setLineJoin(.round)
        let blue = CGColor(red: 0.04, green: 0.41, blue: 0.85, alpha: 0.9)
        for stroke in strokes {
            guard let began = stroke.samples.first?.time, began <= t, (stroke.ended ?? .infinity) + 0.15 >= t else { continue }
            let past = stroke.samples.filter { $0.time <= t }
            guard let now = past.last?.point else { continue }
            let tail = past.filter { $0.time >= t - trail }.map(\.point)
            if tail.count > 1 {
                for (color, lineWidth) in [(CGColor(gray: 1, alpha: 0.7), 7.0), (blue, 3.5)] {
                    context.setStrokeColor(color)
                    context.setLineWidth(lineWidth)
                    context.addLines(between: tail)
                    context.strokePath()
                }
            }
            let dot = CGRect(x: now.x - 14, y: now.y - 14, width: 28, height: 28)
            context.setFillColor(blue.copy(alpha: 0.35)!)
            context.fillEllipse(in: dot)
            context.setStrokeColor(CGColor(gray: 1, alpha: 0.9))
            context.setLineWidth(2.5)
            context.strokeEllipse(in: dot)
        }
        return context.makeImage()
    }
}
#endif

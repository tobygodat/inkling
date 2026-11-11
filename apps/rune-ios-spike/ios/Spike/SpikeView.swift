import QuartzCore
import UIKit
import os

let log = Logger(subsystem: "com.tobygodat.inkling.runespike", category: "bridge")

/// A UTF-16 offset into the document UIKit sees.
final class Pos: UITextPosition {
    let offset: Int
    init(_ offset: Int) { self.offset = offset }
}

final class Span: UITextRange {
    let lo: Int
    let hi: Int
    init(_ a: Int, _ b: Int) { lo = min(a, b); hi = max(a, b) }
    override var start: UITextPosition { Pos(lo) }
    override var end: UITextPosition { Pos(hi) }
    override var isEmpty: Bool { lo == hi }
}

/// Hosts one Rune view on a `CAMetalLayer` and is the keyboard's text input client. All text,
/// selection and marked text live in Rune's `TextEdit`; this view only translates.
final class SpikeView: UIView, UITextInput {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var host: OpaquePointer?
    private var link: CADisplayLink?
    private var metalLayer: CAMetalLayer { layer as! CAMetalLayer }
    private var scale: CGFloat { window?.screen.scale ?? traitCollection.displayScale }

    /// Rune's state stamp as UIKit last knew it. When Rune changes the text or caret on its
    /// own (a tap), UIKit's input delegate must hear it or autocorrect works on stale text.
    private var knownRevision: UInt64 = 0
    /// True while UIKit itself is calling in, so the change it makes isn't echoed back.
    private var inUIKitCall = false

    weak var inputDelegate: UITextInputDelegate?
    lazy var tokenizer: UITextInputTokenizer = UITextInputStringTokenizer(textInput: self)
    var markedTextStyle: [NSAttributedString.Key: Any]?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = true
        metalLayer.isOpaque = true
        metalLayer.pixelFormat = .bgra8Unorm
        isAccessibilityElement = true
        accessibilityLabel = "Editor"
    }

    override var accessibilityValue: String? {
        get { String(decoding: document, as: UTF16.self) }
        set {}
    }

    required init?(coder: NSCoder) { fatalError() }

    deinit {
        link?.invalidate()
        spike_free(host)
    }

    // MARK: Hosting

    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil, bounds.width > 0, bounds.height > 0 else { return }
        metalLayer.contentsScale = scale
        let size = bounds.size
        if let host {
            spike_resize(host, Float(size.width), Float(size.height), Float(scale))
        } else {
            createHost(size)
        }
        sendInsets()
        resume()
    }

    override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        sendInsets()
        resume()
    }

    private func sendInsets() {
        let i = safeAreaInsets
        spike_set_insets(host, Float(i.top), Float(i.right), Float(i.bottom), Float(i.left))
    }

    private func createHost(_ size: CGSize) {
        let context = Unmanaged.passUnretained(self).toOpaque()
        host = spike_new(
            Unmanaged.passUnretained(metalLayer).toOpaque(),
            Float(size.width), Float(size.height), Float(scale),
            { context in
                guard let context else { return }
                DispatchQueue.main.async {
                    Unmanaged<SpikeView>.fromOpaque(context).takeUnretainedValue().resume()
                }
            },
            context
        )
        guard host != nil else {
            log.error("spike_new failed")
            return
        }
        knownRevision = spike_revision(host)
        let link = CADisplayLink(target: LinkTarget(self), selector: #selector(LinkTarget.tick))
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    func resume() { link?.isPaused = false }

    fileprivate func tick() {
        guard let host else { return }
        var wake = -1.0
        if !spike_frame(host, &wake) {
            link?.isPaused = true
            if wake >= 0 {
                DispatchQueue.main.asyncAfter(deadline: .now() + wake) { [weak self] in self?.resume() }
            }
        }
        syncFromRune()
    }

    /// Tells UIKit about a change Rune made by itself.
    private func syncFromRune() {
        guard !inUIKitCall, let host else { return }
        let rev = spike_revision(host)
        guard rev != knownRevision else { return }
        inputDelegate?.selectionWillChange(self)
        inputDelegate?.textWillChange(self)
        knownRevision = rev
        inputDelegate?.textDidChange(self)
        inputDelegate?.selectionDidChange(self)
    }

    /// Runs a call UIKit made into the text, then records Rune's new stamp without echoing it.
    private func edit(_ name: StaticString, _ body: (OpaquePointer) -> Void) {
        guard let host else { return }
        inUIKitCall = true
        body(host)
        inUIKitCall = false
        knownRevision = spike_revision(host)
        log.debug("\(name, privacy: .public) -> \(self.document.count) units")
        resume()
    }

    // MARK: Touches and focus

    override var canBecomeFirstResponder: Bool { true }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 0) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 1) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { send(touches, 3) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        send(touches, 2)
        if spike_focused(host), !isFirstResponder {
            becomeFirstResponder()
        }
    }

    private func send(_ touches: Set<UITouch>, _ phase: UInt32) {
        for t in touches {
            let p = t.location(in: self)
            let id = UInt64(UInt(bitPattern: ObjectIdentifier(t).hashValue))
            spike_touch(host, id, phase, Float(p.x), Float(p.y), t.timestamp)
        }
        resume()
        syncFromRune()
    }

    // MARK: UIKeyInput (Tier 1)

    var hasText: Bool { spike_has_text(host) }

    func insertText(_ text: String) {
        edit("insertText") { h in text.withCString { spike_insert_text(h, $0) } }
    }

    func deleteBackward() {
        edit("deleteBackward") { spike_delete_backward($0) }
    }

    // MARK: UITextInput (Tier 2)

    private var document: [UInt16] {
        guard let host, let c = spike_text(host) else { return [] }
        defer { spike_string_free(c) }
        return Array(String(cString: c).utf16)
    }

    private func clamp(_ n: Int) -> Int { max(0, min(n, document.count)) }

    func text(in range: UITextRange) -> String? {
        guard let r = range as? Span else { return nil }
        let doc = document
        let lo = max(0, min(r.lo, doc.count)), hi = max(lo, min(r.hi, doc.count))
        return String(decoding: doc[lo..<hi], as: UTF16.self)
    }

    func replace(_ range: UITextRange, withText text: String) {
        guard let r = range as? Span else { return }
        edit("replace") { h in text.withCString { spike_replace(h, r.lo, r.hi, $0) } }
    }

    var selectedTextRange: UITextRange? {
        get {
            guard let host else { return nil }
            var a = 0, b = 0
            spike_selection(host, &a, &b)
            return Span(a, b)
        }
        set {
            guard let r = newValue as? Span else { return }
            edit("setSelection") { spike_set_selection($0, r.lo, r.hi) }
        }
    }

    var markedTextRange: UITextRange? {
        guard let host else { return nil }
        var a = 0, b = 0
        return spike_marked_range(host, &a, &b) ? Span(a, b) : nil
    }

    func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
        let text = markedText ?? ""
        edit("setMarkedText") { h in
            text.withCString { spike_set_marked(h, $0, selectedRange.location, selectedRange.location + selectedRange.length) }
        }
    }

    func unmarkText() {
        edit("unmarkText") { spike_unmark($0) }
    }

    var beginningOfDocument: UITextPosition { Pos(0) }
    var endOfDocument: UITextPosition { Pos(document.count) }

    func textRange(from fromPosition: UITextPosition, to toPosition: UITextPosition) -> UITextRange? {
        guard let a = fromPosition as? Pos, let b = toPosition as? Pos else { return nil }
        return Span(a.offset, b.offset)
    }

    func position(from position: UITextPosition, offset: Int) -> UITextPosition? {
        guard let p = position as? Pos else { return nil }
        let n = p.offset + offset
        return (0...document.count).contains(n) ? Pos(n) : nil
    }

    func position(from position: UITextPosition, in direction: UITextLayoutDirection, offset: Int) -> UITextPosition? {
        switch direction {
        case .left, .up: return self.position(from: position, offset: -offset)
        default: return self.position(from: position, offset: offset)
        }
    }

    func compare(_ position: UITextPosition, to other: UITextPosition) -> ComparisonResult {
        let a = (position as? Pos)?.offset ?? 0, b = (other as? Pos)?.offset ?? 0
        return a < b ? .orderedAscending : a > b ? .orderedDescending : .orderedSame
    }

    func offset(from: UITextPosition, to toPosition: UITextPosition) -> Int {
        ((toPosition as? Pos)?.offset ?? 0) - ((from as? Pos)?.offset ?? 0)
    }

    func position(within range: UITextRange, farthestIn direction: UITextLayoutDirection) -> UITextPosition? {
        guard let r = range as? Span else { return nil }
        return direction == .left || direction == .up ? Pos(r.lo) : Pos(r.hi)
    }

    func characterRange(byExtending position: UITextPosition, in direction: UITextLayoutDirection) -> UITextRange? {
        guard let p = position as? Pos else { return nil }
        return direction == .left || direction == .up ? Span(clamp(p.offset - 1), p.offset) : Span(p.offset, clamp(p.offset + 1))
    }

    func baseWritingDirection(for position: UITextPosition, in direction: UITextStorageDirection) -> NSWritingDirection { .leftToRight }
    func setBaseWritingDirection(_ writingDirection: NSWritingDirection, for range: UITextRange) {}

    // Geometry. Rune exposes only the live caret's rect (`Runtime::ime_area`), so every
    // position answers with it. Enough to place the candidate bar; wrong for anything that
    // asks about another position (the loupe, selection handles, the edit menu).
    private var caret: CGRect {
        var r = [Float](repeating: 0, count: 4)
        guard let host, spike_caret_rect(host, &r) else { return .zero }
        return CGRect(x: CGFloat(r[0]), y: CGFloat(r[1]), width: CGFloat(r[2]), height: CGFloat(r[3]))
    }

    func firstRect(for range: UITextRange) -> CGRect { caret }
    func caretRect(for position: UITextPosition) -> CGRect { caret }
    func selectionRects(for range: UITextRange) -> [UITextSelectionRect] { [] }
    func closestPosition(to point: CGPoint) -> UITextPosition? { selectedTextRange?.end }
    func closestPosition(to point: CGPoint, within range: UITextRange) -> UITextPosition? { range.end }
    func characterRange(at point: CGPoint) -> UITextRange? { nil }
}

/// The display link retains its target, so it holds this instead of the view.
private final class LinkTarget: NSObject {
    weak var view: SpikeView?
    init(_ view: SpikeView) { self.view = view }
    @objc func tick() { view?.tick() }
}

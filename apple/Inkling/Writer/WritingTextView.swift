import Foundation
import InklingCore
import os
import QuartzCore
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Logs keystroke-to-painted-frame latency when `INKLING_PROBE_TYPING=1` is set (or collects
/// samples directly for the Mac and iPhone typing benchmarks).
@MainActor
final class TypingProbe {
    static let enabled = ProcessInfo.processInfo.environment["INKLING_PROBE_TYPING"] != nil
        || ProcessInfo.processInfo.environment["INKLING_PROBE_OUT"] != nil
    private static let outPath = ProcessInfo.processInfo.environment["INKLING_PROBE_OUT"]
    private static let logger = Logger(subsystem: "com.tobygodat.inkling.native", category: "typing")
    private var startedAt: ContinuousClock.Instant?
    var onSample: ((Int) -> Void)?

    func recordKeystroke() {
        guard Self.enabled || onSample != nil else { return }
        if startedAt == nil {
            startedAt = ContinuousClock.now
        }
    }

    func recordPainted() {
        guard let start = startedAt else { return }
        startedAt = nil
        let elapsed = start.duration(to: ContinuousClock.now)
        let micros = Int(elapsed.components.seconds * 1_000_000 + elapsed.components.attoseconds / 1_000_000_000_000)
        if Self.enabled {
            Self.logger.notice("Inkling native: typed to painted \(micros, privacy: .public) us")
            if let path = Self.outPath {
                let line = "\(micros)\n"
                if let handle = FileHandle(forWritingAtPath: path) {
                    _ = try? handle.seekToEnd()
                    _ = try? handle.write(contentsOf: Data(line.utf8))
                    try? handle.close()
                } else {
                    try? Data(line.utf8).write(to: URL(filePath: path))
                }
            }
        }
        onSample?(micros)
    }
}

// MARK: - macOS TextKit 2 Live Markdown Editor

#if os(macOS)
struct WritingTextView: NSViewRepresentable {
    let model: WritingModel
    let style: WritingStyle
    let editable: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(model: model, style: style)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = WritingNSTextView(usingTextLayoutManager: true)
        textView.column = style.column
        textView.isEditable = editable
        textView.isSelectable = true
        textView.isRichText = true
        textView.importsGraphics = false
        textView.allowsUndo = true
        textView.usesFindBar = true
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.drawsBackground = true
        textView.backgroundColor = style.palette.ground
        textView.insertionPointColor = style.palette.ink
        textView.selectedTextAttributes = [.backgroundColor: style.palette.selection]
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.lineFragmentPadding = 0
        textView.setAccessibilityIdentifier("editor")
        textView.placeholderColor = style.palette.muted
        textView.placeholderFont = Fonts.reading(style.face, size: style.size)

        let coordinator = context.coordinator
        textView.textLayoutManager?.delegate = coordinator.layoutDelegate
        textView.textStorage?.delegate = coordinator
        textView.delegate = coordinator
        textView.probe = coordinator.probe
        coordinator.textView = textView
        coordinator.attachModel()

        coordinator.loadDocument(model.text, revision: model.externalRevision, into: textView)

        let scroll = NSScrollView()
        scroll.documentView = textView
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = style.palette.ground
        scroll.wantsLayer = true
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.model = model
        guard let textView = coordinator.textView else { return }

        if textView.isEditable != editable {
            textView.isEditable = editable
        }
        if textView.column != style.column {
            textView.column = style.column
            textView.needsLayout = true
            textView.setFrameSize(textView.frame.size)
        }
        if scroll.backgroundColor != style.palette.ground {
            scroll.backgroundColor = style.palette.ground
            textView.backgroundColor = style.palette.ground
        }
        textView.insertionPointColor = style.palette.ink
        textView.selectedTextAttributes = [.backgroundColor: style.palette.selection]
        textView.placeholderColor = style.palette.muted
        textView.placeholderFont = Fonts.reading(style.face, size: style.size)
        coordinator.layoutDelegate.palette = style.palette

        if coordinator.loadedDocId != model.id || coordinator.loadedRevision != model.externalRevision {
            coordinator.loadDocument(model.text, revision: model.externalRevision, into: textView)
        }
        if let storage = textView.textStorage {
            coordinator.styler.update(style: style, storage: storage)
            textView.typingAttributes = coordinator.styler.typingAttributes(in: storage, at: textView.selectedRange().location)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate, NSTextStorageDelegate {
        var model: WritingModel
        let styler: LiveMarkdownStyler
        let layoutDelegate: InklingLayoutDelegate
        let probe = TypingProbe()
        weak var textView: WritingNSTextView?
        var loadedDocId: String = ""
        var loadedRevision: Int = -1
        private var isRestyling = false

        init(model: WritingModel, style: WritingStyle) {
            self.model = model
            self.styler = LiveMarkdownStyler(style: style)
            self.layoutDelegate = InklingLayoutDelegate(palette: style.palette)
        }

        func attachModel() {
            model.readEditorText = { [weak self] in self?.textView?.string ?? "" }
            model.readEditorSelection = { [weak self] in self?.textView?.selectedRange() ?? NSRange() }
        }

        func loadDocument(_ text: String, revision: Int, into textView: WritingNSTextView) {
            loadedDocId = model.id
            loadedRevision = revision
            attachModel()
            guard let storage = textView.textStorage else { return }
            isRestyling = true
            storage.beginEditing()
            storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: text)
            storage.endEditing()
            styler.restyleAll(storage)
            isRestyling = false
            textView.typingAttributes = styler.typingAttributes(in: storage, at: 0)
            textView.needsDisplay = true
        }

        func textStorage(_ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorageEditActions, range editedRange: NSRange, changeInLength delta: Int) {
            guard !isRestyling, editedMask.contains(.editedCharacters) else { return }
            isRestyling = true
            styler.restyleEdited(textStorage, editedRange: editedRange)
            isRestyling = false
        }

        func textDidChange(_ notification: Notification) {
            guard let textView, let storage = textView.textStorage else { return }
            model.markEdited()
            textView.typingAttributes = styler.typingAttributes(in: storage, at: textView.selectedRange().location)
            textView.needsDisplay = true
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard let textView, let storage = textView.textStorage else { return }
            textView.typingAttributes = styler.typingAttributes(in: storage, at: textView.selectedRange().location)
        }
    }
}

/// Centered-column TextKit 2 `NSTextView` with quiet placeholder and typing latency probe.
final class WritingNSTextView: NSTextView {
    var column: CGFloat = 700
    var placeholderText = "Start writing…"
    var placeholderColor: NSColor = .secondaryLabelColor
    var placeholderFont: NSFont = .systemFont(ofSize: 21)
    weak var probe: TypingProbe?

    override var textContainerOrigin: NSPoint {
        NSPoint(x: textContainerInset.width, y: textContainerInset.height)
    }

    override func setFrameSize(_ size: NSSize) {
        let side = max(48, ((size.width - column) / 2).rounded())
        let inset = NSSize(width: side, height: 44)
        if textContainerInset != inset {
            textContainerInset = inset
        }
        super.setFrameSize(size)
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        probe?.recordKeystroke()
        super.insertText(string, replacementRange: replacementRange)
        if probe != nil {
            CATransaction.begin()
            CATransaction.setCompletionBlock { [weak self] in
                MainActor.assumeIsolated { self?.probe?.recordPainted() }
            }
            displayIfNeeded()
            CATransaction.commit()
        }
    }

    override func paste(_ sender: Any?) {
        // Always paste plain Markdown text so copied web styles never pollute the draft.
        pasteAsPlainText(sender)
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard string.isEmpty else { return }
        let origin = textContainerOrigin
        let attrs: [NSAttributedString.Key: Any] = [
            .font: placeholderFont,
            .foregroundColor: placeholderColor.withAlphaComponent(0.65),
        ]
        (placeholderText as NSString).draw(at: NSPoint(x: origin.x + 2, y: origin.y), withAttributes: attrs)
    }
}

// MARK: - iOS TextKit 2 Live Markdown Editor

#else
struct WritingTextView: UIViewRepresentable {
    let model: WritingModel
    let style: WritingStyle
    let editable: Bool
    @Binding var isFocused: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(model: model, style: style, isFocused: $isFocused)
    }

    func makeUIView(context: Context) -> WritingUITextView {
        let view = WritingUITextView(usingTextLayoutManager: true)
        view.column = style.column
        view.isEditable = editable
        view.isSelectable = true
        view.backgroundColor = style.palette.ground
        view.tintColor = style.palette.ink
        view.textContainer.lineFragmentPadding = 0
        view.alwaysBounceVertical = true
        view.keyboardDismissMode = .interactive
        view.smartQuotesType = .no
        view.smartDashesType = .no
        view.accessibilityIdentifier = "editor"
        view.placeholderColor = style.palette.muted
        view.placeholderFont = Fonts.reading(style.face, size: style.size)

        let coordinator = context.coordinator
        view.textLayoutManager?.delegate = coordinator.layoutDelegate
        view.textStorage.delegate = coordinator
        view.delegate = coordinator
        view.probe = coordinator.probe
        coordinator.textView = view
        coordinator.attachModel()

        coordinator.loadDocument(model.text, revision: model.externalRevision, into: view)
        return view
    }

    func updateUIView(_ view: WritingUITextView, context: Context) {
        let coordinator = context.coordinator
        coordinator.model = model
        coordinator.isFocused = $isFocused

        if view.isEditable != editable {
            view.isEditable = editable
        }
        if view.column != style.column {
            view.column = style.column
            view.setNeedsLayout()
        }
        if view.backgroundColor != style.palette.ground {
            view.backgroundColor = style.palette.ground
        }
        view.tintColor = style.palette.ink
        view.placeholderColor = style.palette.muted
        view.placeholderFont = Fonts.reading(style.face, size: style.size)
        coordinator.layoutDelegate.palette = style.palette

        if !isFocused && view.isFirstResponder {
            view.resignFirstResponder()
        }

        if coordinator.loadedDocId != model.id || coordinator.loadedRevision != model.externalRevision {
            coordinator.loadDocument(model.text, revision: model.externalRevision, into: view)
        }
        coordinator.styler.update(style: style, storage: view.textStorage)
        view.typingAttributes = coordinator.styler.typingAttributes(in: view.textStorage, at: view.selectedRange.location)
    }

    final class Coordinator: NSObject, UITextViewDelegate, NSTextStorageDelegate {
        var model: WritingModel
        let styler: LiveMarkdownStyler
        let layoutDelegate: InklingLayoutDelegate
        let probe = TypingProbe()
        var isFocused: Binding<Bool>
        weak var textView: WritingUITextView?
        var loadedDocId: String = ""
        var loadedRevision: Int = -1
        private var isRestyling = false

        init(model: WritingModel, style: WritingStyle, isFocused: Binding<Bool>) {
            self.model = model
            self.styler = LiveMarkdownStyler(style: style)
            self.layoutDelegate = InklingLayoutDelegate(palette: style.palette)
            self.isFocused = isFocused
        }

        func attachModel() {
            model.readEditorText = { [weak self] in self?.textView?.text ?? "" }
            model.readEditorSelection = { [weak self] in self?.textView?.selectedRange ?? NSRange() }
        }

        func loadDocument(_ text: String, revision: Int, into textView: WritingUITextView) {
            loadedDocId = model.id
            loadedRevision = revision
            attachModel()
            let storage = textView.textStorage
            isRestyling = true
            storage.beginEditing()
            storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: text)
            storage.endEditing()
            styler.restyleAll(storage)
            isRestyling = false
            textView.typingAttributes = styler.typingAttributes(in: storage, at: 0)
            textView.updatePlaceholder()
        }

        func textStorage(_ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorage.EditActions, range editedRange: NSRange, changeInLength delta: Int) {
            guard !isRestyling, editedMask.contains(.editedCharacters) else { return }
            isRestyling = true
            styler.restyleEdited(textStorage, editedRange: editedRange)
            isRestyling = false
        }

        func textViewDidBeginEditing(_ textView: UITextView) {
            isFocused.wrappedValue = true
        }

        func textViewDidEndEditing(_ textView: UITextView) {
            isFocused.wrappedValue = false
        }

        func textViewDidChange(_ textView: UITextView) {
            model.markEdited()
            textView.typingAttributes = styler.typingAttributes(in: textView.textStorage, at: textView.selectedRange.location)
            (textView as? WritingUITextView)?.updatePlaceholder()
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            textView.typingAttributes = styler.typingAttributes(in: textView.textStorage, at: textView.selectedRange.location)
        }
    }
}

/// Centered-column TextKit 2 `UITextView` with quiet placeholder and typing latency probe.
final class WritingUITextView: UITextView {
    var column: CGFloat = 700
    var placeholderColor: UIColor = .secondaryLabel { didSet { placeholderLabel.textColor = placeholderColor.withAlphaComponent(0.65) } }
    var placeholderFont: UIFont = .systemFont(ofSize: 19) { didSet { placeholderLabel.font = placeholderFont } }
    weak var probe: TypingProbe?

    private let placeholderLabel: UILabel = {
        let label = UILabel()
        label.text = "Start writing…"
        label.isAccessibilityElement = false
        return label
    }()

    private var frameLink: CADisplayLink?
    private var pendingPaint = false

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if placeholderLabel.superview == nil {
            addSubview(placeholderLabel)
        }
        updatePlaceholder()
        if window != nil && (TypingProbe.enabled || probe?.onSample != nil) {
            if frameLink == nil {
                let link = CADisplayLink(target: self, selector: #selector(frameCommitted))
                link.add(to: .main, forMode: .common)
                frameLink = link
            }
        } else {
            frameLink?.invalidate()
            frameLink = nil
        }
    }

    override func layoutSubviews() {
        let side = max(22, ((bounds.width - column) / 2).rounded())
        let inset = UIEdgeInsets(top: 20, left: side, bottom: 160, right: side)
        if textContainerInset != inset {
            textContainerInset = inset
        }
        super.layoutSubviews()
        let size = placeholderLabel.intrinsicContentSize
        placeholderLabel.frame = CGRect(x: textContainerInset.left + 2, y: textContainerInset.top, width: bounds.width - textContainerInset.left - textContainerInset.right, height: size.height)
    }

    func updatePlaceholder() {
        placeholderLabel.isHidden = !text.isEmpty
    }

    override func insertText(_ text: String) {
        probe?.recordKeystroke()
        super.insertText(text)
        if TypingProbe.enabled || probe?.onSample != nil {
            pendingPaint = true
        }
    }

    @objc private func frameCommitted() {
        guard pendingPaint else { return }
        pendingPaint = false
        layoutIfNeeded()
        probe?.recordPainted()
    }
}
#endif

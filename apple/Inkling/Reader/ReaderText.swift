import InklingCore
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// TextKit 2 helpers both text views share: where a character is laid out, and which character
/// is at a height.
enum Layout {
    static func top(of character: Int, in layout: NSTextLayoutManager) -> CGFloat? {
        let document = layout.documentRange
        guard let location = layout.location(document.location, offsetBy: character),
              let before = NSTextRange(location: document.location, end: location) else { return nil }
        layout.ensureLayout(for: before)
        return layout.textLayoutFragment(for: location)?.layoutFragmentFrame.minY
    }

    static func character(at y: CGFloat, in layout: NSTextLayoutManager) -> Int? {
        guard let fragment = layout.textLayoutFragment(for: CGPoint(x: 1, y: max(y, 0))) else { return nil }
        return layout.offset(from: layout.documentRange.location, to: fragment.rangeInElement.location)
    }

    /// Paints the highlights on `storage`, putting back what `base` had under the ones removed.
    static func paint(_ storage: NSTextStorage, base: NSAttributedString, from old: [NSRange], to new: [NSRange], color: PlatformColor) {
        storage.beginEditing()
        for range in old where NSMaxRange(range) <= base.length {
            storage.removeAttribute(.backgroundColor, range: range)
            base.enumerateAttribute(.backgroundColor, in: range) { value, sub, _ in
                if let value { storage.addAttribute(.backgroundColor, value: value, range: sub) }
            }
        }
        for range in new where NSMaxRange(range) <= storage.length {
            storage.addAttribute(.backgroundColor, value: color, range: range)
        }
        storage.endEditing()
    }
}

#if os(iOS)
struct ReaderText: UIViewRepresentable {
    let model: ReaderModel
    let palette: Palette
    let column: CGFloat

    func makeCoordinator() -> Coordinator { Coordinator(model: model, palette: palette) }

    func makeUIView(context: Context) -> ColumnTextView {
        let view = ColumnTextView(usingTextLayoutManager: true)
        view.column = column
        view.isEditable = false
        view.isSelectable = true
        view.backgroundColor = palette.ground
        view.tintColor = palette.link
        view.textContainer.lineFragmentPadding = 0
        view.alwaysBounceVertical = true
        view.attributedText = model.rendered.text
        view.delegate = context.coordinator
        view.accessibilityIdentifier = "reader"
        context.coordinator.view = view
        context.coordinator.repaint()
        view.pending = model.jump
        model.jump = nil
        view.onLaidOut = { [weak coordinator = context.coordinator] in coordinator?.report() }
        return view
    }

    func updateUIView(_ view: ColumnTextView, context: Context) {
        context.coordinator.model = model
        context.coordinator.repaint()
        if let jump = model.jump {
            view.pending = jump
            model.jump = nil
            view.setNeedsLayout()
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var model: ReaderModel
        let palette: Palette
        weak var view: ColumnTextView?
        private var painted: [NSRange] = []

        init(model: ReaderModel, palette: Palette) {
            self.model = model
            self.palette = palette
        }

        func repaint() {
            let ranges = model.highlightRanges
            guard ranges != painted, let view else { return }
            Layout.paint(view.textStorage, base: model.rendered.text, from: painted, to: ranges, color: palette.highlight)
            painted = ranges
        }

        func scrollViewDidScroll(_ scrollView: UIScrollView) {
            report()
        }

        func report() {
            guard let view, let layout = view.textLayoutManager, view.pending == nil, view.bounds.width > 0 else { return }
            let y = view.contentOffset.y + view.adjustedContentInset.top - view.textContainerInset.top + 1
            let atEnd = view.contentOffset.y + view.bounds.height - view.adjustedContentInset.bottom >= view.contentSize.height - 2
            if let character = Layout.character(at: y, in: layout) {
                model.scrolled(toCharacter: character, atEnd: atEnd && view.contentOffset.y > 0)
            }
        }

        func textView(_ textView: UITextView, editMenuForTextIn range: NSRange, suggestedActions: [UIMenuElement]) -> UIMenu? {
            let action: UIAction
            if let passage = model.highlight(covering: range) {
                action = UIAction(title: "Remove Highlight", image: UIImage(systemName: "highlighter")) { [weak self] _ in
                    self?.model.remove(passage)
                    self?.repaint()
                    textView.selectedTextRange = nil
                }
            } else {
                action = UIAction(title: "Highlight", image: UIImage(systemName: "highlighter")) { [weak self] _ in
                    self?.model.highlight(range)
                    self?.repaint()
                    textView.selectedTextRange = nil
                }
            }
            return UIMenu(children: [action] + suggestedActions)
        }
    }
}

/// A text view that keeps its text in a centred column no wider than the look's page width.
final class ColumnTextView: UITextView {
    var column: CGFloat = 700
    var pending: Int?
    var onLaidOut: (() -> Void)?

    override func layoutSubviews() {
        let side = max(24, ((bounds.width - column) / 2).rounded())
        let inset = UIEdgeInsets(top: 28, left: side, bottom: 96, right: side)
        if textContainerInset != inset { textContainerInset = inset }
        super.layoutSubviews()
        guard let character = pending, bounds.width > 0, let layout = textLayoutManager else { return }
        pending = nil
        if let top = Layout.top(of: character, in: layout), character > 0 {
            let most = max(contentSize.height - bounds.height + adjustedContentInset.bottom, -adjustedContentInset.top)
            contentOffset.y = min(max(top + textContainerInset.top - adjustedContentInset.top, -adjustedContentInset.top), most)
        }
        DispatchQueue.main.async { [weak self] in self?.onLaidOut?() }
    }
}
#else
struct ReaderText: NSViewRepresentable {
    let model: ReaderModel
    let palette: Palette
    let column: CGFloat

    func makeCoordinator() -> Coordinator { Coordinator(model: model, palette: palette) }

    func makeNSView(context: Context) -> NSScrollView {
        let text = ColumnTextView(usingTextLayoutManager: true)
        text.column = column
        text.isEditable = false
        text.isSelectable = true
        text.drawsBackground = true
        text.backgroundColor = palette.ground
        text.isVerticallyResizable = true
        text.isHorizontallyResizable = false
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        text.textContainer?.lineFragmentPadding = 0
        text.selectedTextAttributes = [.backgroundColor: palette.selection]
        text.textStorage?.setAttributedString(model.rendered.text)
        text.delegate = context.coordinator
        text.setAccessibilityIdentifier("reader")

        let scroll = NSScrollView()
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.ground
        scroll.contentView.postsBoundsChangedNotifications = true
        context.coordinator.text = text
        context.coordinator.scroll = scroll
        context.coordinator.repaint()
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.scrolled), name: NSView.boundsDidChangeNotification, object: scroll.contentView)
        text.pending = model.jump
        model.jump = nil
        text.onLaidOut = { [weak coordinator = context.coordinator] in coordinator?.jump() }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.model = model
        context.coordinator.repaint()
        if let jump = model.jump {
            context.coordinator.text?.pending = jump
            model.jump = nil
            context.coordinator.jump()
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var model: ReaderModel
        let palette: Palette
        weak var text: ColumnTextView?
        weak var scroll: NSScrollView?
        private var painted: [NSRange] = []

        init(model: ReaderModel, palette: Palette) {
            self.model = model
            self.palette = palette
        }

        func repaint() {
            let ranges = model.highlightRanges
            guard ranges != painted, let storage = text?.textStorage else { return }
            Layout.paint(storage, base: model.rendered.text, from: painted, to: ranges, color: palette.highlight)
            painted = ranges
        }

        func jump() {
            guard let text, let scroll, let character = text.pending, text.bounds.width > 0, let layout = text.textLayoutManager else { return }
            text.pending = nil
            guard character > 0, let top = Layout.top(of: character, in: layout) else { return }
            let y = top + text.textContainerInset.height - scroll.contentInsets.top
            scroll.contentView.scroll(to: NSPoint(x: 0, y: max(y, -scroll.contentInsets.top)))
            scroll.reflectScrolledClipView(scroll.contentView)
        }

        @objc func scrolled() {
            guard let text, let scroll, text.pending == nil, let layout = text.textLayoutManager else { return }
            let visible = scroll.contentView.bounds
            let y = visible.minY + scroll.contentInsets.top - text.textContainerInset.height + 1
            let atEnd = visible.maxY >= text.frame.height - 2 && visible.minY > 0
            if let character = Layout.character(at: y, in: layout) {
                model.scrolled(toCharacter: character, atEnd: atEnd)
            }
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            model.selection = text?.selectedRange() ?? NSRange()
        }

        func textView(_ view: NSTextView, menu: NSMenu, for event: NSEvent, at charIndex: Int) -> NSMenu? {
            let range = view.selectedRange()
            guard range.length > 0 else { return menu }
            let covering = model.highlight(covering: range)
            let item = NSMenuItem(title: covering == nil ? "Highlight" : "Remove Highlight", action: #selector(toggle), keyEquivalent: "")
            item.image = NSImage(systemSymbolName: "highlighter", accessibilityDescription: nil)
            item.target = self
            menu.insertItem(item, at: 0)
            menu.insertItem(.separator(), at: 1)
            return menu
        }

        @objc func toggle() {
            model.toggleHighlight()
            repaint()
        }
    }
}

/// A text view that keeps its text in a centred column no wider than the look's page width.
final class ColumnTextView: NSTextView {
    var column: CGFloat = 700
    var pending: Int?
    var onLaidOut: (() -> Void)?

    override func setFrameSize(_ size: NSSize) {
        let side = max(40, ((size.width - column) / 2).rounded())
        let inset = NSSize(width: side, height: 40)
        if textContainerInset != inset { textContainerInset = inset }
        super.setFrameSize(size)
        if pending != nil, size.width > 0 {
            DispatchQueue.main.async { [weak self] in self?.onLaidOut?() }
        }
    }
}
#endif

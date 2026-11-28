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
        if character <= 0 {
            #if os(iOS)
            // Fresh opens start at the top; only relocate when jumping back from a later viewport.
            if let start = layout.textViewportLayoutController.viewportRange?.location,
               start.compare(document.location) != .orderedSame {
                let origin = layout.textViewportLayoutController.relocateViewport(to: document.location)
                layout.textViewportLayoutController.layoutViewport()
                return layout.textLayoutFragment(for: document.location)?.layoutFragmentFrame.minY ?? origin
            }
            #endif
            return 0
        }
        guard let location = layout.location(document.location, offsetBy: character) else { return nil }
        #if os(iOS)
        // Relocate TextKit 2's viewport directly to `location` without laying out the whole prefix
        // from character 0, so opening or jumping into a long book doesn't stall the main thread.
        let origin = layout.textViewportLayoutController.relocateViewport(to: location)
        layout.textViewportLayoutController.layoutViewport()
        return layout.textLayoutFragment(for: location)?.layoutFragmentFrame.minY ?? origin
        #else
        let end = layout.location(location, offsetBy: 4_000) ?? document.endLocation
        if let window = NSTextRange(location: location, end: end) {
            layout.ensureLayout(for: window)
        }
        return layout.textLayoutFragment(for: location)?.layoutFragmentFrame.minY
        #endif
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
        view.linkTextAttributes = [:]
        view.textContainer.lineFragmentPadding = 0
        view.alwaysBounceVertical = true
        view.attributedText = model.rendered.text
        view.delegate = context.coordinator
        view.accessibilityIdentifier = "reader"
        if FrameProbe.isActive {
            // Keep XCUITest gesture synthesis from triggering UIKit's O(paragraphs × chars)
            // UITextViewAccessibility._accessibilityParagraphElements snapshot during measurement.
            view.accessibilityElementsHidden = true
        }
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped(_:)))
        tap.delegate = context.coordinator
        view.addGestureRecognizer(tap)
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
            let motion = model.motion
            model.jump = nil
            model.motion = .instant
            context.coordinator.move(to: jump, motion)
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate, UIGestureRecognizerDelegate {
        var model: ReaderModel
        let palette: Palette
        weak var view: ColumnTextView?
        private var painted: [NSRange] = []
        private var paintedPassages: [Passage] = []
        private var lastOffsetY: CGFloat = 0
        private var dragTravel: CGFloat = 0
        /// A chapter jump gliding or dipping out and in, until it settles or a finger stops it.
        private var dip: UIViewPropertyAnimator?

        init(model: ReaderModel, palette: Palette) {
            self.model = model
            self.palette = palette
        }

        /// Paints the highlights when they change. It runs on every update, so it compares the
        /// passages before it maps any of them into the text.
        func repaint() {
            guard model.highlights != paintedPassages, let view else { return }
            let ranges = model.highlightRanges
            Layout.paint(view.textStorage, base: model.rendered.text, from: painted, to: ranges, color: palette.highlight)
            painted = ranges
            paintedPassages = model.highlights
        }

        /// Stops any in-flight chapter jump at its live on-screen position so touching or starting
        /// another jump never snaps to the old target.
        private func stopDip(restoreAlpha: Bool = false) {
            guard let dip, let view else { self.dip = nil; return }
            if dip.isRunning {
                let presentation = view.layer.presentation()
                let liveY = presentation?.bounds.origin.y ?? view.contentOffset.y
                let liveTransform = presentation?.affineTransform() ?? view.transform
                let liveAlpha = CGFloat(presentation?.opacity ?? Float(view.alpha))
                dip.stopAnimation(true)
                view.contentOffset.y = liveY
                view.transform = liveTransform
                view.alpha = liveAlpha
            }
            self.dip = nil
            if restoreAlpha, (view.alpha < 1 || view.transform != .identity) {
                let restore = UIViewPropertyAnimator(duration: 0.2, timingParameters: UISpringTimingParameters(duration: 0.2, bounce: 0))
                restore.addAnimations {
                    view.alpha = 1
                    view.transform = .identity
                }
                restore.startAnimation()
                self.dip = restore
            }
        }

        /// A finger on the text takes it from wherever a jump has got to.
        func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
            stopDip(restoreAlpha: true)
            lastOffsetY = scrollView.contentOffset.y
            dragTravel = 0
        }

        /// Bars hiding or showing shift `adjustedContentInset.top`; resync so the inset change
        /// never looks like the user scrolled the other way.
        func scrollViewDidChangeAdjustedContentInset(_ scrollView: UIScrollView) {
            lastOffsetY = scrollView.contentOffset.y
            dragTravel = 0
        }

        func scrollViewDidScroll(_ scrollView: UIScrollView) {
            report()
            let delta = scrollView.contentOffset.y - lastOffsetY
            lastOffsetY = scrollView.contentOffset.y
            let top = scrollView.contentOffset.y + scrollView.adjustedContentInset.top
            if top < 40 && (scrollView.isTracking || scrollView.isDecelerating) {
                dragTravel = 0
                show(true)
                return
            }
            // Only accumulate travel while the finger is driving the scroll so bottom rubber-band
            // bounce-back never flips the bars back open.
            guard scrollView.isTracking else { return }
            if (delta > 0 && dragTravel < 0) || (delta < 0 && dragTravel > 0) {
                dragTravel = 0
            }
            dragTravel += delta
            if dragTravel < -16 {
                dragTravel = 0
                show(true)
            } else if dragTravel > 16 {
                dragTravel = 0
                show(false)
            }
        }

        /// A tap on the text, with nothing selected, brings the bars back or puts them away. A tap
        /// on "Next: …" only follows it.
        @objc func tapped(_ tap: UITapGestureRecognizer) {
            guard let view, view.selectedRange.length == 0 else { return }
            if let position = view.closestPosition(to: tap.location(in: view)) {
                let at = view.offset(from: view.beginningOfDocument, to: position)
                let storage = view.textStorage
                if [at, at - 1].contains(where: { $0 >= 0 && $0 < storage.length && storage.attribute(.link, at: $0, effectiveRange: nil) != nil }) { return }
            }
            show(model.chromeHidden)
        }

        /// Moves the text so `character`'s line sits at the top of the column.
        func move(to character: Int, _ motion: ReaderModel.Motion) {
            guard let view else { return }
            guard motion != .instant, view.bounds.width > 0, let y = view.offset(for: character) else {
                view.pending = character
                view.setNeedsLayout()
                return
            }
            stopDip()
            let still = UIAccessibility.isReduceMotionEnabled
            let distance = y - view.contentOffset.y
            if motion == .glide, !still, abs(distance) < view.bounds.height * 2 {
                let glide = UIViewPropertyAnimator(duration: 0.32, timingParameters: UISpringTimingParameters(duration: 0.32, bounce: 0))
                glide.addAnimations {
                    view.alpha = 1
                    view.transform = .identity
                    view.contentOffset.y = y
                }
                glide.startAnimation()
                dip = glide
                return
            }
            let rise: CGFloat = still ? 0 : (distance >= 0 ? 14 : -14)
            let out = UIViewPropertyAnimator(duration: 0.12, curve: .easeOut) { view.alpha = 0 }
            out.addCompletion { [weak self] position in
                guard position == .end, let self else { return }
                // Jump contentOffset once while invisible, then rise with a GPU layer transform
                // instead of animating contentOffset frame by frame.
                view.contentOffset.y = y
                view.transform = rise == 0 ? .identity : CGAffineTransform(translationX: 0, y: rise)
                let into = still
                    ? UIViewPropertyAnimator(duration: 0.18, curve: .easeOut)
                    : UIViewPropertyAnimator(duration: 0.28, timingParameters: UISpringTimingParameters(duration: 0.28, bounce: 0))
                into.addAnimations {
                    view.alpha = 1
                    view.transform = .identity
                }
                into.startAnimation()
                dip = into
            }
            out.startAnimation()
            dip = out
        }

        func textView(_ textView: UITextView, primaryActionFor textItem: UITextItem, defaultAction: UIAction) -> UIAction? {
            guard case .link(let url) = textItem.content, Renderer.isOwn(url) else { return defaultAction }
            if let chapter = Renderer.chapter(of: url) {
                return UIAction { [weak self] _ in self?.model.go(toChapter: chapter, .glide) }
            }
            return UIAction { [weak self] _ in self?.model.done?() }
        }

        func textView(_ textView: UITextView, menuConfigurationFor textItem: UITextItem, defaultMenu: UIMenu) -> UITextItem.MenuConfiguration? {
            if case .link(let url) = textItem.content, Renderer.isOwn(url) { return nil }
            return UITextItem.MenuConfiguration(menu: defaultMenu)
        }

        func textViewDidChangeSelection(_ textView: UITextView) {
            model.selection = textView.selectedRange
        }

        func gestureRecognizer(_ gesture: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }

        /// The bars go on a spring, so reading back halfway turns them round from where they are.
        /// Reduce Motion puts them away and back without sliding.
        private func show(_ shown: Bool) {
            guard model.chromeHidden == shown else { return }
            withAnimation(UIAccessibility.isReduceMotionEnabled ? nil : .bars) { model.chromeHidden = !shown }
        }

        /// Reports the place from the line at the top of the column, below the inset kept above it.
        func report() {
            guard let view, let layout = view.textLayoutManager, view.pending == nil, view.bounds.width > 0 else { return }
            let y = view.contentOffset.y + view.adjustedContentInset.top + 1
            let atEnd = view.contentOffset.y + view.bounds.height >= view.contentSize.height - 2
            if let character = Layout.character(at: y, in: layout) {
                model.scrolled(toCharacter: character, atEnd: atEnd && view.contentOffset.y > 0)
                model.rendered.preparePictures(around: character)
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
        guard let character = pending, bounds.width > 0 else { return }
        pending = nil
        if let y = offset(for: character) { contentOffset.y = y }
        DispatchQueue.main.async { [weak self] in self?.onLaidOut?() }
    }

    /// The offset that puts `character`'s line at the top of the column, with the inset above it
    /// kept, so the line clears the fade under the bars.
    func offset(for character: Int) -> CGFloat? {
        guard let layout = textLayoutManager, let top = Layout.top(of: character, in: layout) else { return nil }
        let height = max(contentSize.height, layout.usageBoundsForTextContainer.height + textContainerInset.top + textContainerInset.bottom)
        let most = max(height - bounds.height + adjustedContentInset.bottom, -adjustedContentInset.top)
        return min(max(top - adjustedContentInset.top, -adjustedContentInset.top), most)
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
        text.linkTextAttributes = [.cursor: NSCursor.pointingHand]
        text.textStorage?.setAttributedString(model.rendered.text)
        text.delegate = context.coordinator
        text.setAccessibilityIdentifier("reader")

        let scroll = NSScrollView()
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.ground
        scroll.contentView.postsBoundsChangedNotifications = true
        scroll.wantsLayer = true
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
            let motion = model.motion
            context.coordinator.text?.pending = jump
            model.jump = nil
            model.motion = .instant
            context.coordinator.jump(motion)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var model: ReaderModel
        let palette: Palette
        weak var text: ColumnTextView?
        weak var scroll: NSScrollView?
        private var painted: [NSRange] = []
        private var paintedPassages: [Passage] = []
        private var jumpToken = 0

        init(model: ReaderModel, palette: Palette) {
            self.model = model
            self.palette = palette
        }

        /// Paints the highlights when they change. It runs on every update, so it compares the
        /// passages before it maps any of them into the text.
        func repaint() {
            guard model.highlights != paintedPassages, let storage = text?.textStorage else { return }
            let ranges = model.highlightRanges
            Layout.paint(storage, base: model.rendered.text, from: painted, to: ranges, color: palette.highlight)
            painted = ranges
            paintedPassages = model.highlights
        }

        /// Moves the text so the pending character's line sits at the top of the column, with the
        /// inset above it kept.
        func jump(_ motion: ReaderModel.Motion = .instant) {
            guard let text, let scroll, let character = text.pending, text.bounds.width > 0, let layout = text.textLayoutManager else { return }
            text.pending = nil
            guard let top = Layout.top(of: character, in: layout) else { return }
            let needed = layout.usageBoundsForTextContainer.height + text.textContainerInset.height * 2
            if text.frame.height < needed {
                text.setFrameSize(NSSize(width: text.frame.width, height: needed))
            }
            let most = max(text.frame.height - scroll.contentView.bounds.height, -scroll.contentInsets.top)
            let y = min(max(top - scroll.contentInsets.top, -scroll.contentInsets.top), most)
            let clip = scroll.contentView
            let still = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
            let distance = y - clip.bounds.minY
            jumpToken += 1
            let token = jumpToken
            let settle: @MainActor () -> Void = { scroll.reflectScrolledClipView(clip) }
            switch motion {
            case .instant:
                scroll.alphaValue = 1
                clip.scroll(to: NSPoint(x: 0, y: y))
                settle()
            case .glide where !still && abs(distance) < clip.bounds.height * 2:
                NSAnimationContext.runAnimationGroup { context in
                    context.duration = 0.36
                    context.timingFunction = CAMediaTimingFunction(controlPoints: 0.23, 1, 0.32, 1)
                    clip.animator().setBoundsOrigin(NSPoint(x: 0, y: y))
                } completionHandler: { MainActor.assumeIsolated { settle() } }
            case .glide, .fade:
                let rise: CGFloat = still ? 0 : (distance >= 0 ? 14 : -14)
                NSAnimationContext.runAnimationGroup { context in
                    context.duration = 0.12
                    context.timingFunction = CAMediaTimingFunction(name: .easeOut)
                    scroll.animator().alphaValue = 0
                } completionHandler: { [weak self] in
                    MainActor.assumeIsolated {
                        guard let self, self.jumpToken == token else { return }
                        clip.scroll(to: NSPoint(x: 0, y: y - rise))
                        settle()
                        NSAnimationContext.runAnimationGroup { context in
                            context.duration = still ? 0.18 : 0.26
                            context.timingFunction = CAMediaTimingFunction(controlPoints: 0.23, 1, 0.32, 1)
                            scroll.animator().alphaValue = 1
                            clip.animator().setBoundsOrigin(NSPoint(x: 0, y: y))
                        } completionHandler: { MainActor.assumeIsolated { settle() } }
                    }
                }
            }
        }

        /// Reports the place from the line at the top of the column, below the inset kept above it.
        @objc func scrolled() {
            guard let text, let scroll, text.pending == nil, let layout = text.textLayoutManager else { return }
            let visible = scroll.contentView.bounds
            let y = visible.minY + scroll.contentInsets.top + 1
            let atEnd = visible.maxY >= text.frame.height - 2 && visible.minY > 0
            if let character = Layout.character(at: y, in: layout) {
                model.scrolled(toCharacter: character, atEnd: atEnd)
                model.rendered.preparePictures(around: character)
            }
        }

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            guard let url = link as? URL else { return true }
            if let chapter = Renderer.chapter(of: url) {
                model.go(toChapter: chapter, .glide)
            } else if url == Renderer.done {
                model.done?()
            }
            return true
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

    /// The column starts at the inset. AppKit otherwise keeps an origin it worked out before a long
    /// text's first layout, which shifts every line right of the column.
    override var textContainerOrigin: NSPoint {
        NSPoint(x: textContainerInset.width, y: textContainerInset.height)
    }

    override func setFrameSize(_ size: NSSize) {
        let side = max(56, ((size.width - column) / 2).rounded())
        let inset = NSSize(width: side, height: 40)
        if textContainerInset != inset { textContainerInset = inset }
        super.setFrameSize(size)
        if pending != nil, size.width > 0 {
            DispatchQueue.main.async { [weak self] in self?.onLaidOut?() }
        }
    }
}
#endif

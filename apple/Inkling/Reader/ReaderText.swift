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
    /// Where the line at `anchor` is laid out: its paragraph's top, plus how far down the paragraph.
    /// The text is laid out from its start down past it first, so the place is where the text will
    /// stay, not an estimate of the text before it.
    static func top(of anchor: ReaderModel.Anchor, in layout: NSTextLayoutManager) -> CGFloat? {
        let document = layout.documentRange
        guard let location = layout.location(document.location, offsetBy: max(anchor.character, 0)) else { return nil }
        let end = layout.location(location, offsetBy: 4_000) ?? document.endLocation
        if let before = NSTextRange(location: document.location, end: end) {
            layout.ensureLayout(for: before)
        }
        return layout.textLayoutFragment(for: location).map { Self.line(anchor, in: $0) }
    }

    private static func line(_ anchor: ReaderModel.Anchor, in fragment: NSTextLayoutFragment) -> CGFloat {
        let frame = fragment.layoutFragmentFrame
        return frame.minY + anchor.within * frame.height
    }

    /// The line at the height `y`: the paragraph there and how far down it.
    static func anchor(at y: CGFloat, in layout: NSTextLayoutManager) -> ReaderModel.Anchor? {
        guard let fragment = layout.textLayoutFragment(for: CGPoint(x: 1, y: max(y, 0) + 1)) else { return nil }
        let frame = fragment.layoutFragmentFrame
        let character = layout.offset(from: layout.documentRange.location, to: fragment.rangeInElement.location)
        return ReaderModel.Anchor(character: character, within: min(max((y - frame.minY) / max(frame.height, 1), 0), 1))
    }

    /// Gives the text `text`'s attributes, the same characters in a new paper, and returns how far the
    /// line at `y` moved. Rendering attributes would cost no layout, but UITextView never redraws
    /// a fragment for them.
    static func recolour(_ storage: NSTextStorage, to text: NSAttributedString, in layout: NSTextLayoutManager, at y: CGFloat) -> CGFloat {
        let anchor = anchor(at: y, in: layout)
        let was = anchor.flatMap { top(of: $0, in: layout) }
        storage.beginEditing()
        text.enumerateAttributes(in: NSRange(location: 0, length: min(text.length, storage.length))) { attributes, range, _ in
            storage.setAttributes(attributes, range: range)
        }
        storage.endEditing()
        guard let anchor, let was, let now = top(of: anchor, in: layout) else { return 0 }
        return now - was
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
        view.contentInsetAdjustmentBehavior = .never
        view.attributedText = model.rendered.text
        view.delegate = context.coordinator
        view.accessibilityIdentifier = "reader"
        if FrameProbe.isActive || ScrollProbe.isAsked {
            // Keep XCUITest gesture synthesis from triggering UIKit's O(paragraphs × chars)
            // UITextViewAccessibility._accessibilityParagraphElements snapshot during measurement.
            view.accessibilityElementsHidden = true
        }
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped(_:)))
        tap.delegate = context.coordinator
        view.addGestureRecognizer(tap)
        context.coordinator.view = view
        ScrollProbe.watch(view)
        context.coordinator.repaint()
        view.pending = model.jump.map { ReaderModel.Anchor(character: $0, within: model.jumpWithin) }
        model.jump = nil
        // The page drawn by the screen that holds the reader: the text, its paper and its fades, but
        // not the bars over it or a panel open on it.
        model.snapshot = { [weak view] in
            var holder: UIResponder? = view
            while let next = holder, !(next is UIViewController) { holder = next.next }
            guard let page = (holder as? UIViewController)?.view ?? view, page.bounds.width > 0 else { return nil }
            let size = page.bounds.size
            return UIGraphicsImageRenderer(size: size).image { _ in page.drawHierarchy(in: CGRect(origin: .zero, size: size), afterScreenUpdates: false) }
        }
        view.onLaidOut = { [weak coordinator = context.coordinator] in
            coordinator?.report()
            coordinator?.model.ready = true
        }
        return view
    }

    func updateUIView(_ view: ColumnTextView, context: Context) {
        context.coordinator.model = model
        if palette != context.coordinator.palette { context.coordinator.repaper(palette) }
        if model.rendered.text !== context.coordinator.coloured { context.coordinator.recolour() }
        context.coordinator.repaint()
        view.barsHidden = model.chromeHidden
        if let jump = model.jump {
            let motion = model.motion
            model.jump = nil
            model.motion = .instant
            context.coordinator.move(to: jump, motion)
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate, UIGestureRecognizerDelegate {
        var model: ReaderModel
        var palette: Palette
        /// The text whose colours the text view has. A new paper's text can come an update after
        /// its paper, so each is taken as it comes.
        var coloured: NSAttributedString
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
            coloured = model.rendered.text
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

        /// Puts the text on a new paper.
        func repaper(_ palette: Palette) {
            self.palette = palette
            guard let view else { return }
            view.backgroundColor = palette.ground
            view.tintColor = palette.link
            view.placeCap()
            unpaint()
        }

        /// Gives the text its colours for the new paper where it stands.
        func recolour() {
            coloured = model.rendered.text
            guard let view, let layout = view.textLayoutManager else { return }
            view.contentOffset.y += Layout.recolour(view.textStorage, to: coloured, in: layout, at: view.contentOffset.y + view.adjustedContentInset.top)
            unpaint()
        }

        /// Takes the highlights off, for `repaint` to put back in the paper's colour.
        private func unpaint() {
            guard let view else { return }
            Layout.paint(view.textStorage, base: model.rendered.text, from: painted, to: [], color: palette.highlight)
            painted = []
            paintedPassages = []
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
            model.touches += 1
            (scrollView as? ColumnTextView)?.forgetGoal()
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
            view?.placeCap()
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
            let anchor = ReaderModel.Anchor(character: character, within: 0)
            guard motion != .instant, view.bounds.width > 0 else {
                view.pending = anchor
                view.setNeedsLayout()
                return
            }
            stopDip()
            let still = UIAccessibility.isReduceMotionEnabled
            guard motion == .glide, !still else {
                // The text dips out at once and comes back in once the place is laid out.
                var landing: CGFloat?
                var faded = false
                let land = { [weak self] in
                    guard let self, faded, let y = landing else { return }
                    self.dipIn(view, at: y, still: still)
                }
                let out = UIViewPropertyAnimator(duration: 0.12, curve: .easeOut) { view.alpha = 0 }
                out.addCompletion { position in
                    guard position == .end else { return }
                    faded = true
                    land()
                }
                out.startAnimation()
                dip = out
                view.layOut(to: anchor) { y in
                    landing = y
                    land()
                }
                return
            }
            view.layOut(to: anchor) { [weak self] y in
                guard let self else { return }
                guard abs(y - view.contentOffset.y) < view.bounds.height * 2 else { return self.move(to: character, .fade) }
                let glide = UIViewPropertyAnimator(duration: 0.32, timingParameters: UISpringTimingParameters(duration: 0.32, bounce: 0))
                glide.addAnimations {
                    view.alpha = 1
                    view.transform = .identity
                    view.contentOffset.y = y
                }
                glide.startAnimation()
                self.dip = glide
            }
        }

        /// Puts the text at `y` while it is out of sight, then brings it in rising from the side the
        /// place is on, with a GPU transform rather than scrolling frame by frame.
        private func dipIn(_ view: ColumnTextView, at y: CGFloat, still: Bool) {
            let rise: CGFloat = still ? 0 : (y >= view.contentOffset.y ? 14 : -14)
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
            let y = view.contentOffset.y + view.adjustedContentInset.top
            let atEnd = view.contentOffset.y + view.bounds.height >= view.contentSize.height - 2
            if let anchor = Layout.anchor(at: y, in: layout) {
                model.anchor = anchor
                model.scrolled(toCharacter: anchor.character, atEnd: atEnd && view.contentOffset.y > 0)
                model.rendered.preparePictures(around: anchor.character)
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
    var pending: ReaderModel.Anchor?
    var onLaidOut: (() -> Void)?
    /// The text from its start to here is laid out, not estimated.
    private var laidOut = 0
    private var goal: (anchor: ReaderModel.Anchor, then: (CGFloat) -> Void)?
    private var laying: CADisplayLink?
    /// Paper over the text where it runs on under the bars, so it reads only below them. Made on
    /// first use: `init(usingTextLayoutManager:)` doesn't run Swift's property initialisers.
    private var cap: UIView?

    /// SwiftUI sets the same frame again each time the bars come or go, and UITextView answers any
    /// frame it is given by putting back the offset last set in code, a chapter's place, under
    /// the reader's finger.
    override var frame: CGRect {
        get { super.frame }
        set {
            guard newValue != super.frame else { return }
            super.frame = newValue
        }
    }

    override func layoutSubviews() {
        let side = max(24, ((bounds.width - column) / 2).rounded())
        let inset = UIEdgeInsets(top: 28, left: side, bottom: 96, right: side)
        if textContainerInset != inset { textContainerInset = inset }
        // The text fills the screen under the bars and keeps the most room they have asked for, so
        // the bars going away as the reader reads on never move the text.
        let bars = UIEdgeInsets(top: max(contentInset.top, safeAreaInsets.top), left: 0, bottom: max(contentInset.bottom, safeAreaInsets.bottom), right: 0)
        if bars != contentInset {
            let atTop = contentOffset.y <= -contentInset.top + 0.5
            contentInset = bars
            verticalScrollIndicatorInsets = bars
            if atTop { contentOffset.y = -bars.top }
        }
        super.layoutSubviews()
        placeCap()
        guard let anchor = pending, bounds.width > 0 else { return }
        pending = nil
        alpha = 0
        layOut(to: anchor) { [weak self] y in
            guard let self else { return }
            contentOffset.y = y
            UIView.animate(withDuration: 0.18, delay: 0, options: [.curveEaseOut, .allowUserInteraction]) { self.alpha = 1 }
            onLaidOut?()
        }
    }

    /// Whether the bars are away, so the text reads up to the status bar.
    var barsHidden = false {
        didSet {
            guard barsHidden != oldValue else { return }
            UIView.animate(withDuration: UIAccessibility.isReduceMotionEnabled ? 0 : 0.24, delay: 0, options: [.curveEaseOut, .allowUserInteraction]) { self.placeCap() }
        }
    }

    /// Keeps the paper over the text under the bars, as the text scrolls and the bars come and go.
    func placeCap() {
        let cap = cap ?? UIView()
        if cap.superview == nil {
            addSubview(cap)
            self.cap = cap
        }
        let top = barsHidden ? window?.safeAreaInsets.top ?? 0 : contentInset.top
        cap.backgroundColor = backgroundColor
        cap.frame = CGRect(x: 0, y: contentOffset.y, width: bounds.width, height: top)
        if subviews.last !== cap { bringSubviewToFront(cap) }
    }

    /// The offset that puts `anchor`'s line at the top of the column, with the inset above it
    /// kept, so the line clears the fade under the bars.
    func offset(for anchor: ReaderModel.Anchor) -> CGFloat? {
        guard let layout = textLayoutManager, let top = Layout.top(of: anchor, in: layout) else { return nil }
        let height = max(contentSize.height, layout.usageBoundsForTextContainer.height + textContainerInset.top + textContainerInset.bottom)
        let most = max(height - bounds.height + adjustedContentInset.bottom, -adjustedContentInset.top)
        return min(max(top - adjustedContentInset.top, -adjustedContentInset.top), most)
    }

    /// TextKit 2 lays out the text on screen and estimates the text before it. Where the text went
    /// straight to a place, the estimate turned real as the reader scrolled back and moved the text
    /// under the finger, and it put a page redrawn in a new look somewhere else. So the text is laid
    /// out from its start down past `anchor` first, a slice a frame so nothing stalls, and `then`
    /// gets the offset that puts the anchor's line at the top of the column.
    func layOut(to anchor: ReaderModel.Anchor, then: @escaping (CGFloat) -> Void) {
        goal = (anchor, then)
        guard !layOutSlices(), laying == nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        laying = link
    }

    /// A finger on the text wins over a place still being laid out.
    func forgetGoal() {
        goal = nil
    }

    @objc private func tick() {
        guard layOutSlices() else { return }
        laying?.invalidate()
        laying = nil
    }

    /// Lays out slices for a few milliseconds, and goes to the goal once it is laid out. True when
    /// nothing is left to lay out.
    private func layOutSlices() -> Bool {
        guard let goal, window != nil, let layout = textLayoutManager else { return true }
        let document = layout.documentRange
        let target = min(goal.anchor.character + 6_000, textStorage.length)
        let start = CACurrentMediaTime()
        while laidOut < target {
            if CACurrentMediaTime() - start > 0.005 { return false }
            let end = min(laidOut + 4_000, target)
            if let from = layout.location(document.location, offsetBy: laidOut), let to = layout.location(document.location, offsetBy: end),
               let slice = NSTextRange(location: from, end: to) {
                layout.ensureLayout(for: slice)
            }
            laidOut = end
        }
        self.goal = nil
        if let y = offset(for: goal.anchor) { goal.then(y) }
        return true
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
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.touched), name: NSScrollView.willStartLiveScrollNotification, object: scroll)
        text.pending = model.jump.map { ReaderModel.Anchor(character: $0, within: model.jumpWithin) }
        model.jump = nil
        model.snapshot = { [weak scroll] in
            guard let scroll, let bitmap = scroll.bitmapImageRepForCachingDisplay(in: scroll.bounds) else { return nil }
            scroll.cacheDisplay(in: scroll.bounds, to: bitmap)
            let image = NSImage(size: scroll.bounds.size)
            image.addRepresentation(bitmap)
            return image
        }
        text.onLaidOut = { [weak coordinator = context.coordinator] in coordinator?.jump() }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.model = model
        if palette != context.coordinator.palette { context.coordinator.repaper(palette) }
        if model.rendered.text !== context.coordinator.coloured { context.coordinator.recolour() }
        context.coordinator.repaint()
        if let jump = model.jump {
            let motion = model.motion
            context.coordinator.text?.pending = ReaderModel.Anchor(character: jump, within: 0)
            model.jump = nil
            model.motion = .instant
            context.coordinator.jump(motion)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var model: ReaderModel
        var palette: Palette
        /// The text whose colours the text view has. A new paper's text can come an update after
        /// its paper, so each is taken as it comes.
        var coloured: NSAttributedString
        weak var text: ColumnTextView?
        weak var scroll: NSScrollView?
        private var painted: [NSRange] = []
        private var paintedPassages: [Passage] = []
        private var jumpToken = 0

        init(model: ReaderModel, palette: Palette) {
            self.model = model
            self.palette = palette
            coloured = model.rendered.text
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

        /// Puts the text on a new paper.
        func repaper(_ palette: Palette) {
            self.palette = palette
            text?.backgroundColor = palette.ground
            scroll?.backgroundColor = palette.ground
            text?.selectedTextAttributes = [.backgroundColor: palette.selection]
            unpaint()
        }

        /// Gives the text its colours for the new paper where it stands.
        func recolour() {
            coloured = model.rendered.text
            guard let text, let scroll, let layout = text.textLayoutManager, let storage = text.textStorage else { return }
            let clip = scroll.contentView
            let moved = Layout.recolour(storage, to: coloured, in: layout, at: clip.bounds.minY + scroll.contentInsets.top)
            if moved != 0 {
                clip.scroll(to: NSPoint(x: 0, y: clip.bounds.minY + moved))
                scroll.reflectScrolledClipView(clip)
            }
            unpaint()
        }

        /// Takes the highlights off, for `repaint` to put back in the paper's colour.
        private func unpaint() {
            guard let storage = text?.textStorage else { return }
            Layout.paint(storage, base: model.rendered.text, from: painted, to: [], color: palette.highlight)
            painted = []
            paintedPassages = []
        }

        /// Moves the text so the pending character's line sits at the top of the column, with the
        /// inset above it kept.
        func jump(_ motion: ReaderModel.Motion = .instant) {
            guard let text, let scroll, let anchor = text.pending, text.bounds.width > 0, let layout = text.textLayoutManager else { return }
            text.pending = nil
            guard let top = Layout.top(of: anchor, in: layout) else { return }
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
                model.ready = true
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
            let y = visible.minY + scroll.contentInsets.top
            let atEnd = visible.maxY >= text.frame.height - 2 && visible.minY > 0
            if let anchor = Layout.anchor(at: y, in: layout) {
                model.anchor = anchor
                model.scrolled(toCharacter: anchor.character, atEnd: atEnd)
                model.rendered.preparePictures(around: anchor.character)
            }
        }

        /// A scroll by hand hurries a paper still spreading, which shows the text as it was.
        @objc func touched() {
            model.touches += 1
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
    var pending: ReaderModel.Anchor?
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

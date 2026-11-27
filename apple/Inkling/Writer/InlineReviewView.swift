import Foundation
import InklingCore
import SwiftUI

#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Builds the review `NSAttributedString` shared by Mac and iPhone, plus the UTF-16 range of each
/// change (for iPhone tap hit-testing) and inline controls on Mac.
enum ReviewComposer {
    struct Result {
        let attributed: NSAttributedString
        let changeRanges: [(range: NSRange, id: UInt32)]
        #if os(macOS)
        let macControls: [(location: Int, payload: ReviewControlPayload)]
        #endif
    }

    static func compose(
        segments: [ReviewSegment],
        nextPending: UInt32?,
        selectedId: UInt32?,
        style: WritingStyle,
        model: WritingModel
    ) -> Result {
        let out = NSMutableAttributedString()
        var changeRanges: [(NSRange, UInt32)] = []
        #if os(macOS)
        var macControls: [(Int, ReviewControlPayload)] = []
        #endif

        let bodyFont = Fonts.reading(style.face, size: style.size)
        let para = NSMutableParagraphStyle()
        let line = style.lineHeight * style.size
        para.minimumLineHeight = line
        para.maximumLineHeight = line
        para.lineBreakStrategy = .pushOut
        let offset = max(0, (line - (bodyFont.ascender - bodyFont.descender)) / 4)

        let base: [NSAttributedString.Key: Any] = [
            .font: bodyFont,
            .foregroundColor: style.palette.ink,
            .paragraphStyle: para,
            .baselineOffset: offset,
        ]

        func append(_ string: String, extra: [NSAttributedString.Key: Any] = [:]) {
            guard !string.isEmpty else { return }
            var attrs = base
            for (k, v) in extra { attrs[k] = v }
            out.append(NSAttributedString(string: string, attributes: attrs))
        }

        for segment in segments {
            switch segment {
            case .unchanged(let text):
                append(text)

            case .change(let change):
                let start = out.length
                let isSelected = (selectedId == change.id)

                func markSelected(_ attrs: inout [NSAttributedString.Key: Any]) {
                    #if os(iOS)
                    if isSelected {
                        attrs[.underlineStyle] = NSUnderlineStyle.thick.rawValue
                        attrs[.underlineColor] = style.palette.ink
                    }
                    #endif
                }

                switch change.decision {
                case .pending:
                    if !change.removed.isEmpty {
                        var attrs: [NSAttributedString.Key: Any] = [
                            .foregroundColor: style.palette.removedText,
                            .backgroundColor: style.palette.removedFill,
                            .strikethroughStyle: NSUnderlineStyle.single.rawValue,
                            .strikethroughColor: style.palette.removedLine,
                        ]
                        markSelected(&attrs)
                        append(change.removed, extra: attrs)
                    }
                    if !change.removed.isEmpty && !change.added.isEmpty {
                        var gapAttrs: [NSAttributedString.Key: Any] = [:]
                        markSelected(&gapAttrs)
                        append(" ", extra: gapAttrs)
                    }
                    if !change.added.isEmpty {
                        var attrs: [NSAttributedString.Key: Any] = [
                            .foregroundColor: style.palette.addedText,
                            .backgroundColor: style.palette.addedFill,
                        ]
                        markSelected(&attrs)
                        append(change.added, extra: attrs)
                    }

                case .accepted:
                    var attrs: [NSAttributedString.Key: Any] = [
                        .foregroundColor: style.palette.addedText,
                        .underlineStyle: NSUnderlineStyle.single.rawValue,
                        .underlineColor: style.palette.addedLine,
                    ]
                    markSelected(&attrs)
                    append(change.added, extra: attrs)

                case .rejected:
                    var attrs: [NSAttributedString.Key: Any] = [:]
                    markSelected(&attrs)
                    append(change.removed, extra: attrs)
                }

                let end = out.length
                changeRanges.append((NSRange(location: start, length: max(0, end - start)), change.id))

                #if os(macOS)
                let attachment = ReviewControlAttachment(
                    change: change,
                    isNextPending: nextPending == change.id,
                    palette: style.palette,
                    bodyFont: bodyFont,
                    model: model
                )
                // Glue the control to the word before with a narrow no-break space.
                append("\u{202F}")
                let attachLocation = out.length
                let attachStr = NSMutableAttributedString(attachment: attachment)
                attachStr.addAttributes([.paragraphStyle: para, .baselineOffset: offset], range: NSRange(location: 0, length: attachStr.length))
                out.append(attachStr)
                macControls.append((attachLocation, attachment.payload))
                #endif
            }
        }

        #if os(macOS)
        return Result(attributed: out, changeRanges: changeRanges, macControls: macControls)
        #else
        return Result(attributed: out, changeRanges: changeRanges)
        #endif
    }
}

// MARK: - macOS Inline Review (TextKit 2 + Native Inline Attachment Controls)

#if os(macOS)
struct InlineReviewView: NSViewRepresentable {
    let model: WritingModel
    let segments: [ReviewSegment]
    let nextPending: UInt32?
    let style: WritingStyle

    func makeCoordinator() -> Coordinator { Coordinator(model: model) }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = ReviewNSTextView(usingTextLayoutManager: true)
        textView.column = style.column
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = true
        textView.backgroundColor = style.palette.ground
        textView.selectedTextAttributes = [.backgroundColor: style.palette.selection]
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.lineFragmentPadding = 0
        textView.setAccessibilityIdentifier("review")
        textView.onAcceptNext = { [weak model] in model?.acceptNextPending() }
        textView.onFinish = { [weak model] in model?.finishReview() }

        let scroll = NSScrollView()
        scroll.documentView = textView
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = style.palette.ground
        scroll.wantsLayer = true

        context.coordinator.textView = textView
        apply(to: textView, scroll: scroll)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.model = model
        guard let textView = context.coordinator.textView else { return }
        textView.onAcceptNext = { [weak model] in model?.acceptNextPending() }
        textView.onFinish = { [weak model] in model?.finishReview() }
        apply(to: textView, scroll: scroll)
    }

    private func apply(to textView: ReviewNSTextView, scroll: NSScrollView) {
        let savedOffset = scroll.contentView.bounds.origin
        textView.column = style.column
        textView.backgroundColor = style.palette.ground
        scroll.backgroundColor = style.palette.ground
        let composed = ReviewComposer.compose(
            segments: segments,
            nextPending: nextPending,
            selectedId: model.selectedChangeId,
            style: style,
            model: model
        )
        textView.textStorage?.setAttributedString(composed.attributed)
        textView.setControls(composed.macControls)
        scroll.contentView.scroll(to: savedOffset)
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    final class Coordinator: NSObject {
        var model: WritingModel
        weak var textView: ReviewNSTextView?
        init(model: WritingModel) { self.model = model }
    }
}

final class ReviewNSTextView: NSTextView {
    var column: CGFloat = 700
    var onAcceptNext: (() -> Void)?
    var onFinish: (() -> Void)?
    private var controls: [(location: Int, payload: ReviewControlPayload, pill: InlineControlPill)] = []

    override var textContainerOrigin: NSPoint {
        NSPoint(x: textContainerInset.width, y: textContainerInset.height)
    }

    override func setFrameSize(_ size: NSSize) {
        let side = max(48, ((size.width - column) / 2).rounded())
        let inset = NSSize(width: side, height: 32)
        if textContainerInset != inset { textContainerInset = inset }
        super.setFrameSize(size)
        positionControls()
    }

    override func layout() {
        super.layout()
        positionControls()
    }

    func setControls(_ slots: [(location: Int, payload: ReviewControlPayload)]) {
        for item in controls { item.pill.removeFromSuperview() }
        controls = slots.map { slot in
            let pill = InlineControlPill(payload: slot.payload)
            addSubview(pill)
            return (slot.location, slot.payload, pill)
        }
        positionControls()
        DispatchQueue.main.async { [weak self] in self?.positionControls() }
    }

    private func positionControls() {
        guard let layout = textLayoutManager, bounds.width > 0 else { return }
        let origin = textContainerOrigin
        let docStart = layout.documentRange.location
        for item in controls {
            guard let loc = layout.location(docStart, offsetBy: item.location),
                  let prefix = NSTextRange(location: docStart, end: loc) else { continue }
            layout.ensureLayout(for: prefix)
            guard let fragment = layout.textLayoutFragment(for: loc) else { continue }
            let fragOrigin = fragment.layoutFragmentFrame.origin
            let size = item.payload.pillSize
            let attachRect = fragment.frameForTextAttachment(at: loc)
            if attachRect.width > 0 && attachRect.height > 0 {
                let x = origin.x + fragOrigin.x + attachRect.minX
                let y = origin.y + fragOrigin.y + attachRect.midY - size.height / 2.0
                item.pill.frame = CGRect(x: x.rounded(), y: y.rounded(), width: size.width, height: size.height)
            } else {
                let local = layout.offset(from: fragment.rangeInElement.location, to: loc)
                if let line = fragment.textLineFragments.first(where: { NSLocationInRange(local, $0.characterRange) }) ?? fragment.textLineFragments.last {
                    let pt = line.locationForCharacter(at: local)
                    let x = origin.x + fragOrigin.x + line.typographicBounds.minX + pt.x
                    let y = origin.y + fragOrigin.y + line.typographicBounds.minY + (line.typographicBounds.height - size.height) / 2.0
                    item.pill.frame = CGRect(x: x.rounded(), y: y.rounded(), width: size.width, height: size.height)
                }
            }
        }
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 48: // Tab -> accept next pending change
            onAcceptNext?()
        case 53: // Escape -> back to draft
            onFinish?()
        default:
            super.keyDown(with: event)
        }
    }
}

/// Immutable payload carried by `ReviewControlAttachment` across TextKit 2's nonisolated provider.
nonisolated struct ReviewControlPayload: @unchecked Sendable {
    let change: ReviewChange
    let isNextPending: Bool
    let palette: Palette
    let capHeight: CGFloat
    let pillSize: CGSize
    let modelRef: UncheckedModelRef
}

/// TextKit 2 attachment that reserves exact inline horizontal room right beside each change on
/// macOS while `ReviewNSTextView` mounts the real interactive `InlineControlPill` at that frame.
nonisolated final class ReviewControlAttachment: NSTextAttachment {
    let payload: ReviewControlPayload

    init(change: ReviewChange, isNextPending: Bool, palette: Palette, bodyFont: NSFont, model: WritingModel) {
        self.payload = ReviewControlPayload(
            change: change,
            isNextPending: isNextPending,
            palette: palette,
            capHeight: bodyFont.capHeight,
            pillSize: Self.measure(change: change),
            modelRef: UncheckedModelRef(model)
        )
        super.init(data: nil, ofType: nil)
        allowsTextAttachmentView = false
        lineLayoutPadding = 2
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override func image(forBounds imageBounds: CGRect, textContainer: NSTextContainer?, characterIndex charIndex: Int) -> NSImage? {
        nil
    }

    override func attachmentBounds(for attributes: [NSAttributedString.Key: Any], location: any NSTextLocation, textContainer: NSTextContainer?, proposedLineFragment: CGRect, position: CGPoint) -> CGRect {
        let size = payload.pillSize
        let yOffset = ((payload.capHeight - size.height) / 2.0).rounded()
        return CGRect(x: 0, y: yOffset, width: size.width, height: size.height)
    }

    private static func measure(change: ReviewChange) -> CGSize {
        switch change.decision {
        case .pending:
            let moreWidth: CGFloat = change.more > 0 ? 54 : 0
            return CGSize(width: 60 + moreWidth, height: 28)
        case .accepted:
            return CGSize(width: 92, height: 24)
        case .rejected:
            return CGSize(width: 108, height: 24)
        }
    }
}

nonisolated final class UncheckedModelRef: @unchecked Sendable {
    weak var model: WritingModel?
    init(_ model: WritingModel) { self.model = model }
}

/// Crisp native AppKit inline pill view (`✓` / `✕` / `n more`, or `✓ kept · Undo` / `✕ rejected · Undo`).
final class InlineControlPill: NSView {
    private let change: ReviewChange
    private let palette: Palette
    private weak var model: WritingModel?

    init(payload: ReviewControlPayload) {
        self.change = payload.change
        self.palette = payload.palette
        self.model = payload.modelRef.model
        super.init(frame: CGRect(origin: .zero, size: payload.pillSize))
        wantsLayer = true
        build(isNextPending: payload.isNextPending)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    private func build(isNextPending: Bool) {
        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 2
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)

        switch change.decision {
        case .pending:
            layer?.backgroundColor = palette.surface.cgColor
            layer?.borderColor = palette.hairline.cgColor
            layer?.borderWidth = 1
            layer?.cornerRadius = 8

            let accept = makeGlyphButton(
                title: "✓",
                color: palette.addedText,
                fill: isNextPending ? palette.addedFill : nil,
                id: "accept-\(change.id)",
                action: #selector(acceptTapped)
            )
            let reject = makeGlyphButton(
                title: "✕",
                color: palette.removedText,
                fill: nil,
                id: "reject-\(change.id)",
                action: #selector(rejectTapped)
            )
            stack.addArrangedSubview(accept)
            stack.addArrangedSubview(reject)

            if change.more > 0 {
                let more = makeTextButton(
                    title: "\(change.more) more",
                    color: palette.muted,
                    underline: false,
                    id: "more-\(change.id)",
                    action: #selector(moreTapped(_:))
                )
                stack.addArrangedSubview(more)
            }

            NSLayoutConstraint.activate([
                stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 2),
                stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -2),
                stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])

        case .accepted, .rejected:
            let isAccepted = change.decision == .accepted
            let label = NSTextField(labelWithString: isAccepted ? "✓ kept" : "✕ rejected")
            label.font = NSFont(name: Fonts.interMedium, size: 11) ?? .systemFont(ofSize: 11, weight: .medium)
            label.textColor = isAccepted ? palette.addedCount : palette.removedText

            let undo = makeTextButton(
                title: "Undo",
                color: palette.muted,
                underline: true,
                id: "undo-\(change.id)",
                action: #selector(undoTapped)
            )
            stack.spacing = 6
            stack.addArrangedSubview(label)
            stack.addArrangedSubview(undo)

            NSLayoutConstraint.activate([
                stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
                stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])
        }
    }

    private func makeGlyphButton(title: String, color: NSColor, fill: NSColor?, id: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.isBordered = false
        button.wantsLayer = true
        button.layer?.cornerRadius = 6
        if let fill { button.layer?.backgroundColor = fill.cgColor }
        let font = NSFont(name: Fonts.interSemibold, size: 13) ?? .systemFont(ofSize: 13, weight: .semibold)
        button.attributedTitle = NSAttributedString(string: title, attributes: [.font: font, .foregroundColor: color])
        button.setAccessibilityIdentifier(id)
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: 26),
            button.heightAnchor.constraint(equalToConstant: 22),
        ])
        return button
    }

    private func makeTextButton(title: String, color: NSColor, underline: Bool, id: String, action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.isBordered = false
        let font = NSFont(name: Fonts.interMedium, size: 11) ?? .systemFont(ofSize: 11, weight: .medium)
        var attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
        if underline { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        button.attributedTitle = NSAttributedString(string: title, attributes: attrs)
        button.setAccessibilityIdentifier(id)
        return button
    }

    @objc private func acceptTapped() { model?.acceptChange(change.id) }
    @objc private func rejectTapped() { model?.rejectChange(change.id) }
    @objc private func undoTapped() { model?.undoChange(change.id) }

    @objc private func moreTapped(_ sender: NSButton) {
        let menu = NSMenu()
        for (idx, wording) in change.wordings.enumerated() {
            let item = NSMenuItem(title: wording, action: #selector(wordingPicked(_:)), keyEquivalent: "")
            item.target = self
            item.tag = idx
            item.state = (UInt32(idx) == change.chosen) ? .on : .off
            menu.addItem(item)
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 4), in: sender)
    }

    @objc private func wordingPicked(_ sender: NSMenuItem) {
        model?.chooseWording(change.id, wording: UInt32(sender.tag))
    }
}

// MARK: - iOS Inline Review (TextKit 2 Tap-to-Select + Native Bottom Sheet Bar)

#else
struct InlineReviewView: UIViewRepresentable {
    let model: WritingModel
    let segments: [ReviewSegment]
    let nextPending: UInt32?
    let style: WritingStyle

    func makeCoordinator() -> Coordinator { Coordinator(model: model) }

    func makeUIView(context: Context) -> ReviewUITextView {
        let view = ReviewUITextView(usingTextLayoutManager: true)
        view.column = style.column
        view.isEditable = false
        view.isSelectable = false
        view.backgroundColor = style.palette.ground
        view.textContainer.lineFragmentPadding = 0
        view.alwaysBounceVertical = true
        view.accessibilityIdentifier = "review"

        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped(_:)))
        view.addGestureRecognizer(tap)
        context.coordinator.view = view
        apply(to: view, coordinator: context.coordinator)
        return view
    }

    func updateUIView(_ view: ReviewUITextView, context: Context) {
        context.coordinator.model = model
        apply(to: view, coordinator: context.coordinator)
    }

    private func apply(to view: ReviewUITextView, coordinator: Coordinator) {
        let savedOffset = view.contentOffset
        view.column = style.column
        view.backgroundColor = style.palette.ground
        let composed = ReviewComposer.compose(
            segments: segments,
            nextPending: nextPending,
            selectedId: model.selectedChangeId,
            style: style,
            model: model
        )
        coordinator.changeRanges = composed.changeRanges
        view.attributedText = composed.attributed
        if view.contentSize.height > 0 {
            view.contentOffset = savedOffset
        }
    }

    final class Coordinator: NSObject {
        var model: WritingModel
        weak var view: ReviewUITextView?
        var changeRanges: [(range: NSRange, id: UInt32)] = []

        init(model: WritingModel) { self.model = model }

        @objc func tapped(_ gesture: UITapGestureRecognizer) {
            guard let view else { return }
            let point = gesture.location(in: view)
            guard let pos = view.closestPosition(to: point) else { return }
            let offset = view.offset(from: view.beginningOfDocument, to: pos)
            if let hit = changeRanges.first(where: { NSLocationInRange(offset, $0.range) || (offset > 0 && NSLocationInRange(offset - 1, $0.range)) }) {
                model.selectedChangeId = hit.id
            }
        }
    }
}

final class ReviewUITextView: UITextView {
    var column: CGFloat = 700

    override func layoutSubviews() {
        let side = max(22, ((bounds.width - column) / 2).rounded())
        let inset = UIEdgeInsets(top: 16, left: side, bottom: 24, right: side)
        if textContainerInset != inset { textContainerInset = inset }
        super.layoutSubviews()
    }
}

/// iPhone bottom action card for the tapped change: Keep / Use (>= 44 pt touch targets), previous /
/// next step buttons, alternative wordings for Find a word, and Undo.
struct PhoneReviewSheet: View {
    let model: WritingModel
    let segments: [ReviewSegment]
    let palette: Palette

    private var changes: [ReviewChange] {
        segments.compactMap {
            if case .change(let c) = $0 { return c }
            return nil
        }
    }

    private var currentIndex: Int {
        guard let selected = model.selectedChangeId else { return 0 }
        return changes.firstIndex(where: { $0.id == selected }) ?? 0
    }

    var body: some View {
        if !changes.isEmpty {
            let change = changes[currentIndex]
            VStack(spacing: 12) {
                HStack(spacing: 8) {
                    Text("Change \(currentIndex + 1) of \(changes.count)")
                        .font(.inter(13, weight: .medium, relativeTo: .footnote))
                        .foregroundStyle(Color(palette.muted))
                        .monospacedDigit()
                    Spacer()
                    Button("Back to draft") { model.finishReview() }
                        .font(.inter(13, weight: .medium, relativeTo: .footnote))
                        .foregroundStyle(Color(palette.muted))
                        .frame(minHeight: 44)
                        .padding(.horizontal, 6)
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("back-to-draft")
                    Button { model.selectStep(-1) } label: {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color(palette.ink))
                            .frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Previous change")
                    .accessibilityIdentifier("previous-change")
                    Button { model.selectStep(1) } label: {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(Color(palette.ink))
                            .frame(width: 44, height: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Next change")
                    .accessibilityIdentifier("next-change")
                }

                HStack(spacing: 10) {
                    Button {
                        model.rejectChange(change.id)
                    } label: {
                        Text(keepLabel(change))
                            .font(.inter(15, weight: .semibold, relativeTo: .body))
                            .foregroundStyle(Color(palette.removedText))
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .padding(.horizontal, 12)
                            .background(
                                RoundedRectangle(cornerRadius: 12)
                                    .fill(Color(palette.surface))
                                    .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.hairline), lineWidth: 1) }
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("review-keep")

                    Button {
                        model.acceptChange(change.id)
                    } label: {
                        Text(useLabel(change))
                            .font(.inter(15, weight: .semibold, relativeTo: .body))
                            .foregroundStyle(Color(palette.addedText))
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .padding(.horizontal, 12)
                            .background(
                                RoundedRectangle(cornerRadius: 12)
                                    .fill(Color(palette.addedFill))
                                    .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.addedLine).opacity(0.45), lineWidth: 1) }
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("review-use")
                }

                if change.more > 0 {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(Array(change.wordings.enumerated()), id: \.offset) { idx, word in
                                let chosen = UInt32(idx) == change.chosen
                                Button {
                                    model.chooseWording(change.id, wording: UInt32(idx))
                                } label: {
                                    Text(chosen ? "✓ \(word)" : word)
                                        .font(.inter(14, weight: chosen ? .semibold : .medium, relativeTo: .callout))
                                        .foregroundStyle(Color(chosen ? palette.addedText : palette.ink))
                                        .padding(.horizontal, 12)
                                        .frame(minHeight: 44)
                                        .background(
                                            Capsule()
                                                .fill(Color(chosen ? palette.addedFill : palette.ground))
                                                .overlay { Capsule().strokeBorder(Color(palette.hairline), lineWidth: 1) }
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }

                if change.decision != .pending {
                    Button {
                        model.undoChange(change.id)
                    } label: {
                        Text("Undo")
                            .font(.inter(14, weight: .medium, relativeTo: .callout))
                            .foregroundStyle(Color(palette.muted))
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .background(
                                RoundedRectangle(cornerRadius: 10)
                                    .strokeBorder(Color(palette.hairline), lineWidth: 1)
                            )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("review-undo")
                }
            }
            .padding(.horizontal, 18)
            .padding(.top, 10)
            .padding(.bottom, 14)
            .background(
                UnevenRoundedRectangle(topLeadingRadius: 18, topTrailingRadius: 18)
                    .fill(Color(palette.surface))
                    .overlay(alignment: .top) { Rectangle().fill(Color(palette.hairline)).frame(height: 1) }
                    .ignoresSafeArea(edges: .bottom)
            )
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("phone-review-sheet")
        }
    }

    private func keepLabel(_ change: ReviewChange) -> String {
        let removed = change.removed.trimmingCharacters(in: .whitespacesAndNewlines)
        return removed.isEmpty ? "✕ Leave it out" : "✕ Keep “\(removed)”"
    }

    private func useLabel(_ change: ReviewChange) -> String {
        let removed = change.removed.trimmingCharacters(in: .whitespacesAndNewlines)
        let added = change.added.trimmingCharacters(in: .whitespacesAndNewlines)
        return added.isEmpty ? "✓ Cut “\(removed)”" : "✓ Use “\(added)”"
    }
}
#endif

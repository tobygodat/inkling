import InklingCore
import SwiftUI

/// The Ask bar at rest: + for suggestions and attachments, and a pill that opens the composer.
/// On iPhone it is the tab bar's bottom accessory; on the Mac it floats at the foot of the page.
struct AskBar: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette

    var body: some View {
        let asker = shelf.asker
        HStack(spacing: 2) {
            Button {
                asker.openAdd()
            } label: {
                Image(systemName: "plus")
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(Color(palette.ink))
                    .frame(width: 40, height: 40)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Add to question")
            .accessibilityIdentifier("ask-add")

            Button {
                asker.openComposer()
            } label: {
                HStack(spacing: 8) {
                    Text(asker.busy ? "Answering…" : asker.prompt)
                        .font(.inter(15, relativeTo: .body))
                        #if os(macOS)
                        .foregroundStyle(Color(palette.ink).opacity(0.72))
                        #else
                        .foregroundStyle(Color(palette.muted))
                        #endif
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if asker.busy {
                        ProgressView()
                            .controlSize(.small)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 40)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(asker.prompt)
            .accessibilityIdentifier("ask-bar")
        }
        .padding(.leading, 4)
        .padding(.trailing, 16)
    }
}

/// The conversation and the composer under it: a sheet on iPhone, the bar grown upward on the Mac.
struct AskPanel: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    @FocusState private var focused: Bool
    /// How tall the conversation is, so the Mac's panel grows only as tall as it needs.
    @State private var height: CGFloat = 0

    var body: some View {
        let asker = shelf.asker
        let empty = asker.turns.isEmpty && asker.pending == nil && asker.problem == nil
        VStack(spacing: 0) {
            if !empty || isPhone {
                transcript(asker)
                if !isPhone {
                    Divider().overlay(Color(palette.hairline))
                }
            }

            Composer(focused: $focused)
                .padding(.horizontal, isPhone ? 12 : 0)
                .padding(.bottom, isPhone ? 12 : 0)
        }
        .background(Color(isPhone ? palette.ground : palette.surface))
        .onAppear { focused = asker.turns.isEmpty }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ask-panel")
    }

    private var isPhone: Bool {
        #if os(iOS)
        true
        #else
        false
        #endif
    }

    private func transcript(_ asker: Asker) -> some View {
        ScrollView {
                LazyVStack(alignment: .leading, spacing: 30) {
                    ForEach(Array(asker.turns.enumerated()), id: \.offset) { _, turn in
                        TurnView(turn: turn, streaming: false, open: open)
                    }
                    if let pending = asker.pending {
                        TurnView(turn: pending, streaming: true, open: open)
                    }
                    if let problem = asker.problem {
                        HStack(spacing: 10) {
                            Text(problem)
                                .font(.inter(13, relativeTo: .footnote))
                                .foregroundStyle(Color(palette.muted))
                            if asker.missingKey {
                                Button("Add key…") { asker.choosingProvider = true }
                                    .font(.inter(13, weight: .semibold, relativeTo: .footnote))
                                    .foregroundStyle(Color(palette.link))
                                    .buttonStyle(.plain)
                            }
                        }
                        .accessibilityIdentifier("ask-problem")
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 22)
                .padding(.bottom, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height = $0 }
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            #if os(macOS)
            .background(Color(palette.ground))
            .frame(height: min(max(height, 1), 440))
            #endif
    }

    private func open(_ citation: Citation) {
        guard let item = shelf.item(citation.item) else { return }
        shelf.asker.composing = false
        navigator.open(item, at: Passage(chapter: citation.chapter, start: citation.start, end: citation.end, words: citation.words, made: 0))
    }
}

/// The field, what goes with the question, +, the provider and Send.
private struct Composer: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    var focused: FocusState<Bool>.Binding

    var body: some View {
        @Bindable var asker = shelf.asker
        let empty = asker.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        Chip(symbol: symbol(asker.place), words: asker.label)
                        if asker.selected != nil {
                            Chip(symbol: "text.cursor", words: "Selection", on: asker.withSelection) { asker.withSelection.toggle() }
                                .accessibilityIdentifier("ask-selection")
                        }
                        ForEach(asker.attached, id: \.id) { item in
                            Chip(symbol: item.kind == .article ? "doc.text" : "book.closed", words: item.title, on: true) {
                                asker.attached.removeAll { $0.id == item.id }
                            }
                        }
                    }
                }
                .scrollClipDisabled()
                #if os(macOS)
                Button {
                    asker.composing = false
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color(palette.muted))
                        .frame(width: 24, height: 24)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
                .accessibilityIdentifier("ask-close")
                #endif
            }

            TextField(asker.prompt, text: $asker.draft, axis: vertical ? .vertical : .horizontal)
                .lineLimit(1...6)
                .textFieldStyle(.plain)
                .font(.inter(16, relativeTo: .body))
                .foregroundStyle(Color(palette.ink))
                .focused(focused)
                .onSubmit { asker.send() }
                .submitLabel(.send)
                .accessibilityIdentifier("ask-field")

            HStack(spacing: 8) {
                Button {
                    asker.openAdd()
                } label: {
                    Image(systemName: "plus")
                        .font(.system(size: 15, weight: .medium))
                        .frame(width: 32, height: 32)
                        .background(Circle().fill(Color(palette.segment)))
                }
                .buttonStyle(.plain)
                .foregroundStyle(Color(palette.ink))
                .accessibilityLabel("Add to question")
                .accessibilityIdentifier("ask-composer-add")

                Button {
                    asker.choosingProvider = true
                } label: {
                    HStack(spacing: 4) {
                        Text(providerLabel(provider: shelf.ai.provider()))
                            .font(.inter(13, weight: .medium, relativeTo: .footnote))
                        Image(systemName: "chevron.down")
                            .font(.system(size: 9, weight: .semibold))
                    }
                    .foregroundStyle(Color(palette.muted))
                    .padding(.horizontal, 10)
                    .frame(height: 32)
                    .background(Capsule().fill(Color(palette.segment)))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("ask-provider")

                Spacer(minLength: 0)

                Button {
                    if asker.busy { asker.cancel() } else { asker.send() }
                } label: {
                    Image(systemName: asker.busy ? "stop.fill" : "arrow.up")
                        .font(.system(size: asker.busy ? 11 : 14, weight: .bold))
                        .foregroundStyle(Color(empty && !asker.busy ? palette.muted : palette.surface))
                        .frame(width: 32, height: 32)
                        .background(Circle().fill(Color(empty && !asker.busy ? palette.segment : palette.ink)))
                        .contentTransition(.symbolEffect(.replace))
                }
                .buttonStyle(.plain)
                .disabled(empty && !asker.busy)
                .accessibilityLabel(asker.busy ? "Stop" : "Send")
                .accessibilityIdentifier("ask-send")
            }
        }
        .padding(vertical ? 12 : 14)
        .background {
            if vertical {
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .fill(Color(palette.surface))
                    .overlay { RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Color(palette.hairline), lineWidth: 1) }
            } else {
                Color(palette.surface)
            }
        }
    }

    private var vertical: Bool {
        #if os(iOS)
        true
        #else
        false
        #endif
    }

    private func symbol(_ place: AskPlace) -> String {
        switch place {
        case .screen(let section): section.symbol
        case .reading(let model): model.item.kind == .article ? "doc.text" : "book.closed"
        case .writing: "pencil.line"
        }
    }
}

/// What goes with a question: the screen it is asked from, the selection, an attached book.
private struct Chip: View {
    @Environment(\.palette) private var palette
    let symbol: String
    let words: String
    var on: Bool? = nil
    var toggle: (() -> Void)? = nil

    var body: some View {
        let label = HStack(spacing: 5) {
            Image(systemName: symbol)
                .font(.system(size: 11, weight: .medium))
            Text(words)
                .font(.inter(12, weight: .medium, relativeTo: .caption))
                .lineLimit(1)
            if let on {
                Image(systemName: on ? "xmark" : "plus")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Color(palette.muted))
            }
        }
        .foregroundStyle(Color(on == false ? palette.muted : palette.ink))
        .padding(.horizontal, 9)
        .frame(height: 26)
        .background(Capsule().fill(Color(palette.segment)))
        .frame(maxWidth: 240, alignment: .leading)

        if let toggle {
            Button(action: toggle) { label }
                .buttonStyle(.plain)
                .frame(minHeight: 44)
        } else {
            label
                .accessibilityIdentifier("ask-context")
        }
    }
}

/// One turn: where it was asked, the question, and the answer with its citations.
struct TurnView: View {
    @Environment(\.palette) private var palette
    let turn: Turn
    let streaming: Bool
    let open: (Citation) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(turn.context)
                .font(.inter(12, weight: .medium, relativeTo: .caption))
                .foregroundStyle(Color(palette.muted))
                .lineLimit(1)
            Text(turn.asked)
                .font(.inter(15, weight: .semibold, relativeTo: .body))
                .foregroundStyle(Color(palette.ink))
                .fixedSize(horizontal: false, vertical: true)
            if turn.edit {
                Text(turn.answer)
                    .font(.inter(14, relativeTo: .callout))
                    .foregroundStyle(Color(palette.muted))
            } else if streaming && turn.answer.isEmpty {
                ProgressView()
                    .controlSize(.small)
                    .padding(.vertical, 4)
            } else {
                Text(Self.marked(turn.answer, palette: palette))
                    .font(.serif(17, relativeTo: .body))
                    .lineSpacing(5)
                    .foregroundStyle(Color(palette.ink))
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 2)
            }
            if !turn.citations.isEmpty {
                VStack(spacing: 6) {
                    ForEach(turn.citations, id: \.n) { citation in
                        CitationRow(citation: citation) { open(citation) }
                    }
                }
                .padding(.top, 6)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(streaming ? "ask-pending" : "ask-turn")
    }

    /// The answer with each citation number raised and in Link.
    static func marked(_ answer: String, palette: Palette) -> AttributedString {
        var out = AttributedString(answer)
        for match in answer.matches(of: /\[(\d+)\]/).reversed() {
            guard let lower = AttributedString.Index(match.range.lowerBound, within: out),
                  let upper = AttributedString.Index(match.range.upperBound, within: out) else { continue }
            var mark = AttributedString(String(match.output.1))
            mark.font = .inter(11, weight: .semibold, relativeTo: .caption)
            mark.foregroundColor = Color(palette.link)
            mark.baselineOffset = 6
            out.replaceSubrange(lower..<upper, with: mark)
        }
        return out
    }
}

/// A cited passage: its number, where it is, and its opening words. Choosing it opens it.
private struct CitationRow: View {
    @Environment(\.palette) private var palette
    let citation: Citation
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("\(citation.n)")
                    .font(.inter(12, weight: .semibold, relativeTo: .caption))
                    .foregroundStyle(Color(palette.link))
                    .frame(width: 14, alignment: .leading)
                VStack(alignment: .leading, spacing: 3) {
                    Text(citation.place.isEmpty ? citation.title : "\(citation.title) · \(citation.place)")
                        .font(.inter(13, weight: .medium, relativeTo: .footnote))
                        .foregroundStyle(Color(palette.ink))
                        .lineLimit(1)
                    Text(citation.words)
                        .font(.custom(Fonts.serifItalic, size: 14, relativeTo: .callout))
                        .foregroundStyle(Color(palette.muted))
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                }
                Spacer(minLength: 0)
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Color(palette.muted))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color(palette.segment)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("citation-\(citation.n)")
    }
}

/// The + sheet: what fits the screen, as large tiles, then what can go with the question.
struct AskAddSheet: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @State private var choosing = false

    var body: some View {
        let asker = shelf.asker
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Text("Add to question")
                    .font(.inter(15, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Color(palette.ink))
                Spacer()
                Button {
                    asker.adding = false
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color(palette.muted))
                        .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
                .accessibilityIdentifier("ask-add-close")
            }
            .padding(.bottom, -8)

            if !asker.suggestions.isEmpty {
                HStack(spacing: 10) {
                    ForEach(asker.suggestions) { suggestion in
                        Button {
                            asker.run(suggestion)
                        } label: {
                            VStack(spacing: 8) {
                                Image(systemName: suggestion.symbol)
                                    .font(.system(size: 18, weight: .regular))
                                Text(suggestion.title)
                                    .font(.inter(13, weight: .medium, relativeTo: .footnote))
                            }
                            .foregroundStyle(Color(palette.ink))
                            .frame(maxWidth: .infinity, minHeight: 78)
                            .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color(palette.segment)))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("suggest-\(suggestion.rawValue)")
                    }
                }
            }

            VStack(spacing: 0) {
                if asker.selected != nil {
                    row(symbol: "text.cursor", words: "The selection", trailing: asker.withSelection ? "checkmark" : nil) {
                        asker.withSelection.toggle()
                    }
                    Divider().overlay(Color(palette.hairline)).padding(.leading, 44)
                }
                row(symbol: "book.closed", words: "A book or article", trailing: "chevron.right") { choosing = true }
                    .accessibilityIdentifier("attach-item")
            }
            .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color(palette.segment)))
        }
        .padding(.horizontal, 20)
        .padding(.top, 14)
        .padding(.bottom, 20)
        .frame(maxWidth: .infinity, alignment: .top)
        .background(Color(palette.ground))
        .sheet(isPresented: $choosing) {
            AttachList()
                .environment(\.palette, palette)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("ask-add-sheet")
    }

    private func row(symbol: String, words: String, trailing: String?, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: symbol)
                    .font(.system(size: 15))
                    .frame(width: 22)
                Text(words)
                    .font(.inter(15, relativeTo: .body))
                Spacer()
                if let trailing {
                    Image(systemName: trailing)
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(Color(palette.muted))
                }
            }
            .foregroundStyle(Color(palette.ink))
            .padding(.horizontal, 14)
            .frame(minHeight: 48)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// Every book and article, to attach one or more to the question.
private struct AttachList: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        let asker = shelf.asker
        let words = query.trimmingCharacters(in: .whitespaces)
        let items = (shelf.books + shelf.inbox).filter { words.isEmpty || $0.title.localizedStandardContains(words) || ($0.author ?? "").localizedStandardContains(words) }
        NavigationStack {
            List(items, id: \.id) { item in
                let on = asker.attached.contains { $0.id == item.id }
                Button {
                    if on { asker.attached.removeAll { $0.id == item.id } } else { asker.attached.append(item) }
                } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(item.title)
                                .font(.serif(16, weight: .semibold, relativeTo: .body))
                                .foregroundStyle(Color(palette.ink))
                            if let by = item.author ?? item.site {
                                Text(by)
                                    .font(.inter(12, relativeTo: .caption))
                                    .foregroundStyle(Color(palette.muted))
                            }
                        }
                        Spacer()
                        if on { Image(systemName: "checkmark").foregroundStyle(Color(palette.ink)) }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .listRowBackground(Color(palette.ground))
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Color(palette.ground))
            .searchable(text: $query, prompt: "Titles and authors")
            .navigationTitle("Attach")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        dismiss()
                        if !asker.attached.isEmpty && !asker.composing {
                            asker.adding = false
                            asker.composing = true
                        }
                    }
                }
            }
        }
        #if os(macOS)
        .frame(width: 420, height: 480)
        #endif
    }
}

#if os(macOS)
/// The Ask bar on the Mac, at the foot of the page: the pill, which grows into the conversation
/// and the composer, or the + sheet.
struct MacAskDock: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.palette) private var palette
    @Environment(\.accessibilityReduceMotion) private var still

    /// DESIGN.md's one motion: 200 ms on a quick ease-out, growing from the bar it came from.
    private var grow: AnyTransition {
        still ? .opacity : .scale(scale: 0.96, anchor: .bottom).combined(with: .opacity)
    }

    var body: some View {
        let asker = shelf.asker
        Group {
            if asker.adding {
                AskAddSheet()
                    .fixedSize(horizontal: false, vertical: true)
                    .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                    .overlay { RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Color(palette.hairline), lineWidth: 1) }
                    .shadow(color: .black.opacity(palette.isDark ? 0.35 : 0.1), radius: 24, y: 10)
                    .onExitCommand { asker.adding = false }
                    .transition(grow)
            } else if asker.composing {
                AskPanel()
                    .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                    .overlay { RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Color(palette.hairline), lineWidth: 1) }
                    .shadow(color: .black.opacity(palette.isDark ? 0.35 : 0.1), radius: 24, y: 10)
                    .onExitCommand { asker.composing = false }
                    .transition(grow)
            } else {
                AskBar()
                    .frame(height: 44)
                    .glassEffect(.regular.tint(Color(palette.surface).opacity(palette.isDark ? 0.65 : 0.78)), in: Capsule())
                    .transition(.opacity)
            }
        }
        .frame(maxWidth: 620)
        .padding(.horizontal, 24)
        .padding(.bottom, 16)
        .animation(.timingCurve(0.23, 1, 0.32, 1, duration: 0.2), value: asker.composing)
        .animation(.timingCurve(0.23, 1, 0.32, 1, duration: 0.2), value: asker.adding)
        .sheet(isPresented: Binding(get: { asker.choosingProvider }, set: { asker.choosingProvider = $0 })) {
            ProviderPanel()
                .frame(width: 420, height: 300)
        }
    }
}
#endif

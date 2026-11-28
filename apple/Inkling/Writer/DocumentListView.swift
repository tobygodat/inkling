import InklingCore
import SwiftUI

/// The writer's documents: newest first, with title, a quiet excerpt line, relative date, New
/// document action, delete and undo.
struct DocumentListView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    #if os(macOS)
    @Environment(\.appearsActive) private var active
    #endif

    var body: some View {
        @Bindable var navigator = navigator
        ZStack(alignment: .bottom) {
            Group {
                if shelf.docs.isEmpty {
                    ContentUnavailableView {
                        Label("Nothing written yet", systemImage: "pencil.line")
                    } description: {
                        Text("Start a new page. Your drafts save automatically as plain Markdown.")
                    } actions: {
                        Button("New Document") { createAndOpen() }
                            .buttonStyle(.glassProminent)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color(palette.ground))
                } else {
                    #if os(macOS)
                    List(selection: $navigator.document) {
                        ForEach(shelf.docs, id: \.id) { doc in
                            DocumentRow(doc: doc, excerpt: shelf.documentExcerpt(doc.id))
                                .tag(doc.id)
                                .contextMenu {
                                    Button("Delete", systemImage: "trash", role: .destructive) {
                                        delete(doc)
                                    }
                                }
                                .listRowSeparatorTint(Color(palette.hairline))
                                .listRowBackground(selectionBackground(doc))
                                .background(DocumentQuietSelection())
                        }
                    }
                    #else
                    List {
                        ForEach(shelf.docs, id: \.id) { doc in
                            NavigationLink(value: DocumentRoute(id: doc.id)) {
                                DocumentRow(doc: doc, excerpt: shelf.documentExcerpt(doc.id))
                            }
                            .navigationLinkIndicatorVisibility(.hidden)
                            .listRowBackground(Color(palette.ground))
                            .listRowSeparatorTint(Color(palette.hairline))
                            .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                                Button("Delete", systemImage: "trash", role: .destructive) {
                                    delete(doc)
                                }
                            }
                            .accessibilityIdentifier("doc-\(doc.title.isEmpty ? "Untitled" : doc.title)")
                        }
                    }
                    #endif
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Color(palette.ground))

            if shelf.canUndoDeleteDocument {
                undoBanner
                    .padding(.horizontal, 16)
                    .padding(.bottom, 16)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .animation(.snappy(duration: 0.25), value: shelf.canUndoDeleteDocument)
        #if os(iOS)
        .refreshable {
            if shelf.account.signedIn {
                await shelf.account.syncNowAsync()
            }
        }
        #endif
        .navigationTitle("Writing")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("New Document", systemImage: "square.and.pencil") { createAndOpen() }
                    .accessibilityIdentifier("new-document")
            }
        }
    }

    private var undoBanner: some View {
        HStack(spacing: 12) {
            Text("Document deleted")
                .font(.inter(13, weight: .medium, relativeTo: .callout))
                .foregroundStyle(Color(palette.ink))
            Spacer(minLength: 0)
            Button("Undo") {
                if let restored = shelf.undoDeleteDocument() {
                    navigator.document = restored
                }
            }
            .font(.inter(13, weight: .semibold, relativeTo: .callout))
            .foregroundStyle(Color(palette.link))
            .buttonStyle(.plain)
            .accessibilityIdentifier("undo-delete-doc")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color(palette.surface))
                .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.hairline), lineWidth: 1) }
                .shadow(color: .black.opacity(palette.isDark ? 0.35 : 0.08), radius: 10, y: 4)
        )
    }

    private func createAndOpen() {
        let id = shelf.createDocument()
        navigator.document = id
        #if os(iOS)
        navigator.phoneDocument = DocumentRoute(id: id)
        #endif
    }

    private func delete(_ doc: DocInfo) {
        let wasSelected = navigator.document == doc.id
        shelf.deleteDocument(doc)
        if wasSelected {
            navigator.document = shelf.docs.first?.id
        }
    }

    #if os(macOS)
    @ViewBuilder private func selectionBackground(_ doc: DocInfo) -> some View {
        if navigator.document == doc.id {
            RoundedRectangle(cornerRadius: 8)
                .fill(active ? Color(palette.ink).opacity(0.08) : Color(palette.segment))
                .padding(.horizontal, 8)
        } else {
            Color.clear
        }
    }
    #endif
}

/// One row in the documents list: title in Source Serif 4 semibold, optional quiet excerpt line,
/// and the day it last changed.
private struct DocumentRow: View {
    @Environment(\.palette) private var palette
    let doc: DocInfo
    let excerpt: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(doc.title.isEmpty ? "Untitled" : doc.title)
                    .font(.serif(18, weight: .semibold, relativeTo: .headline))
                    .foregroundStyle(Color(doc.title.isEmpty ? palette.muted : palette.ink))
                    .lineLimit(1)
                Spacer(minLength: 4)
                Text(Self.dayLabel(doc.changedMs))
                    .font(.inter(12, relativeTo: .caption))
                    .foregroundStyle(Color(palette.muted))
                    .monospacedDigit()
            }
            if let excerpt {
                Text(excerpt)
                    .font(.inter(13, relativeTo: .subheadline))
                    .foregroundStyle(Color(palette.muted))
                    .lineLimit(2)
                    .lineSpacing(2)
            }
        }
        .padding(.vertical, 13)
        .contentShape(.rect)
        .accessibilityElement(children: .combine)
    }

    static func dayLabel(_ millis: UInt64) -> String {
        let date = Date(timeIntervalSince1970: Double(millis) / 1000.0)
        let cal = Calendar.current
        if cal.isDateInToday(date) { return "Today" }
        if cal.isDateInYesterday(date) { return "Yesterday" }
        let sameYear = cal.component(.year, from: date) == cal.component(.year, from: Date())
        let formatter = DateFormatter()
        formatter.dateFormat = sameYear ? "d MMM" : "d MMM yyyy"
        return formatter.string(from: date)
    }
}

/// Navigation value for pushing a document on iPhone (distinct from a book/article `String` id).
struct DocumentRoute: Hashable {
    let id: String
}

#if os(macOS)
private struct DocumentQuietSelection: NSViewRepresentable {
    func makeNSView(context: Context) -> Probe { Probe() }
    func updateNSView(_ view: Probe, context: Context) {}

    final class Probe: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            var view = superview
            while let next = view, !(next is NSTableView) { view = next.superview }
            (view as? NSTableView)?.selectionHighlightStyle = .none
        }
    }
}
#endif

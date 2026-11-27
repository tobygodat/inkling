import InklingCore
import SwiftUI
import UniformTypeIdentifiers
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// The sidebar's sections of the library.
public enum LibrarySection: String, CaseIterable, Identifiable, Hashable, Sendable {
    case all = "All"
    case articles = "Articles"
    case books = "Books"

    public var id: String { rawValue }

    var symbol: String {
        switch self {
        case .all: "books.vertical"
        case .articles: "doc.richtext"
        case .books: "book.closed"
        }
    }

    func holds(_ item: Item) -> Bool {
        switch self {
        case .all: true
        case .articles: item.kind == .article
        case .books: item.kind != .article
        }
    }
}

/// The app's one window: on the Mac the sidebar, the library's rows and the reader side by side;
/// on iPhone the library, which opens the reader.
public struct RootView: View {
    @Environment(Shelf.self) private var shelf
    @State private var section: LibrarySection? = .all
    @State private var selection: String?
    @State private var path: [String] = []
    @State private var search = ""
    @State private var adding = false
    @State private var importing = false
    @State private var problem: String?

    /// `open` starts with that item in the reader, for the snapshots.
    public init(open: String? = nil) {
        _selection = State(initialValue: open)
        _path = State(initialValue: open.map { [$0] } ?? [])
    }

    public var body: some View {
        layout
            .sheet(isPresented: $adding) {
                AddLinkView { id in show(id) }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.epub, .pdf], allowsMultipleSelection: true) { result in
                guard case .success(let files) = result else { return }
                Task {
                    for file in files {
                        do { show(try await shelf.add(file: file).id) } catch { problem = Shelf.describe(error) }
                    }
                }
            }
            .alert("Couldn’t add that", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } }), presenting: problem) { _ in
                Button("OK") { problem = nil }
            } message: { Text($0) }
            .tint(Palette.link)
    }

    @ViewBuilder private var layout: some View {
        #if os(macOS)
        NavigationSplitView {
            List(LibrarySection.allCases, selection: $section) { section in
                Label(section.rawValue, systemImage: section.symbol).tag(section)
            }
            .navigationSplitViewColumnWidth(min: 160, ideal: 190)
        } content: {
            List(rows, id: \.id, selection: $selection) { item in
                Row(item: item).tag(item.id)
            }
            .overlay { empty }
            .searchable(text: $search, prompt: "Search the library")
            .navigationTitle(section?.rawValue ?? "Library")
            .navigationSplitViewColumnWidth(min: 260, ideal: 320)
            .toolbar { adds }
        } detail: {
            if let selection {
                ReaderView(id: selection).id(selection)
            } else {
                ContentUnavailableView("Nothing open", systemImage: "book", description: Text("Choose something from the library."))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Palette.ground)
            }
        }
        #else
        NavigationStack(path: $path) {
            List(rows, id: \.id) { item in
                NavigationLink(value: item.id) { Row(item: item) }
            }
            .overlay { empty }
            .searchable(text: $search, prompt: "Search the library")
            .navigationTitle("Library")
            .toolbar { adds }
            .navigationDestination(for: String.self) { id in
                ReaderView(id: id)
            }
        }
        #endif
    }

    @ToolbarContentBuilder private var adds: some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Menu {
                Button("Add Link…", systemImage: "link") { adding = true }
                    .keyboardShortcut("l", modifiers: [.command, .shift])
                Button("Add EPUB or PDF…", systemImage: "doc.badge.plus") { importing = true }
                    .keyboardShortcut("o", modifiers: .command)
            } label: {
                Label("Add", systemImage: "plus")
            }
        }
    }

    @ViewBuilder private var empty: some View {
        if let why = shelf.problem {
            ContentUnavailableView("The library couldn’t be opened", systemImage: "exclamationmark.triangle", description: Text(why))
        } else if rows.isEmpty {
            if search.isEmpty {
                ContentUnavailableView("Nothing here yet", systemImage: "books.vertical",
                                       description: Text("Add a link to an article, or an EPUB or a PDF."))
            } else {
                ContentUnavailableView.search(text: search)
            }
        }
    }

    private var rows: [Item] {
        let words = search.trimmingCharacters(in: .whitespaces)
        return shelf.items.filter { item in
            (section ?? .all).holds(item) && (words.isEmpty
                || item.title.localizedStandardContains(words)
                || (item.author?.localizedStandardContains(words) ?? false))
        }
    }

    private func show(_ id: String) {
        #if os(macOS)
        if let section, let item = shelf.item(id), !section.holds(item) { self.section = .all }
        selection = id
        #else
        path = [id]
        #endif
    }
}

/// One row of the library: the title, who wrote it, what it is and how far through it is.
struct Row: View {
    let item: Item

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Cover(item: item)
            VStack(alignment: .leading, spacing: 4) {
                Text(item.title)
                    .font(Typeface.ui(14, weight: .medium))
                    .foregroundStyle(Palette.ink)
                    .lineLimit(2)
                Text(detail)
                    .font(Typeface.ui(12))
                    .foregroundStyle(Palette.muted)
                    .lineLimit(1)
                if item.progress > 0 {
                    Progress(value: Double(item.progress))
                        .padding(.top, 2)
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    private var detail: String {
        let kind = switch item.kind {
        case .article: "Article"
        case .epub: "EPUB"
        case .pdf: "PDF"
        }
        let state = item.progress >= 0.99 ? "Finished" : item.progress > 0 ? "\(Int((item.progress * 100).rounded()))%" : "New"
        return [item.author, kind, state].compactMap { $0 }.joined(separator: " · ")
    }
}

/// How far through, as a thin bar under the row.
private struct Progress: View {
    let value: Double

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Palette.segment)
                Capsule().fill(Palette.link).frame(width: geometry.size.width * min(1, max(0, value)))
            }
        }
        .frame(height: 3)
        .accessibilityHidden(true)
    }
}

/// A book's cover, or the mark of what it is.
private struct Cover: View {
    let item: Item

    var body: some View {
        Group {
            if let path = item.cover, let image = Cover.load(path) {
                image.resizable().scaledToFill()
            } else {
                Image(systemName: item.kind == .article ? "doc.richtext" : "book.closed")
                    .font(.system(size: 16))
                    .foregroundStyle(Palette.muted)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Palette.segment)
            }
        }
        .frame(width: 36, height: 52)
        .clipShape(RoundedRectangle(cornerRadius: 4))
        .accessibilityHidden(true)
    }

    private static func load(_ path: String) -> Image? {
        #if canImport(AppKit)
        NSImage(contentsOfFile: path).map(Image.init(nsImage:))
        #else
        UIImage(contentsOfFile: path).map(Image.init(uiImage:))
        #endif
    }
}

/// Asks for a web address and saves the page as an article.
struct AddLinkView: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.dismiss) private var dismiss
    let saved: (String) -> Void

    @State private var link = ""
    @State private var saving = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Add a link")
                .font(Typeface.ui(17, weight: .semibold))
            TextField("https://", text: $link)
                .textFieldStyle(.roundedBorder)
                .font(Typeface.ui(14))
                .onSubmit(save)
                .disabled(saving)
                #if os(iOS)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                #endif
            if let problem {
                Text(problem)
                    .font(Typeface.ui(13))
                    .foregroundStyle(Palette.muted)
            }
            HStack {
                if saving { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(saving || link.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(minWidth: 380)
        .presentationDetents([.medium])
    }

    private func save() {
        guard !saving else { return }
        saving = true
        problem = nil
        Task {
            do {
                let item = try await shelf.save(link: link)
                dismiss()
                saved(item.id)
            } catch {
                problem = Shelf.describe(error)
            }
            saving = false
        }
    }
}

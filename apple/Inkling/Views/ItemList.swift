import InklingCore
import SwiftUI

/// A section's items in one list, in titled groups when there are several. Archive and Move to
/// Queue are on a swipe on iPhone and on the context menu and Item menu on both.
struct ItemList<Empty: View>: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    let groups: [(title: String, items: [Item])]
    let section: Section
    @ViewBuilder let empty: () -> Empty

    var body: some View {
        @Bindable var navigator = navigator
        Group {
            if groups.allSatisfy(\.items.isEmpty) {
                empty()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color(palette.ground))
            } else {
                #if os(macOS)
                List(selection: $navigator.selection) { rows }
                #else
                List { rows }
                #endif
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(Color(palette.ground))
    }

    @ViewBuilder private var rows: some View {
        ForEach(groups.filter { !$0.items.isEmpty }, id: \.title) { group in
            if group.title.isEmpty {
                ForEach(group.items, id: \.id) { row($0) }
            } else {
                SwiftUI.Section {
                    ForEach(group.items, id: \.id) { row($0) }
                } header: {
                    Text(group.title)
                        .font(.inter(13, weight: .semibold, relativeTo: .subheadline))
                        .foregroundStyle(Color(palette.muted))
                }
            }
        }
    }

    private func row(_ item: Item) -> some View {
        #if os(macOS)
        ItemRow(item: item)
            .tag(item.id)
            .contextMenu { actions(item) }
            .listRowSeparatorTint(Color(palette.hairline))
        #else
        NavigationLink(value: item.id) {
            ItemRow(item: item)
        }
        .navigationLinkIndicatorVisibility(.hidden)
        .listRowBackground(Color(palette.ground))
        .listRowSeparatorTint(Color(palette.hairline))
        .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
        .swipeActions(edge: .trailing, allowsFullSwipe: true) {
            if item.archived == 0 {
                Button("Archive", systemImage: "archivebox") { shelf.setArchived(item, true) }
                    .tint(Color(palette.ink))
            } else {
                Button("Move to Queue", systemImage: "arrow.uturn.backward") { shelf.setArchived(item, false) }
                    .tint(Color(palette.ink))
            }
        }
        .contextMenu { actions(item) }
        .accessibilityIdentifier("item-\(item.title)")
        #endif
    }

    @ViewBuilder private func actions(_ item: Item) -> some View {
        if item.archived == 0 {
            Button("Archive", systemImage: "archivebox") { shelf.setArchived(item, true) }
        } else {
            Button("Move to Queue", systemImage: "arrow.uturn.backward") { shelf.setArchived(item, false) }
        }
    }
}

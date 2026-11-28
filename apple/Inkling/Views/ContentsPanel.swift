import SwiftUI

/// The chapters of the open book, the one being read marked, and the steps to the chapters either
/// side of it. It rises from the foot of the iPhone and hangs from its button on the Mac, as
/// Appearance does.
struct ContentsPanel: View {
    @Environment(\.dismiss) private var dismiss
    let model: ReaderModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 4) {
                Text("Contents")
                    .font(.inter(15, weight: .semibold, relativeTo: .headline))
                Spacer()
                Button("Previous Chapter", systemImage: "chevron.backward") { choose(model.chapter - 1) }
                    .disabled(model.chapter == 0)
                Button("Next Chapter", systemImage: "chevron.forward") { choose(model.chapter + 1) }
                    .disabled(model.chapter + 1 >= model.chapters)
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 8)

            ScrollViewReader { list in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(model.rendered.titles.indices, id: \.self) { index in
                            row(index)
                        }
                    }
                    .padding(.bottom, 12)
                }
                .onAppear { list.scrollTo(model.chapter, anchor: .center) }
            }
        }
        .frame(minWidth: 320, idealWidth: 360)
    }

    private func row(_ index: Int) -> some View {
        let reading = index == model.chapter
        return Button { choose(index) } label: {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                Text(model.rendered.titles[index])
                    .font(.inter(15, weight: reading ? .semibold : .regular, relativeTo: .body))
                    .foregroundStyle(reading ? .primary : .secondary)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
                if reading {
                    Image(systemName: "checkmark")
                        .font(.inter(13, weight: .semibold, relativeTo: .footnote))
                        .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 11)
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(reading ? .isSelected : [])
        .id(index)
    }

    /// Closes the panel and dips to the chapter, as a chapter chosen from Contents always has.
    private func choose(_ index: Int) {
        dismiss()
        model.go(toChapter: index, .fade)
    }
}

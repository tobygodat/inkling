import SwiftUI

/// Saving an article from a link: the address, pasted in or typed, and Save.
struct SaveLinkSheet: View {
    @Environment(Shelf.self) private var shelf
    @Environment(\.dismiss) private var dismiss
    @Environment(\.palette) private var palette
    @State private var link = ""
    @State private var saving = false
    @State private var problem: String?
    @FocusState private var focused: Bool

    var body: some View {
        #if os(macOS)
        VStack(alignment: .leading, spacing: 14) {
            Text("Save Link")
                .font(.serif(20, weight: .semibold, relativeTo: .title3))
                .foregroundStyle(Color(palette.ink))
            field
            HStack {
                hint
                Spacer(minLength: 16)
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(action: save) {
                    if saving { ProgressView().controlSize(.small) } else { Text("Save") }
                }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.glassProminent)
                .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty || saving)
                .accessibilityIdentifier("save")
            }
        }
        .padding(20)
        .frame(width: 460)
        .background(Color(palette.ground))
        .onAppear(perform: appear)
        #else
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                field
                hint
                Spacer(minLength: 0)
            }
            .padding(20)
            .background(Color(palette.ground))
            .navigationTitle("Save Link")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", role: .cancel) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button("Save", action: save)
                            .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty)
                            .accessibilityIdentifier("save")
                    }
                }
            }
        }
        .presentationDetents([.height(210)])
        .onAppear(perform: appear)
        #endif
    }

    private var field: some View {
        TextField("https://", text: $link)
            .textFieldStyle(.plain)
            .font(.inter(16, relativeTo: .body))
            .textContentType(.URL)
            .autocorrectionDisabled()
            #if os(iOS)
            .keyboardType(.URL)
            .textInputAutocapitalization(.never)
            .submitLabel(.go)
            #endif
            .focused($focused)
            .onSubmit(save)
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(Color(palette.surface), in: .rect(cornerRadius: 12))
            .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.hairline)) }
            .accessibilityIdentifier("link")
    }

    private var hint: some View {
        Text(problem ?? "Saved to read offline, with its pictures.")
            .font(.inter(13, relativeTo: .footnote))
            .foregroundStyle(problem == nil ? Color(palette.muted) : Color(palette.ink))
            .animation(.easeOut(duration: 0.2), value: problem)
    }

    private func appear() {
        if link.isEmpty, let pasted = Pasteboard.link { link = pasted }
        focused = true
    }

    private func save() {
        guard !saving else { return }
        saving = true
        problem = nil
        Task {
            do {
                _ = try await shelf.save(link: link)
                dismiss()
            } catch {
                problem = Shelf.words(error)
            }
            saving = false
        }
    }
}

/// A web address on the clipboard, offered as the link to save.
enum Pasteboard {
    static var link: String? {
        #if os(macOS)
        let text = NSPasteboard.general.string(forType: .string)
        #else
        guard UIPasteboard.general.hasURLs else { return nil }
        let text = UIPasteboard.general.url?.absoluteString
        #endif
        guard let text, let url = URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)), url.scheme?.hasPrefix("http") == true else { return nil }
        return url.absoluteString
    }
}

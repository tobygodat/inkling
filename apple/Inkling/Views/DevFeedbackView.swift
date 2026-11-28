#if os(iOS)
import SwiftUI
import UIKit

/// The dev builds' feedback (TOB-571): a handle on the right edge, which opens the panel from that
/// side. Drag the handle up or down to move it out of the way.
struct DevFeedbackOverlay: View {
    @Environment(Shelf.self) private var shelf
    @Environment(Navigator.self) private var navigator
    @Environment(\.palette) private var palette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var feedback = DevFeedback()
    @AppStorage("devFeedbackHandle") private var handleAt = 0.42
    @GestureState private var dragged: CGFloat = 0
    @FocusState private var typing: Bool

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topTrailing) {
                WindowReader { feedback.watch($0) }
                    .frame(width: 0, height: 0)
                if feedback.open {
                    Color.black.opacity(0.18)
                        .ignoresSafeArea()
                        .onTapGesture(perform: close)
                        .transition(.opacity)
                    panel(width: min(360, proxy.size.width - 24))
                        .transition(reduceMotion ? .opacity : .move(edge: .trailing).combined(with: .opacity))
                } else {
                    handle(in: proxy.size)
                        .transition(.opacity)
                }
            }
        }
        .animation(.timingCurve(0.22, 1, 0.36, 1, duration: 0.2), value: feedback.open)
    }

    private func handle(in size: CGSize) -> some View {
        let lowest = 72.0, highest = max(lowest, size.height - 150)
        let y = min(max(size.height * handleAt + dragged, lowest), highest)
        return UnevenRoundedRectangle(topLeadingRadius: 10, bottomLeadingRadius: 10)
            .fill(Color(palette.surface))
            .overlay(UnevenRoundedRectangle(topLeadingRadius: 10, bottomLeadingRadius: 10).strokeBorder(Color(palette.hairline)))
            .overlay(Image(systemName: "chevron.compact.left").font(.system(size: 17, weight: .semibold)).foregroundStyle(Color(palette.ink)))
            .frame(width: 20, height: 56)
            .shadow(color: .black.opacity(0.12), radius: 6, x: -1, y: 1)
            .frame(width: 44, height: 64, alignment: .trailing)
            .contentShape(Rectangle())
            .onTapGesture(perform: show)
            .gesture(
                DragGesture(minimumDistance: 6)
                    .updating($dragged) { value, state, _ in state = value.translation.height }
                    .onEnded { value in handleAt = min(max(size.height * handleAt + value.translation.height, lowest), highest) / size.height }
            )
            .position(x: size.width - 22, y: y)
            .accessibilityElement()
            .accessibilityAddTraits(.isButton)
            .accessibilityLabel("Feedback")
            .accessibilityIdentifier("feedback-handle")
    }

    private func panel(width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 8) {
                Text("Feedback")
                    .font(.inter(17, weight: .semibold, relativeTo: .headline))
                    .onTapGesture { typing = false }
                Text("Dev")
                    .font(.inter(12, weight: .medium, relativeTo: .caption))
                    .foregroundStyle(Color(palette.muted))
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Color(palette.segment), in: Capsule())
                Spacer(minLength: 0)
                Button(action: close) {
                    Image(systemName: "xmark")
                        .font(.system(size: 15, weight: .semibold))
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .padding(.trailing, -12)
                .accessibilityLabel("Close")
                .accessibilityIdentifier("feedback-close")
            }
            if feedback.phase == .sent {
                sent
            } else {
                form
            }
        }
        .foregroundStyle(Color(palette.ink))
        .padding(.horizontal, 20)
        .padding(.top, 6)
        .padding(.bottom, 20)
        .frame(width: width, alignment: .leading)
        .background(Color(palette.surface), in: UnevenRoundedRectangle(topLeadingRadius: 22, bottomLeadingRadius: 22))
        .overlay(UnevenRoundedRectangle(topLeadingRadius: 22, bottomLeadingRadius: 22).strokeBorder(Color(palette.hairline)))
        .shadow(color: .black.opacity(0.16), radius: 18, x: -2, y: 4)
        .padding(.top, 8)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("feedback-panel")
    }

    @ViewBuilder private var form: some View {
        @Bindable var feedback = feedback
        Toggle(isOn: $feedback.attach) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Attach the last 10 seconds")
                    .font(.inter(15, weight: .medium, relativeTo: .body))
                Text("The screen and your touches, kept on this iPhone until you send")
                    .font(.inter(13, relativeTo: .footnote))
                    .foregroundStyle(Color(palette.muted))
            }
        }
        .tint(Color(palette.ink))
        .accessibilityIdentifier("feedback-attach")
        if feedback.attach {
            HStack(alignment: .center, spacing: 12) {
                if let preview = feedback.preview {
                    Image(uiImage: preview)
                        .resizable()
                        .scaledToFit()
                        .frame(height: 96)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color(palette.hairline)))
                }
                Text(clipWords)
                    .font(.inter(13, relativeTo: .footnote))
                    .foregroundStyle(Color(palette.muted))
                    .accessibilityIdentifier("feedback-clip")
            }
        }
        TextField("What should change?", text: $feedback.text, axis: .vertical)
            .lineLimit(4...8)
            .font(.inter(15, relativeTo: .body))
            .focused($typing)
            .padding(12)
            .background(Color(palette.ground), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color(palette.hairline)))
            .accessibilityIdentifier("feedback-text")
        Text("From \(DevFeedback.whereAt(shelf, navigator).place)")
            .font(.inter(12, relativeTo: .caption))
            .foregroundStyle(Color(palette.muted))
            .lineLimit(2)
        if case .failed(let words) = feedback.phase {
            Text(words)
                .font(.inter(13, relativeTo: .footnote))
                .accessibilityIdentifier("feedback-problem")
        } else if !shelf.account.signedIn {
            Text("Sign in on Profile to send feedback.")
                .font(.inter(13, relativeTo: .footnote))
        }
        Button {
            typing = false
            feedback.send(from: shelf, at: navigator)
        } label: {
            Group {
                if feedback.phase == .sending {
                    ProgressView().tint(Color(palette.surface))
                } else {
                    Text("Send").font(.inter(15, weight: .semibold, relativeTo: .body))
                }
            }
            .frame(maxWidth: .infinity, minHeight: 44)
            .foregroundStyle(Color(palette.surface))
            .background(Color(palette.ink), in: Capsule())
            .opacity(canSend ? 1 : 0.4)
        }
        .buttonStyle(.plain)
        .disabled(!canSend)
        .accessibilityIdentifier("feedback-send")
    }

    private var sent: some View {
        VStack(alignment: .leading, spacing: 10) {
            Image(systemName: "checkmark.circle")
                .font(.system(size: 30, weight: .regular))
            Text("Sent to the Captain")
                .font(.inter(17, weight: .semibold, relativeTo: .headline))
            Text("It has your words and where you were\(feedback.attach ? ", with the last 10 seconds" : "").")
                .font(.inter(14, relativeTo: .body))
                .foregroundStyle(Color(palette.muted))
            Button(action: close) {
                Text("Done")
                    .font(.inter(15, weight: .semibold, relativeTo: .body))
                    .frame(maxWidth: .infinity, minHeight: 44)
                    .background(Color(palette.segment), in: Capsule())
            }
            .buttonStyle(.plain)
            .padding(.top, 6)
            .accessibilityIdentifier("feedback-done")
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("feedback-sent")
    }

    private var clipWords: String {
        guard let clip = feedback.clip else {
            return feedback.preparing ? "Making the clip…" : "The clip couldn’t be made: \(feedback.clipProblem ?? "nothing was captured yet.")"
        }
        let touches = clip.touches == 1 ? "1 touch" : "\(clip.touches) touches"
        return "\(String(format: "%.0f", clip.seconds)) s · \(touches)"
    }

    private var canSend: Bool {
        shelf.account.signedIn && feedback.phase != .sending && !feedback.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !(feedback.attach && feedback.clip == nil)
    }

    private func show() {
        feedback.show()
    }

    private func close() {
        typing = false
        feedback.hide()
    }
}

/// Hands over the window the overlay is in, once it is in one.
private struct WindowReader: UIViewRepresentable {
    let found: (UIWindow) -> Void

    func makeUIView(context: Context) -> Probe {
        let probe = Probe()
        probe.found = found
        probe.isUserInteractionEnabled = false
        return probe
    }

    func updateUIView(_ uiView: Probe, context: Context) {}

    final class Probe: UIView {
        var found: ((UIWindow) -> Void)?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            if let window { found?(window) }
        }
    }
}
#endif

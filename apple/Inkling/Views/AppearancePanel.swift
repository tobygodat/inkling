import InklingCore
import SwiftUI

/// The app's own Light or Dark: the system's, or one of them whatever the system says.
enum Appearance: String, CaseIterable {
    case system, light, dark

    var scheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }

    /// `INKLING_APPEARANCE`, which wins for the snapshot tests: the Simulator's own setting doesn't
    /// reach apps on a Simulator booted headless.
    static var forced: ColorScheme? {
        ProcessInfo.processInfo.environment["INKLING_APPEARANCE"].flatMap(Appearance.init(rawValue:))?.scheme
    }
}

/// How reading or writing looks: the app's Light or Dark, the paper, the face, the size and the
/// line height. It opens from the reader or writing page, which redraws behind it, and is in the
/// Mac's Settings.
struct AppearancePanel: View {
    enum Target { case reading, writing }

    @Environment(Shelf.self) private var shelf
    @AppStorage("appearance") private var appearance = Appearance.system
    var target: Target = .reading
    var includeListDensity: Bool = false

    /// Auto (Light or Dark, as the app is) and the paper tones. Custom is the writer's own and
    /// waits for a custom colour picker.
    private static let tones: [Tone] = [.light, .paper, .sepia, .sage, .dusk, .night]

    private var currentLook: Look {
        target == .writing ? shelf.writingLook : shelf.look
    }

    private func updateLook(_ look: Look) {
        if target == .writing {
            shelf.setWritingLook(look)
        } else {
            shelf.setLook(look)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            Picker("Appearance", selection: $appearance) {
                ForEach(Appearance.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            HStack(spacing: 0) {
                ForEach(Self.tones, id: \.self) { tone in
                    swatch(tone).frame(maxWidth: .infinity)
                }
            }

            VStack(spacing: 14) {
                HStack {
                    Text("Face")
                    Spacer()
                    Picker("Face", selection: binding(\.face)) {
                        ForEach(faces(), id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .fixedSize()
                }
                Stepper(value: binding(\.textSize), in: 15...32) {
                    LabeledContent("Size", value: "\(currentLook.textSize)")
                }
                .accessibilityIdentifier("size")
                Stepper(value: binding(\.lineSpacing), in: 120...220, step: 5) {
                    LabeledContent("Line height", value: (Double(currentLook.lineSpacing) / 100).formatted(.number.precision(.fractionLength(2))))
                }
                .accessibilityIdentifier("line-height")
                if includeListDensity {
                    HStack {
                        Text("List density")
                        Spacer()
                        Picker("List density", selection: binding(\.density)) {
                            Text("Compact").tag(ListDensity.compact)
                            Text("Comfortable").tag(ListDensity.comfortable)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .fixedSize()
                        .accessibilityIdentifier("list-density")
                    }
                }
            }
            .font(.inter(15, relativeTo: .body))
            .monospacedDigit()
        }
        .padding(20)
        .frame(minWidth: 320, idealWidth: 360)
        .sensoryFeedback(.selection, trigger: currentLook)
    }

    private func swatch(_ tone: Tone) -> some View {
        let selected = tone == .light ? [.light, .dark].contains(currentLook.tone) : currentLook.tone == tone
        let name = tone == .light ? "Auto" : String(describing: tone).capitalized
        let light = Palette(shelf.looks.colors(tone: tone))
        let dark = Palette(shelf.looks.colors(tone: tone == .light ? .dark : tone))
        return Button {
            var look = currentLook
            look.tone = tone
            updateLook(look)
        } label: {
            VStack(spacing: 6) {
                Group {
                    if tone == .light {
                        ZStack {
                            sample(light).mask(Self.half(first: true))
                            sample(dark).mask(Self.half(first: false))
                        }
                    } else {
                        sample(light)
                    }
                }
                    .overlay { Circle().strokeBorder(Color(light.hairline), lineWidth: 1) }
                    .padding(3)
                    .overlay { Circle().strokeBorder(selected ? Color.primary : .clear, lineWidth: 2) }
                Text(name)
                    .font(.inter(11, weight: .medium, relativeTo: .caption))
                    .foregroundStyle(selected ? .primary : .secondary)
            }
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name)
        .accessibilityIdentifier("tone-\(name.lowercased())")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    /// "Aa" on a tone's paper, as the swatch shows it.
    private func sample(_ palette: Palette) -> some View {
        Text("Aa")
            .font(.serif(15, relativeTo: .body))
            .foregroundStyle(Color(palette.ink))
            .frame(width: 44, height: 44)
            .background(Circle().fill(Color(palette.ground)))
    }

    /// The top-left half of a swatch, or the rest: Auto shows Light's paper and Dark's together.
    private static func half(first: Bool) -> LinearGradient {
        let (a, b): (Color, Color) = first ? (.black, .clear) : (.clear, .black)
        return LinearGradient(stops: [.init(color: a, location: 0.5), .init(color: b, location: 0.5)], startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    private func binding<Value>(_ key: WritableKeyPath<Look, Value>) -> Binding<Value> {
        Binding {
            currentLook[keyPath: key]
        } set: {
            var look = currentLook
            look[keyPath: key] = $0
            updateLook(look)
        }
    }
}

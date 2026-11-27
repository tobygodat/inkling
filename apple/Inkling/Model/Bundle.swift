import Foundation

private final class Token {}

extension Bundle {
    /// The bundle the app's resources ship in: the app itself, or the snapshot tests' bundle.
    nonisolated static let inkling = Bundle(for: Token.self)
}

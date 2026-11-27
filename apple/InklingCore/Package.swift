// swift-tools-version: 6.0
// Inkling's Rust crates for the native app (ADR 0007): the XCFramework and the Swift side that
// apple/core.sh builds from inkling-ffi, and the few Swift helpers that go with them.
import PackageDescription

let package = Package(
    name: "InklingCore",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "InklingCore", targets: ["InklingCore"])],
    targets: [
        .binaryTarget(name: "InklingFFI", path: "InklingFFI.xcframework"),
        .target(name: "InklingCore", dependencies: ["InklingFFI"]),
        .testTarget(name: "InklingCoreTests", dependencies: ["InklingCore"]),
    ]
)

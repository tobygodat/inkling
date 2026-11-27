// swift-tools-version: 6.0
// The native app's screens (ADR 0007), shared by the Mac and iPhone targets in apple/project.yml.
// The apps themselves are thin: everything they show is here, so `swift test` can check the reader's
// blocks and render the screens offscreen without opening an app.
import PackageDescription

let package = Package(
    name: "InklingUI",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "InklingUI", targets: ["InklingUI"])],
    dependencies: [.package(path: "../InklingCore")],
    targets: [
        .target(
            name: "InklingUI",
            dependencies: [.product(name: "InklingCore", package: "InklingCore")],
            resources: [.copy("Fonts")]
        ),
        .testTarget(name: "InklingUITests", dependencies: ["InklingUI"]),
    ]
)

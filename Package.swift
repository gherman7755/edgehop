// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EdgeHop",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EdgeHopCore", targets: ["EdgeHopCore"]),
    ],
    targets: [
        .target(name: "EdgeHopCore"),
        .testTarget(name: "EdgeHopCoreTests", dependencies: ["EdgeHopCore"]),
    ]
)

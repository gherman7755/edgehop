// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EdgeHop",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EdgeHopCore", targets: ["EdgeHopCore"]),
        .executable(name: "edgehop-tap", targets: ["edgehop-tap"]),
        .executable(name: "edgehop-inject", targets: ["edgehop-inject"]),
    ],
    targets: [
        .target(name: "EdgeHopCore"),
        .executableTarget(name: "edgehop-tap", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-inject", dependencies: ["EdgeHopCore"]),
        .testTarget(name: "EdgeHopCoreTests", dependencies: ["EdgeHopCore"]),
    ]
)

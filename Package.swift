// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EdgeHop",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EdgeHopCore", targets: ["EdgeHopCore"]),
        .executable(name: "edgehop-tap", targets: ["edgehop-tap"]),
        .executable(name: "edgehop-inject", targets: ["edgehop-inject"]),
        .executable(name: "edgehop-link", targets: ["edgehop-link"]),
    ],
    targets: [
        .target(name: "EdgeHopCore"),
        .executableTarget(name: "edgehop-tap", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-inject", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-link", dependencies: ["EdgeHopCore"]),
        .testTarget(name: "EdgeHopCoreTests", dependencies: ["EdgeHopCore"]),
    ]
)

// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EdgeHop",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "EdgeHopCore", targets: ["EdgeHopCore"]),
        .library(name: "EdgeHopIPC", targets: ["EdgeHopIPC"]),
        .executable(name: "edgehop-tap", targets: ["edgehop-tap"]),
        .executable(name: "edgehop-inject", targets: ["edgehop-inject"]),
        .executable(name: "edgehop-link", targets: ["edgehop-link"]),
        .executable(name: "EdgeHopApp", targets: ["EdgeHopApp"]),
        .executable(name: "edgehop-core", targets: ["edgehop-core"]),
    ],
    targets: [
        .target(name: "EdgeHopCore"),
        .target(name: "EdgeHopIPC", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-tap", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-inject", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-link", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "EdgeHopApp", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-core", dependencies: ["EdgeHopCore", "EdgeHopIPC"]),
        .executable(name: "edgehop-tap", targets: ["edgehop-tap"]),
        .executable(name: "edgehop-inject", targets: ["edgehop-inject"]),
    ],
    targets: [
        .target(name: "EdgeHopCore"),
        .executableTarget(name: "edgehop-tap", dependencies: ["EdgeHopCore"]),
        .executableTarget(name: "edgehop-inject", dependencies: ["EdgeHopCore"]),
        .testTarget(name: "EdgeHopCoreTests", dependencies: ["EdgeHopCore"]),
        .testTarget(name: "EdgeHopIPCTests", dependencies: ["EdgeHopIPC"]),
    ]
)

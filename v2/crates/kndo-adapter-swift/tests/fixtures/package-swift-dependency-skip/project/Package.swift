// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "package-swift-dependency-skip",
    dependencies: [
        .package(url: "https://github.com/apple/swift-algorithms.git", from: "1.0.0")
    ],
    targets: [
        .executableTarget(name: "App")
    ]
)

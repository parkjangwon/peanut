// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PeanutSDK",
    platforms: [
        .iOS(.v15),
        .macOS(.v12)
    ],
    products: [
        .library(name: "PeanutSDK", targets: ["PeanutSDK"])
    ],
    targets: [
        .target(name: "PeanutSDK"),
        .testTarget(name: "PeanutSDKTests", dependencies: ["PeanutSDK"])
    ]
)

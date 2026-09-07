// swift-tools-version:5.5
import PackageDescription

// The library and the two app targets that ship the storyboards. The apps are
// executable targets so their classes are JUDGED: what a `.storyboard` or an
// `Info.plist` names is the only thing keeping several of them alive, which is
// the whole point of the fixture. A source tree no target declares would be
// keep-alive instead, and would prove nothing.
let package = Package(
    name: "Alamofire",
    products: [.library(name: "Alamofire", targets: ["Alamofire"])],
    targets: [
        .target(name: "Alamofire", path: "Source"),
        .executableTarget(name: "Example", path: "Example/Source"),
        .executableTarget(
            name: "WatchKitExtension",
            path: "watchOS Example/watchOS Example WatchKit Extension"
        ),
    ]
)

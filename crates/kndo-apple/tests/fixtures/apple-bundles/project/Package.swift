// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "Alamofire",
    products: [.library(name: "Alamofire", targets: ["Alamofire"])],
    targets: [.target(name: "Alamofire", path: "Source")]
)

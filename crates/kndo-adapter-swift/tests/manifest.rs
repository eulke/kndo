use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::adapter::SourceFile;
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;

#[test]
fn package_swift_dependencies_read_by_label() {
    let a = SwiftAdapter::new();
    let manifest = br#"// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "demo",
    dependencies: [
        .package(url: "https://github.com/vapor/vapor.git", from: "4.0.0"),
        .package(url: "https://github.com/apple/swift-nio", from: "2.0.0"),
        .package(path: "../local-kit"),
    ],
    targets: [.target(name: "App")]
)
"#;
    let names = a
        .manifest_dependencies(&SourceFile {
            path: &ProjectPath::new("Package.swift"),
            content: manifest,
            region: None,
        })
        .into_iter()
        .map(|d| d.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["local-kit", "swift-nio", "vapor"]);
}

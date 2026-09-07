//! `Package.swift` read once, by label: SwiftPM requires every argument named,
//! so `.package(url:)` and `.package(path:)` are exact where a scan would
//! guess. The NAME is the identity the ecosystem imports the package by — the
//! last path segment of the url with any `.git` dropped, or of the local path.

use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::manifest::ManifestEvidence;

fn read(manifest_path: &str, content: &str) -> ManifestEvidence {
    kndo_testkit::manifest_evidence(&SwiftAdapter::new(), manifest_path, content, &[])
}

#[test]
fn package_swift_dependencies_read_by_label() {
    let evidence = read(
        "Package.swift",
        r#"// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "demo",
    dependencies: [
        .package(url: "https://github.com/vapor/vapor.git", from: "4.0.0"),
        .package(url: "https://github.com/apple/swift-nio", from: "2.0.0"),
        .package(path: "../local-kit"),
    ],
    targets: [
        .target(name: "App", dependencies: [
            // A target's list NAMES a package; it does not declare one, and
            // the package-level list is the only place that does.
            .product(name: "Vapor", package: "vapor"),
        ])
    ]
)
"#,
    );
    let names: Vec<String> = evidence
        .dependencies
        .into_iter()
        .map(|d| d.name.to_string())
        .collect();
    assert_eq!(names, vec!["local-kit", "swift-nio", "vapor"]);
}

#[test]
fn a_manifest_with_no_dependency_list_declares_none() {
    let evidence = read(
        "Package.swift",
        "let package = Package(name: \"demo\", targets: [.target(name: \"App\")])\n",
    );
    assert!(evidence.dependencies.is_empty());
    assert_eq!(evidence.units.len(), 1);
}

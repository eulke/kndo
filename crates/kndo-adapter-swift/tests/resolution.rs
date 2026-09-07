use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::extension::Extension;
use kndo_contract::manifest::UnitKind;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn path(p: &str) -> ProjectPath {
    ProjectPath::new(p)
}

fn known(paths: &[&str]) -> BTreeSet<ProjectPath> {
    paths.iter().map(|p| ProjectPath::new(*p)).collect()
}

#[test]
fn an_import_names_a_local_target_or_nothing() {
    let files = known(&[
        "Sources/Core/A.swift",
        "Sources/Core/Sub/B.swift",
        "Sources/App/main.swift",
    ]);
    let cx = ResolveContext::new(&files);
    let a = SwiftAdapter::new();
    assert_eq!(
        a.resolve(&path("Sources/App/main.swift"), "Core", &cx),
        Resolution::Files(vec![
            path("Sources/Core/A.swift"),
            path("Sources/Core/Sub/B.swift"),
        ]),
        "the target is flat: subdirectories are organizational"
    );
    assert_eq!(
        a.resolve(&path("Sources/App/main.swift"), "Foundation", &cx),
        Resolution::Unresolved,
        "SDK and external modules stay keep-alive"
    );
}

#[test]
fn a_test_target_is_a_friend_of_what_it_tests() {
    // `@testable import App` reaches App's `internal`, and SwiftPM says which
    // targets a test target may do that to — its dependencies. Friendship is
    // the manifest's statement, which is why no `Tests/` predicate can stand
    // in for it: a test target that depends on nothing is a friend of nothing.
    let units = units(
        "Package.swift",
        r#"
        let package = Package(
            name: "App",
            products: [.library(name: "App", targets: ["App"])],
            targets: [
                .target(name: "App"),
                .target(name: "Helper"),
                .testTarget(name: "AppTests", dependencies: ["App"]),
            ]
        )
        "#,
    );
    let app = unit_named(&units, "App");
    assert_eq!(app.kind, UnitKind::Library);
    assert_eq!(app.roots, vec!["Sources/App"], "SwiftPM's predefined place");
    assert!(app.is_published(), "a library product names it");
    let helper = unit_named(&units, "Helper");
    assert!(
        !helper.is_published(),
        "no product names it, so its exports are the package's own"
    );
    let tests = unit_named(&units, "AppTests");
    assert_eq!(tests.kind, UnitKind::Test);
    assert_eq!(tests.roots, vec!["Tests/AppTests"]);
    assert_eq!(
        tests.friend_of,
        vec!["App"],
        "and NOT Helper, which it never named"
    );
}

#[test]
fn a_path_override_moves_the_target_and_exclude_narrows_it() {
    // Alamofire's shape: `path:` replaces the predefined directory outright,
    // and `exclude:` is relative to it.
    let units = units(
        "Package.swift",
        r#"
        let package = Package(
            name: "Alamofire",
            targets: [
                .target(name: "Alamofire", path: "Source", exclude: ["Info.plist"]),
                .testTarget(name: "AlamofireTests", dependencies: ["Alamofire"], path: "Tests"),
            ]
        )
        "#,
    );
    let lib = unit_named(&units, "Alamofire");
    assert_eq!(lib.roots, vec!["Source"]);
    assert_eq!(lib.excludes, vec!["Source/Info.plist"]);
    assert_eq!(unit_named(&units, "AlamofireTests").roots, vec!["Tests"]);
}

#[test]
fn sources_narrows_the_target_to_what_it_lists() {
    let units = units(
        "pkg/Package.swift",
        r#"
        let package = Package(
            name: "P",
            targets: [.target(name: "Core", sources: ["a", "b/c.swift"])]
        )
        "#,
    );
    // Joined under the manifest's own directory, like every other root.
    assert_eq!(
        unit_named(&units, "Core").roots,
        vec!["pkg/Sources/Core/a", "pkg/Sources/Core/b/c.swift"]
    );
}

#[test]
fn a_dependency_on_another_package_names_no_unit_here() {
    let units = units(
        "Package.swift",
        r#"
        let package = Package(
            name: "P",
            targets: [.target(name: "Core", dependencies: [
                .product(name: "NIO", package: "swift-nio"),
                .target(name: "Sibling"),
                "Bare",
            ])]
        )
        "#,
    );
    assert_eq!(
        unit_named(&units, "Core").depends_on,
        vec!["Bare", "Sibling"],
        "a `.product` is another package's, so it names no unit of this project"
    );
}

fn units(path: &str, source: &str) -> Vec<kndo_contract::manifest::Unit> {
    let path = ProjectPath::new(path);
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let cx = ResolveContext::new(&known);
    let mut sink = kndo_contract::manifest::ManifestSink::default();
    SwiftAdapter::new().extract_manifest(
        &kndo_contract::adapter::SourceFile {
            path: &path,
            content: source.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish().units
}

fn unit_named<'a>(
    units: &'a [kndo_contract::manifest::Unit],
    name: &str,
) -> &'a kndo_contract::manifest::Unit {
    units.iter().find(|u| u.name == name).unwrap_or_else(|| {
        panic!(
            "no unit {name} among {:?}",
            units.iter().map(|u| &u.name).collect::<Vec<_>>()
        )
    })
}

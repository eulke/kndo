use kndo_adapter_swift::SwiftAdapter;
use kndo_contract::adapter::{Resolution, ResolveContext};
use kndo_contract::evidence::Reach;
use kndo_contract::extension::Extension;
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
fn the_module_region_adds_every_test_tree() {
    let files = known(&[
        "Sources/App/A.swift",
        "Sources/Other/C.swift",
        "Tests/AppTests/ATests.swift",
    ]);
    let cx = ResolveContext::new(&files);
    let a = SwiftAdapter::new();
    let region = a
        .seen_from(&path("Sources/App/A.swift"), &Reach::Unit { up: 0 }, &cx)
        .expect("the unit is bounded from the target layout");
    assert_eq!(
        region,
        vec![
            path("Sources/App/A.swift"),
            path("Tests/AppTests/ATests.swift")
        ],
        "any test target may @testable-import the module; other modules may not"
    );
    assert!(
        a.seen_from(
            &path("Sources/App/A.swift"),
            &Reach::Namespace { up: 0 },
            &cx
        )
        .is_none()
    );
}

#[test]
fn a_path_override_layout_takes_its_first_segment_as_the_target() {
    let files = known(&[
        "Source/AF.swift",
        "Source/Core/Request.swift",
        "Tests/RequestTests.swift",
    ]);
    let cx = ResolveContext::new(&files);
    let a = SwiftAdapter::new();
    // The fact is the target, and `seen_from` is what reads it: `internal`
    // in `Source/AF.swift` is bounded by the module that layout names.
    assert_eq!(
        a.seen_from(&path("Source/AF.swift"), &Reach::Unit { up: 0 }, &cx),
        Some(vec![
            path("Source/AF.swift"),
            path("Source/Core/Request.swift"),
            path("Tests/RequestTests.swift"),
        ]),
        "Alamofire's Source/** compiles as one module"
    );
}

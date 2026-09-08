//! The pom reader graded against Maven itself. `tests/captured/maven.json` is
//! what `mvn help:effective-pom` answered for the two-pom reactor beside it —
//! `reactor-parent-pom.xml` and `reactor-child-pom.xml`, byte-identical to the
//! files Maven was handed — and the checks below ask the reader the same
//! questions. Recapture with `mvn -f child/pom.xml help:effective-pom` when
//! the pinned version moves.
//!
//! Two of Maven's answers are the reason this reader is a document parser
//! rather than a line scanner, and both are defects the scanner had: an
//! `<exclusion>` is not a dependency, and a `<dependencyManagement>` entry
//! nobody declares is not one either.

use kndo_contract::adapter::{DependencyScope, ResolveContext, SourceFile};
use kndo_contract::manifest::{ManifestEvidence, ManifestSink};
use kndo_contract::vocab::ProjectPath;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PARENT: &str = include_str!("captured/reactor-parent-pom.xml");
const CHILD: &str = include_str!("captured/reactor-child-pom.xml");

fn captured() -> Value {
    serde_json::from_str(include_str!("captured/maven.json")).expect("the capture is valid json")
}

/// The child read with its parent beside it, exactly as the engine hands a
/// reader every manifest's content.
fn read_child() -> ManifestEvidence {
    let path = ProjectPath::new("child/pom.xml");
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let manifests: BTreeMap<ProjectPath, &[u8]> =
        [(ProjectPath::new("pom.xml"), PARENT.as_bytes())].into();
    let cx = ResolveContext::with_manifests(&known, &manifests);
    let mut sink = ManifestSink::new();
    kndo_toolkit::jvm_manifest::structure(
        &SourceFile {
            path: &path,
            content: CHILD.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

#[test]
fn the_declared_dependencies_are_mavens_own() {
    let doc = captured();
    let mut want: Vec<String> = doc["child"]["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .flat_map(|d| {
            let group = d["groupId"].as_str().expect("groupId");
            let artifact = d["artifactId"].as_str().expect("artifactId");
            [format!("{group}:{artifact}"), artifact.to_string()]
        })
        .collect();
    want.sort();
    let mut got: Vec<String> = read_child()
        .dependencies
        .iter()
        .map(|d| d.name.to_string())
        .collect();
    got.sort();
    assert_eq!(
        got,
        want,
        "graded against {}",
        doc["producer"].as_str().unwrap_or_default()
    );
}

#[test]
fn an_exclusion_is_not_a_dependency() {
    // Maven records `hamcrest-core` under junit's `exclusions` and never as a
    // dependency of the child; the line scanner this replaced saw the
    // `<groupId>`/`<artifactId>` pair inside `<exclusion>` and declared it.
    let doc = captured();
    let excluded: Vec<&str> = doc["child"]["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .flat_map(|d| d["exclusions"].as_array().expect("exclusions"))
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(excluded, ["hamcrest-core"], "the capture pins the case");
    let names: Vec<String> = read_child()
        .dependencies
        .iter()
        .map(|d| d.name.to_string())
        .collect();
    for name in excluded {
        assert!(
            !names
                .iter()
                .any(|n| n == name || n.ends_with(&format!(":{name}"))),
            "{name} is excluded, not declared: {names:?}"
        );
    }
}

#[test]
fn a_managed_version_alone_declares_nothing() {
    // The parent MANAGES `com.managed:managed-only`; only the child DECLARES
    // it, and a reader that scans lines inside any `<dependencies>` element
    // reads the management block as declarations of the parent.
    let path = ProjectPath::new("pom.xml");
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let manifests: BTreeMap<ProjectPath, &[u8]> = BTreeMap::new();
    let cx = ResolveContext::with_manifests(&known, &manifests);
    let mut sink = ManifestSink::new();
    kndo_toolkit::jvm_manifest::structure(
        &SourceFile {
            path: &path,
            content: PARENT.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    let parent = sink.finish();
    assert!(
        parent.dependencies.is_empty(),
        "the aggregator manages a version and declares no dependency: {:?}",
        parent.dependencies
    );
    // …and the child, which does declare it, has it.
    assert!(
        read_child()
            .dependencies
            .iter()
            .any(|d| d.name == "managed-only")
    );
}

#[test]
fn an_inherited_test_source_directory_is_the_childs_own() {
    // Maven resolves the parent's relative `test-src` against the CHILD's
    // directory — `child/test-src`, which the capture records as the
    // effective pom's `testSourceDirectory`.
    let doc = captured();
    assert_eq!(
        doc["child"]["testSourceDirectory"].as_str(),
        Some("test-src")
    );
    let evidence = read_child();
    let tests = evidence
        .units
        .iter()
        .find(|u| u.name == "child:test")
        .expect("the test unit");
    assert!(
        tests.roots.iter().any(|r| r.path == "child/test-src"),
        "{:?}",
        tests.roots
    );
}

#[test]
fn the_scope_a_dependency_states_is_the_scope_it_gets() {
    let evidence = read_child();
    let scope = |name: &str| {
        evidence
            .dependencies
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} declared"))
            .scope
    };
    assert_eq!(
        scope("junit"),
        Some(DependencyScope::Dev),
        "<scope>test</scope>"
    );
    assert_eq!(
        scope("managed-only"),
        None,
        "Maven's effective scope is `compile`, which the pom itself never wrote"
    );
}

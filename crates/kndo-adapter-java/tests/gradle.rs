//! The Gradle reader over the build `tests/fixtures/gradle-catalog-and-includes`
//! holds — the same build `cargo xtask capture` runs Gradle 8.14.3 on, so the
//! transcript beside it and these assertions read one tree.
//!
//! What the transcript grades, this file does not repeat: the modules the
//! settings file includes (`aggregates`), the source directories each set ends
//! up with (`compiles`), the coordinates a catalog alias means and the scope a
//! configuration states (`declares`), and — because the reading is WHOLE — that
//! a commented-out include or dependency declares nothing. What remains here is
//! what Gradle's model says and the claim vocabulary has no word for: the two
//! units a module is, their friendship, and a `project(":core")` naming a unit
//! of this build rather than an artifact.

use kndo_adapter_java::JavaAdapter;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::manifest::{Grant, ManifestEvidence, ManifestSink, UnitKind};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::ProjectPath;
use std::collections::{BTreeMap, BTreeSet};

const BUILD: &str = "tests/fixtures/gradle-catalog-and-includes/project";

/// One manifest of the fixture's build, read with the whole build beside it —
/// as the engine hands a reader every manifest's content.
fn read(relative: &str) -> ManifestEvidence {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(BUILD);
    let files: Vec<(ProjectPath, Vec<u8>)> = [
        "settings.gradle.kts",
        "core/build.gradle.kts",
        "app/build.gradle.kts",
        "gradle/libs.versions.toml",
    ]
    .into_iter()
    .map(|p| {
        (
            ProjectPath::new(p),
            std::fs::read(root.join(p)).unwrap_or_else(|e| panic!("{p}: {e}")),
        )
    })
    .collect();
    let manifests: BTreeMap<ProjectPath, &[u8]> = files
        .iter()
        .map(|(p, c)| (p.clone(), c.as_slice()))
        .collect();
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let cx = ResolveContext::with_manifests(&known, &manifests);
    let path = ProjectPath::new(relative);
    let content = std::fs::read(root.join(relative)).expect("the manifest");
    let mut sink = ManifestSink::new();
    JavaAdapter::new().extract_manifest(
        &SourceFile {
            path: &path,
            content: &content,
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

#[test]
fn a_module_is_two_units_and_the_tests_are_the_mains_friend() {
    let evidence = read("core/build.gradle.kts");
    let unit = |name: &str| {
        evidence
            .units
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("{name}: {:?}", evidence.units))
    };
    assert_eq!(unit("core").kind, UnitKind::Library);
    assert_eq!(unit("core:test").kind, UnitKind::Test);
    assert_eq!(
        unit("core:test")
            .depends_on
            .iter()
            .filter(|d| d.grants >= Grant::Unit)
            .map(|d| d.unit.name.as_str())
            .collect::<Vec<_>>(),
        ["core"],
        "Gradle compiles the test set against the main one, which is what \
         makes Kotlin's `internal` visible from a module's own tests"
    );
}

#[test]
fn a_project_dependency_names_a_unit_of_this_build_and_never_an_artifact() {
    let evidence = read("app/build.gradle.kts");
    let main = evidence
        .units
        .iter()
        .find(|u| u.name == "app")
        .expect("the main unit");
    assert_eq!(
        main.depends_on
            .iter()
            .map(|d| d.unit.name.as_str())
            .collect::<Vec<_>>(),
        ["core"],
        "`project(\":core\")` names a unit of this build — Gradle reports it as \
         a `project` dependency, not as a coordinate, and the transcript's \
         `declares` claims are external ones alone"
    );
}

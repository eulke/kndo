//! The Gradle reader graded against Gradle itself. `tests/captured/gradle.json`
//! is what a `kndoReport` task printed from inside Gradle 8.14.3 for the
//! two-module build beside it — `gradle-settings.gradle.kts`,
//! `gradle-core-build.gradle.kts`, `gradle-app-build.gradle.kts` and
//! `gradle-gradle-libs.versions.toml`, byte-identical to the files Gradle was
//! handed — and the checks below ask the reader the same questions.
//!
//! Three of Gradle's answers are why this is a block scanner over a comment-
//! blanked copy rather than a line reader: an `include(` spans lines, a
//! commented-out one names nothing, and a dependency named through the version
//! catalog has no coordinate in the script at all.

use kndo_contract::adapter::{DependencyScope, ResolveContext, SourceFile};
use kndo_contract::manifest::{ManifestEvidence, ManifestSink, UnitKind};
use kndo_contract::vocab::ProjectPath;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const SETTINGS: &str = include_str!("captured/gradle-settings.gradle.kts");
const CORE: &str = include_str!("captured/gradle-core-build.gradle.kts");
const APP: &str = include_str!("captured/gradle-app-build.gradle.kts");
const CATALOG: &str = include_str!("captured/gradle-gradle-libs.versions.toml");

fn captured() -> Value {
    serde_json::from_str(include_str!("captured/gradle.json")).expect("the capture is valid json")
}

/// One manifest of the captured build, read with the whole build beside it.
fn read(path: &str) -> ManifestEvidence {
    let files: [(&str, &str); 4] = [
        ("settings.gradle.kts", SETTINGS),
        ("core/build.gradle.kts", CORE),
        ("app/build.gradle.kts", APP),
        ("gradle/libs.versions.toml", CATALOG),
    ];
    let content = files
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, c)| *c)
        .expect("a file of the captured build");
    let known: BTreeSet<ProjectPath> = BTreeSet::new();
    let manifests: BTreeMap<ProjectPath, &[u8]> = files
        .iter()
        .map(|(p, c)| (ProjectPath::new(*p), c.as_bytes()))
        .collect();
    let cx = ResolveContext::with_manifests(&known, &manifests);
    let path = ProjectPath::new(path);
    let mut sink = ManifestSink::new();
    kndo_toolkit::jvm_manifest::structure(
        &SourceFile {
            path: &path,
            content: content.as_bytes(),
            region: None,
        },
        &cx,
        &mut sink,
    );
    sink.finish()
}

#[test]
fn the_included_modules_are_gradles_own() {
    let doc = captured();
    // Gradle's project list minus the root, which no `include` names.
    let mut want: Vec<String> = doc["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .filter_map(|p| p["path"].as_str())
        .filter(|p| *p != ":")
        .map(|p| p.trim_start_matches(':').to_string())
        .collect();
    want.sort();
    assert_eq!(want, ["app", "core"], "the capture pins the case");

    let mut got: Vec<String> = read("settings.gradle.kts")
        .packages
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    got.sort();
    assert_eq!(
        got,
        want,
        "an `include(` across lines is a project and a commented-out one is not; \
         graded against {}",
        doc["producer"].as_str().unwrap_or_default()
    );
}

#[test]
fn a_module_is_two_units_and_the_tests_are_the_mains_friend() {
    let evidence = read("core/build.gradle.kts");
    let main = evidence
        .units
        .iter()
        .find(|u| u.name == "core")
        .expect("the main unit");
    assert_eq!(main.kind, UnitKind::Library);
    let tests = evidence
        .units
        .iter()
        .find(|u| u.name == "core:test")
        .expect("the test unit");
    assert_eq!(tests.kind, UnitKind::Test);
    assert_eq!(
        tests
            .depends_on
            .iter()
            .filter(|d| d.friend)
            .map(|d| d.unit.as_str())
            .collect::<Vec<_>>(),
        ["core"],
        "Gradle compiles the test set against the main one, which is what \
         makes Kotlin's `internal` visible from a module's own tests"
    );
    // Gradle's own answer for this module's test source set.
    let doc = captured();
    let want: Vec<&str> = doc["sourceSets"]
        .as_array()
        .expect("sourceSets")
        .iter()
        .find(|s| s["project"] == ":core" && s["sourceSet"] == "test")
        .and_then(|s| s["srcDirs"].as_array())
        .expect("core's test source set")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(want, ["src/test/java"]);
    assert!(
        tests.roots.iter().any(|r| r.path == "core/src/test/java"),
        "{:?}",
        tests.roots
    );
}

#[test]
fn a_declared_src_dir_is_the_units_root() {
    // `app` adds `src/generated/java` to its main set, and Gradle answers with
    // both directories.
    let doc = captured();
    let want: Vec<&str> = doc["sourceSets"]
        .as_array()
        .expect("sourceSets")
        .iter()
        .find(|s| s["project"] == ":app" && s["sourceSet"] == "main")
        .and_then(|s| s["srcDirs"].as_array())
        .expect("app's main source set")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(want, ["src/generated/java", "src/main/java"]);

    let evidence = read("app/build.gradle.kts");
    let main = evidence
        .units
        .iter()
        .find(|u| u.name == "app")
        .expect("the main unit");
    let mut roots: Vec<&str> = main.roots.iter().map(|r| r.path.as_str()).collect();
    roots.sort_unstable();
    assert_eq!(roots, ["app/src/generated/java", "app/src/main/java"]);
}

#[test]
fn a_project_dependency_names_a_unit_and_a_catalog_alias_names_a_coordinate() {
    let doc = captured();
    let of = |project: &str, kind: &str| -> Vec<String> {
        doc["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .filter(|d| d["project"] == project && d["kind"] == kind)
            .filter_map(|d| d["coordinate"].as_str())
            .map(|c| c.split(':').take(2).collect::<Vec<_>>().join(":"))
            .collect()
    };
    assert_eq!(
        of(":app", "project"),
        ["oracle:core"],
        "the capture pins it"
    );

    let evidence = read("app/build.gradle.kts");
    let main = evidence
        .units
        .iter()
        .find(|u| u.name == "app")
        .expect("the main unit");
    assert_eq!(
        main.depends_on
            .iter()
            .map(|d| d.unit.as_str())
            .collect::<Vec<_>>(),
        ["core"],
        "`project(\":core\")` names a unit of this build, never an artifact"
    );

    // Gradle resolves `libs.guava` and `libs.junit.core` to coordinates the
    // script never spells; the reader answers them from the same catalog.
    let mut want: Vec<String> = of(":app", "external");
    want.sort();
    assert_eq!(want, ["com.google.guava:guava", "junit:junit"]);
    let declared: Vec<String> = evidence
        .dependencies
        .iter()
        .map(|d| d.name.to_string())
        .collect();
    for coordinate in &want {
        assert!(
            declared.contains(coordinate),
            "{coordinate} is what the alias means: {declared:?}"
        );
    }
    // …and the scope its configuration states.
    let scope = |name: &str| {
        evidence
            .dependencies
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} declared"))
            .scope
    };
    assert_eq!(
        scope("guava"),
        Some(DependencyScope::Prod),
        "implementation"
    );
    assert_eq!(
        scope("junit"),
        Some(DependencyScope::Dev),
        "testImplementation"
    );
}

#[test]
fn a_commented_out_declaration_declares_nothing() {
    // `core` comments out `com.commented:out`; blanking comments before any
    // reading is what keeps it out, and the same pass keeps
    // `// include("never-built")` from naming a project.
    let declared: Vec<String> = read("core/build.gradle.kts")
        .dependencies
        .iter()
        .map(|d| d.name.to_string())
        .collect();
    assert!(
        !declared
            .iter()
            .any(|d| d.contains("commented") || d.contains("out")),
        "{declared:?}"
    );
    assert!(declared.iter().any(|d| d == "org.slf4j:slf4j-api"));
}

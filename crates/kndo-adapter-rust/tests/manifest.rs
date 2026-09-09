//! Cargo.toml → units, packages and dependencies: every target cargo builds
//! with the file it is entered through, entry-optional packages, the dependency
//! tables, and degradation on dangling or broken manifests.

use kndo_adapter_rust::RustAdapter;
use kndo_contract::manifest::{ManifestEvidence, Publication, UnitDep, UnitKind};

fn read(manifest_path: &str, manifest: &str, files: &[&str]) -> ManifestEvidence {
    kndo_testkit::manifest_evidence(&RustAdapter::new(), manifest_path, manifest, files)
}

/// Every unit as (name, kind, its one entry, its source root), name-sorted.
fn units(evidence: &ManifestEvidence) -> Vec<(String, UnitKind, String, String)> {
    let mut out: Vec<(String, UnitKind, String, String)> = evidence
        .units
        .iter()
        .map(|u| {
            (
                u.name.to_string(),
                u.kind,
                u.entries
                    .first()
                    .map(|e| e.as_str().to_string())
                    .unwrap_or_default(),
                u.roots
                    .first()
                    .map(|r| r.path.to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn every_cargo_target_is_a_unit_entered_through_its_own_file() {
    let evidence = read(
        "Cargo.toml",
        r#"
[package]
name = "demo"

[[bin]]
name = "tool"
path = "tools/tool.rs"
"#,
        &[
            "src/lib.rs",
            "src/main.rs",
            "build.rs",
            "tools/tool.rs",
            "src/bin/extra.rs",
            "src/bin/nested/main.rs",
            "tests/integration.rs",
            "tests/helpers/mod.rs",
            "benches/throughput.rs",
            "examples/quickstart.rs",
        ],
    );
    assert_eq!(
        units(&evidence),
        [
            (
                "bench:throughput".to_string(),
                UnitKind::Bench,
                "benches/throughput.rs".to_string(),
                "benches".to_string()
            ),
            (
                "bin:demo".to_string(),
                UnitKind::Executable,
                "src/main.rs".to_string(),
                "src".to_string()
            ),
            (
                "bin:extra".to_string(),
                UnitKind::Executable,
                "src/bin/extra.rs".to_string(),
                "src/bin".to_string()
            ),
            (
                "bin:nested".to_string(),
                UnitKind::Executable,
                "src/bin/nested/main.rs".to_string(),
                "src/bin/nested".to_string()
            ),
            (
                "bin:tool".to_string(),
                UnitKind::Executable,
                "tools/tool.rs".to_string(),
                "tools".to_string()
            ),
            (
                "build".to_string(),
                UnitKind::Tooling,
                "build.rs".to_string(),
                String::new()
            ),
            (
                "demo".to_string(),
                UnitKind::Library,
                "src/lib.rs".to_string(),
                "src".to_string()
            ),
            (
                "example:quickstart".to_string(),
                UnitKind::Example,
                "examples/quickstart.rs".to_string(),
                "examples".to_string()
            ),
            (
                "test:integration".to_string(),
                UnitKind::Test,
                "tests/integration.rs".to_string(),
                "tests".to_string()
            ),
        ],
        "tests/helpers/mod.rs is a module of a test crate, never a target"
    );
}

#[test]
fn a_library_is_published_unless_cargo_says_otherwise() {
    let published = read(
        "Cargo.toml",
        "[package]\nname = \"demo\"\n",
        &["src/lib.rs"],
    );
    // What a registry sees, a consumer names: `demo::a::Foo` is a module path,
    // not a file the manifest mapped, so every `pub` item is on the surface.
    assert_eq!(published.units[0].publication, Publication::ByName);
    assert!(published.units[0].is_published() && published.units[0].publishes_every_export());

    for spelling in ["publish = false", "publish = []"] {
        let private = read(
            "Cargo.toml",
            &format!("[package]\nname = \"demo\"\n{spelling}\n"),
            &["src/lib.rs"],
        );
        assert_eq!(
            private.units[0].publication,
            Publication::Unpublished,
            "{spelling}"
        );
        assert!(!private.units[0].is_published(), "{spelling}");
    }
}

#[test]
fn every_other_target_compiles_against_the_library() {
    let evidence = read(
        "Cargo.toml",
        "[package]\nname = \"demo\"\n\n[dependencies]\nserde = \"1\"\n",
        &["src/lib.rs", "tests/api.rs"],
    );
    let unit = |name: &str| {
        evidence
            .units
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("no unit {name}"))
    };
    assert_eq!(unit("demo").depends_on, [UnitDep::on("serde")]);
    assert_eq!(
        unit("test:api").depends_on,
        [UnitDep::on("serde"), UnitDep::on("demo")]
    );
    // Cargo never says an integration test may read what its library keeps
    // private, because it may not: it is a separate crate — so not one of the
    // dependencies it states is a friendship.
    assert!(
        unit("test:api")
            .depends_on
            .iter()
            .all(|d| d.grants == kndo_contract::manifest::Grant::Exports)
    );
}

#[test]
fn nested_package_units_stay_under_its_directory() {
    let evidence = read(
        "crates/core/Cargo.toml",
        "[package]\nname = \"core\"\n",
        &[
            "crates/core/src/lib.rs",
            "crates/core/tests/api.rs",
            "src/lib.rs",
        ],
    );
    assert_eq!(
        units(&evidence)
            .into_iter()
            .map(|(name, _, entry, _)| (name, entry))
            .collect::<Vec<_>>(),
        [
            ("core".to_string(), "crates/core/src/lib.rs".to_string()),
            (
                "test:api".to_string(),
                "crates/core/tests/api.rs".to_string()
            ),
        ]
    );
}

#[test]
fn a_virtual_workspace_declares_no_unit_and_still_pools_dependencies() {
    let evidence = read(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nserde = \"1\"\n",
        &["crates/a/src/lib.rs", "src/lib.rs"],
    );
    assert!(evidence.units.is_empty());
    assert!(evidence.packages.is_empty());
    assert_eq!(
        evidence
            .dependencies
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        ["serde"],
        "the pool a member inherits from is still a declaration"
    );
}

#[test]
fn dangling_and_broken_manifests_declare_nothing() {
    let dangling = read("Cargo.toml", "[package]\nname = \"demo\"\n", &["README.md"]);
    assert!(dangling.units.is_empty());
    let broken = read("Cargo.toml", "not [ toml", &["src/lib.rs"]);
    assert!(broken.units.is_empty());
    assert!(broken.packages.is_empty());
    assert!(broken.dependencies.is_empty());
}

#[test]
fn packages_are_entry_optional_and_underscored() {
    let files = &["crates/demo-core/src/lib.rs", "crates/tool/src/main.rs"];
    let libs = read(
        "crates/demo-core/Cargo.toml",
        "[package]\nname = \"demo-core\"\n",
        files,
    )
    .packages;
    assert_eq!(libs.len(), 1);
    assert_eq!(
        libs[0].name, "demo_core",
        "the ecosystem imports underscores"
    );
    assert_eq!(
        libs[0].aliases,
        ["demo-core"],
        "and the manifest DECLARES hyphens: cargo's own rename, so a dependency \
         declaration and an import spell one package two ways"
    );
    assert_eq!(
        libs[0].entry.as_ref().map(|e| e.as_str()),
        Some("crates/demo-core/src/lib.rs")
    );
    assert_eq!(libs[0].dir, "crates/demo-core");

    // A bin-only crate has no importable entry, but its directory still teaches
    // `package_of` which crate its files belong to.
    let bins = read(
        "crates/tool/Cargo.toml",
        "[package]\nname = \"tool\"\n",
        files,
    )
    .packages;
    assert_eq!(bins.len(), 1);
    assert!(bins[0].entry.is_none());
    assert_eq!(bins[0].dir, "crates/tool");
    assert!(
        bins[0].aliases.is_empty(),
        "a name with no hyphen is spelled one way, so it answers to nothing else"
    );
}

#[test]
fn lib_name_overrides_the_import_name() {
    let evidence = read(
        "Cargo.toml",
        "[package]\nname = \"demo-cli\"\n\n[lib]\nname = \"demo\"\n",
        &["src/lib.rs"],
    );
    assert_eq!(evidence.packages[0].name, "demo");
    assert_eq!(
        evidence.packages[0].aliases,
        ["demo-cli"],
        "the package the manifest declares still names the same entry"
    );
}

#[test]
fn declared_dependencies_report_every_cargo_table() {
    let manifest = r#"
[package]
name = "demo"

[dependencies]
serde = "1"
tokio = { version = "1", features = ["full"] }

[dev-dependencies]
tempfile = "3"

[build-dependencies]
cc = "1"

[workspace.dependencies]
thiserror = "2"

[target.'cfg(windows)'.dependencies]
winapi = "0.3"
"#;
    let mut deps = read("Cargo.toml", manifest, &["src/lib.rs"]).dependencies;
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    use kndo_contract::adapter::DependencyScope as S;
    let brief: Vec<(&str, Option<S>, Option<&str>)> = deps
        .iter()
        .map(|d| {
            (
                d.name.as_str(),
                d.scope,
                d.version_req.as_ref().map(|v| v.spelled.as_str()),
            )
        })
        .collect();
    assert_eq!(
        brief,
        [
            ("cc", Some(S::Build), Some("1")),
            ("serde", Some(S::Prod), Some("1")),
            ("tempfile", Some(S::Dev), Some("3")),
            // The workspace pool declares a comparable requirement with no
            // usage scope of its own.
            ("thiserror", None, Some("2")),
            ("tokio", Some(S::Prod), Some("1")),
            ("winapi", Some(S::Prod), Some("0.3")),
        ]
    );
}

#[test]
fn optional_dependencies_declare_optional_whatever_their_table() {
    use kndo_contract::adapter::DependencyScope as S;
    let deps = read(
        "Cargo.toml",
        "[package]\nname = \"x\"\n\n[dependencies]\nplain = \"1\"\narbitrary = { version = \"1.3\", optional = true }\n",
        &["src/lib.rs"],
    )
    .dependencies;
    let scope = |name: &str| deps.iter().find(|d| d.name == name).and_then(|d| d.scope);
    assert_eq!(scope("plain"), Some(S::Prod));
    assert_eq!(
        scope("arbitrary"),
        Some(S::Optional),
        "a feature gate is not a usage claim"
    );
}

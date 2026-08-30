//! Cargo.toml → roots and packages: declared targets, the auto-discovered
//! conventions, entry-optional packages, and degradation on dangling or broken
//! manifests.

use kndo_adapter_rust::RustAdapter;
use kndo_contract::adapter::{ResolveContext, SourceFile};
use kndo_contract::evidence::RootKind;
use kndo_contract::extension::Extension;
use kndo_contract::vocab::ProjectPath;
use std::collections::BTreeSet;

fn cx_files(paths: &[&str]) -> BTreeSet<ProjectPath> {
    paths.iter().map(|p| ProjectPath::new(*p)).collect()
}

fn roots_of(manifest_path: &str, manifest: &str, files: &[&str]) -> Vec<(String, RootKind)> {
    let known = cx_files(files);
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new(manifest_path);
    let mut out: Vec<(String, RootKind)> = RustAdapter::new()
        .roots(
            &SourceFile {
                path: &path,
                content: manifest.as_bytes(),
            },
            &cx,
        )
        .into_iter()
        .map(|r| (r.file.as_str().to_string(), r.kind))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn cargo_targets_and_conventions_root() {
    let roots = roots_of(
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
        roots,
        [
            ("benches/throughput.rs".to_string(), RootKind::Test),
            ("build.rs".to_string(), RootKind::Tooling),
            ("examples/quickstart.rs".to_string(), RootKind::Tooling),
            ("src/bin/extra.rs".to_string(), RootKind::Production),
            ("src/bin/nested/main.rs".to_string(), RootKind::Production),
            ("src/lib.rs".to_string(), RootKind::Production),
            ("src/main.rs".to_string(), RootKind::Production),
            ("tests/integration.rs".to_string(), RootKind::Test),
            ("tools/tool.rs".to_string(), RootKind::Production),
        ],
        "tests/helpers/mod.rs is a module of a test crate, never a root"
    );
}

#[test]
fn nested_package_roots_stay_under_its_directory() {
    let roots = roots_of(
        "crates/core/Cargo.toml",
        "[package]\nname = \"core\"\n",
        &[
            "crates/core/src/lib.rs",
            "crates/core/tests/api.rs",
            "src/lib.rs",
        ],
    );
    assert_eq!(
        roots,
        [
            ("crates/core/src/lib.rs".to_string(), RootKind::Production),
            ("crates/core/tests/api.rs".to_string(), RootKind::Test),
        ]
    );
}

#[test]
fn virtual_workspace_manifest_roots_nothing() {
    let roots = roots_of(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
        &["crates/a/src/lib.rs", "src/lib.rs"],
    );
    assert!(roots.is_empty());
}

#[test]
fn dangling_and_broken_manifests_anchor_nothing() {
    assert!(roots_of("Cargo.toml", "[package]\nname = \"demo\"\n", &["README.md"],).is_empty());
    assert!(roots_of("Cargo.toml", "not [ toml", &["src/lib.rs"]).is_empty());
}

#[test]
fn packages_are_entry_optional_and_underscored() {
    let known = cx_files(&["crates/demo-core/src/lib.rs", "crates/tool/src/main.rs"]);
    let cx = ResolveContext::new(&known);
    let adapter = RustAdapter::new();

    let lib_path = ProjectPath::new("crates/demo-core/Cargo.toml");
    let libs = adapter.packages(
        &SourceFile {
            path: &lib_path,
            content: b"[package]\nname = \"demo-core\"\n",
        },
        &cx,
    );
    assert_eq!(libs.len(), 1);
    assert_eq!(
        libs[0].name, "demo_core",
        "the ecosystem imports underscores"
    );
    assert_eq!(
        libs[0].entry.as_ref().map(|e| e.as_str()),
        Some("crates/demo-core/src/lib.rs")
    );
    assert_eq!(libs[0].dir, "crates/demo-core");

    // A bin-only crate has no importable entry, but its directory still teaches
    // `package_of` which crate its files belong to.
    let bin_path = ProjectPath::new("crates/tool/Cargo.toml");
    let bins = adapter.packages(
        &SourceFile {
            path: &bin_path,
            content: b"[package]\nname = \"tool\"\n",
        },
        &cx,
    );
    assert_eq!(bins.len(), 1);
    assert!(bins[0].entry.is_none());
    assert_eq!(bins[0].dir, "crates/tool");
}

#[test]
fn lib_name_overrides_the_import_name() {
    let known = cx_files(&["src/lib.rs"]);
    let cx = ResolveContext::new(&known);
    let path = ProjectPath::new("Cargo.toml");
    let pkgs = RustAdapter::new().packages(
        &SourceFile {
            path: &path,
            content: b"[package]\nname = \"demo-cli\"\n\n[lib]\nname = \"demo\"\n",
        },
        &cx,
    );
    assert_eq!(pkgs[0].name, "demo");
}

#[test]
fn manifest_dependencies_report_every_cargo_table() {
    let path = ProjectPath::new("Cargo.toml");
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
    let mut deps: Vec<String> = RustAdapter::new()
        .manifest_dependencies(&SourceFile {
            path: &path,
            content: manifest.as_bytes(),
        })
        .into_iter()
        .map(|d| d.to_string())
        .collect();
    deps.sort();
    assert_eq!(
        deps,
        ["cc", "serde", "tempfile", "thiserror", "tokio", "winapi"]
    );
}

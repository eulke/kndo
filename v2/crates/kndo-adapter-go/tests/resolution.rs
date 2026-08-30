//! Package-shaped resolution: the synthetic `"."` edge, module-path prefixes to
//! directories, multi-module trees, and the keep-alive `Unresolved` for the
//! standard library and everything external.

use kndo_adapter_go::GoAdapter;
use kndo_contract::adapter::{LanguageAdapter, PackageEntry, Resolution, ResolveContext};
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

fn project(paths: &[&str]) -> BTreeSet<ProjectPath> {
    paths.iter().map(|p| ProjectPath::new(*p)).collect()
}

fn files(paths: &[&str]) -> Resolution {
    Resolution::Files(paths.iter().map(|p| ProjectPath::new(*p)).collect())
}

#[test]
fn the_package_edge_is_the_non_test_siblings() {
    let known = project(&[
        "pkg/a.go",
        "pkg/b.go",
        "pkg/b_test.go",
        "pkg/sub/c.go",
        "other/d.go",
    ]);
    let cx = ResolveContext::new(&known);
    let adapter = GoAdapter::new();
    // Sibling files, the test file and the subdirectory excluded, self excluded.
    assert_eq!(
        adapter.resolve(&ProjectPath::new("pkg/a.go"), ".", &cx),
        files(&["pkg/b.go"])
    );
    // From the test file, the whole package is the target.
    assert_eq!(
        adapter.resolve(&ProjectPath::new("pkg/b_test.go"), ".", &cx),
        files(&["pkg/a.go", "pkg/b.go"])
    );
}

#[test]
fn module_paths_map_to_directories() {
    let known = project(&[
        "go.mod",
        "main.go",
        "sub/greet.go",
        "sub/format.go",
        "sub/format_test.go",
    ]);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    packages.insert(
        SmolStr::new("example.com/demo"),
        PackageEntry {
            name: SmolStr::new("example.com/demo"),
            entry: None,
            dir: SmolStr::new(""),
        },
    );
    let cx = ResolveContext::with_packages(&known, &packages);
    let adapter = GoAdapter::new();
    let from = ProjectPath::new("main.go");
    assert_eq!(
        adapter.resolve(&from, "example.com/demo/sub", &cx),
        files(&["sub/format.go", "sub/greet.go"])
    );
    assert_eq!(
        adapter.resolve(&from, "example.com/demo", &cx),
        files(&["main.go"])
    );
    // The standard library and foreign modules stay keep-alive.
    assert_eq!(adapter.resolve(&from, "fmt", &cx), Resolution::Unresolved);
    assert_eq!(
        adapter.resolve(&from, "github.com/other/dep", &cx),
        Resolution::Unresolved
    );
}

#[test]
fn longest_module_prefix_wins_across_a_workspace() {
    let known = project(&[
        "modA/go.mod",
        "modA/a.go",
        "modA/nested/go.mod",
        "modA/nested/n.go",
    ]);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    for (name, dir) in [
        ("example.com/a", "modA"),
        ("example.com/a/nested", "modA/nested"),
    ] {
        packages.insert(
            SmolStr::new(name),
            PackageEntry {
                name: SmolStr::new(name),
                entry: None,
                dir: SmolStr::new(dir),
            },
        );
    }
    let cx = ResolveContext::with_packages(&known, &packages);
    let adapter = GoAdapter::new();
    let from = ProjectPath::new("modA/a.go");
    assert_eq!(
        adapter.resolve(&from, "example.com/a/nested", &cx),
        files(&["modA/nested/n.go"])
    );
    assert_eq!(
        adapter.resolve(&from, "example.com/a", &cx),
        files(&["modA/a.go"])
    );
}

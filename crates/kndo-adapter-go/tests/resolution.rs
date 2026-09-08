//! Package-shaped resolution: the synthetic `"."` edge, module-path prefixes to
//! directories, multi-module trees, and the keep-alive `Unresolved` for the
//! standard library and everything external.

use kndo_adapter_go::GoAdapter;
use kndo_contract::adapter::{PackageEntry, Resolution, ResolveContext};
use kndo_contract::plugin::Plugin;
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
fn sight_is_declared_not_enumerated() {
    // What a Go package compiles together is the namespace this adapter
    // DECLARES — the directory plus the package clause — and the engine reads
    // the co-visible set off that node. Nothing enumerates a directory, and
    // the asymmetry that made the old enumeration subtle (a test file is
    // compiled into the test binary alone) is the engine's, pinned by
    // `crates/kndo/tests/mounts.rs`.
    let ev = kndo_testkit::extract_evidence(
        &GoAdapter::new(),
        "pkg/a.go",
        "package pkg\n\nfunc A() {}\n",
    );
    assert_eq!(
        ev.namespace.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        ["pkg", "pkg"],
        "the directory and the clause, which is what makes two `pkg` \
         directories two namespaces"
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
            aliases: Vec::new(),
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
                aliases: Vec::new(),
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

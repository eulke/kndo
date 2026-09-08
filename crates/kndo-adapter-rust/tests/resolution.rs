//! Module-path resolution against synthetic file trees: the prefix walk, `mod.rs`
//! fallback, `self`/`super` bases, item-in-file landings, workspace packages, and
//! the keep-alive `Unresolved` for everything external.

use kndo_adapter_rust::RustAdapter;
use kndo_contract::adapter::{PackageEntry, Resolution, ResolveContext};
use kndo_contract::plugin::Plugin;
use kndo_contract::vocab::ProjectPath;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};

fn project(paths: &[&str]) -> BTreeSet<ProjectPath> {
    paths.iter().map(|p| ProjectPath::new(*p)).collect()
}

fn resolve(files: &[&str], from: &str, specifier: &str) -> Resolution {
    kndo_testkit::resolve_in(&RustAdapter::new(), files, from, specifier)
}

fn file(path: &str) -> Resolution {
    Resolution::File(ProjectPath::new(path))
}

#[test]
fn crate_paths_walk_the_module_tree() {
    let files = [
        "src/lib.rs",
        "src/net.rs",
        "src/model/mod.rs",
        "src/model/user.rs",
    ];
    assert_eq!(
        resolve(&files, "src/lib.rs", "crate::net"),
        file("src/net.rs")
    );
    assert_eq!(
        resolve(&files, "src/net.rs", "crate::model::user"),
        file("src/model/user.rs")
    );
    // `mod.rs` is the fallback spelling of a directory module.
    assert_eq!(
        resolve(&files, "src/net.rs", "crate::model"),
        file("src/model/mod.rs")
    );
}

#[test]
fn longest_prefix_wins_and_items_land_in_their_file() {
    let files = ["src/lib.rs", "src/config.rs"];
    // `load` is an item inside config.rs — the prefix walk lands on the file.
    assert_eq!(
        resolve(&files, "src/lib.rs", "crate::config::load"),
        file("src/config.rs")
    );
    // An item at the crate root lands in the entry file itself.
    assert_eq!(
        resolve(&files, "src/config.rs", "crate::VERSION"),
        file("src/lib.rs")
    );
}

#[test]
fn self_and_super_are_module_relative() {
    let files = [
        "src/lib.rs",
        "src/a.rs",
        "src/a/b.rs",
        "src/a/sibling.rs",
        "src/top.rs",
    ];
    // a.rs parents src/a/ — its children live there.
    assert_eq!(resolve(&files, "src/a.rs", "self::b"), file("src/a/b.rs"));
    // From b.rs, super is module a; super::sibling is a's child.
    assert_eq!(
        resolve(&files, "src/a/b.rs", "super::sibling"),
        file("src/a/sibling.rs")
    );
    assert_eq!(
        resolve(&files, "src/a/b.rs", "super::super::top"),
        file("src/top.rs")
    );
    // Above the crate root lives another crate.
    assert_eq!(
        resolve(&files, "src/lib.rs", "super::anything"),
        Resolution::Unresolved
    );
    // `self` alone (a test mod's rebased `use super::*`) is the file itself.
    assert_eq!(resolve(&files, "src/a.rs", "self"), file("src/a.rs"));
}

#[test]
fn sibling_modules_arrive_package_shaped_and_fall_back() {
    let files = ["src/main.rs", "src/util.rs"];
    // `util::helper()` written qualified, no `use` — the resolver tries packages,
    // then module-relative from the crate root file.
    assert_eq!(
        resolve(&files, "src/main.rs", "util::helper"),
        file("src/util.rs")
    );
    // A path that matches nothing local is an external crate: keep-alive.
    assert_eq!(
        resolve(&files, "src/main.rs", "std::fs::read"),
        Resolution::Unresolved
    );
}

#[test]
fn workspace_packages_link_by_import_name() {
    let known = project(&[
        "crates/core/src/lib.rs",
        "crates/core/src/graph.rs",
        "crates/app/src/main.rs",
    ]);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    packages.insert(
        SmolStr::new("demo_core"),
        PackageEntry {
            name: SmolStr::new("demo_core"),
            entry: Some(ProjectPath::new("crates/core/src/lib.rs")),
            dir: SmolStr::new("crates/core"),
            aliases: Vec::new(),
        },
    );
    let cx = ResolveContext::with_packages(&known, &packages);
    let adapter = RustAdapter::new();
    let from = ProjectPath::new("crates/app/src/main.rs");
    assert_eq!(
        adapter.resolve(&from, "demo_core", &cx),
        file("crates/core/src/lib.rs")
    );
    assert_eq!(
        adapter.resolve(&from, "demo_core::graph", &cx),
        file("crates/core/src/graph.rs")
    );
    assert_eq!(
        adapter.resolve(&from, "demo_core::graph::assemble", &cx),
        file("crates/core/src/graph.rs")
    );
    assert_eq!(adapter.resolve(&from, "serde", &cx), Resolution::Unresolved);
}

#[test]
fn crate_base_comes_from_the_owning_package() {
    let known = project(&[
        "crates/core/src/lib.rs",
        "crates/core/src/graph.rs",
        "crates/app/src/main.rs",
        "crates/app/src/cli.rs",
    ]);
    let mut packages: BTreeMap<SmolStr, PackageEntry> = BTreeMap::new();
    for (name, dir, entry) in [
        ("demo_core", "crates/core", Some("crates/core/src/lib.rs")),
        ("demo_app", "crates/app", None),
    ] {
        packages.insert(
            SmolStr::new(name),
            PackageEntry {
                name: SmolStr::new(name),
                entry: entry.map(ProjectPath::new),
                dir: SmolStr::new(dir),
                aliases: Vec::new(),
            },
        );
    }
    let cx = ResolveContext::with_packages(&known, &packages);
    let adapter = RustAdapter::new();
    // crate:: inside app resolves against app's tree, not core's — and an
    // entryless (bin-only) package still knows its src dir.
    assert_eq!(
        adapter.resolve(
            &ProjectPath::new("crates/app/src/main.rs"),
            "crate::cli",
            &cx
        ),
        file("crates/app/src/cli.rs")
    );
    assert_eq!(
        adapter.resolve(
            &ProjectPath::new("crates/core/src/lib.rs"),
            "crate::graph",
            &cx
        ),
        file("crates/core/src/graph.rs")
    );
}

#[test]
fn single_file_crates_resolve_beside_themselves() {
    let files = ["src/lib.rs", "tests/integration.rs", "tests/common.rs"];
    assert_eq!(
        resolve(&files, "tests/integration.rs", "self::common"),
        file("tests/common.rs")
    );
}

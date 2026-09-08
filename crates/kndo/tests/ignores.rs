//! What a language's own tool never compiles, kndo discovers and never judges:
//! a file under a declared ignore is in the tree — an import into it is not
//! broken — and in no verdict, and a manifest under one declares nothing.

mod common;

use common::reported;
use kndo::Category;
use kndo_testkit::{MockPlugin, TempProject};

/// The kmock language whose tool never compiles a `vendor` directory.
fn vendoring() -> MockPlugin {
    MockPlugin::with(|spec| spec.ignores(&["**/vendor/**"]))
}

/// A project that imports from a vendored copy carrying its own manifest and
/// its own dead code.
fn vendored_project() -> TempProject {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/main.kmock\n",
    )
    .file(
        "src/main.kmock",
        "import ./vendor/dep { greet }\ncall greet\nfn stale\n",
    )
    .file("src/vendor/dep.kmock", "pub fn greet\nfn abandoned\n")
    .file(
        "src/vendor/kmock.pkg",
        "unit dep library entries=dep.kmock\n",
    );
    p
}

fn claimed(snap: &kndo::Snapshot) -> Vec<&str> {
    snap.graph.files.iter().map(|f| f.path.as_str()).collect()
}

fn units(snap: &kndo::Snapshot) -> Vec<&str> {
    snap.graph
        .project
        .units
        .iter()
        .map(|u| u.name.as_str())
        .collect()
}

#[test]
fn an_ignored_file_is_discovered_and_never_judged() {
    let p = vendored_project();
    let snap = common::analyze(&p, vec![Box::new(vendoring())]);

    // The tree knows the file; the graph judges only the project's own.
    assert_eq!(claimed(&snap), ["src/main.kmock"]);
    assert!(
        snap.graph
            .discovered
            .iter()
            .any(|p| p.as_str() == "src/vendor/dep.kmock")
    );
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/main.kmock — stale"]
    );
    // The import points at a file that exists: scope, not breakage.
    assert!(reported(&snap, &Category::UNRESOLVED).is_empty());
    // A manifest under the ignore declares nothing about this project.
    assert_eq!(units(&snap), ["core"]);
}

#[test]
fn without_the_declaration_the_same_tree_is_judged_whole() {
    let p = vendored_project();
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    assert_eq!(claimed(&snap), ["src/main.kmock", "src/vendor/dep.kmock"]);
    assert_eq!(units(&snap), ["core", "dep"]);
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["src/main.kmock — stale", "src/vendor/dep.kmock — abandoned"]
    );
}

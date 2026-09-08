//! The two structural imports. A mount nests a namespace: the file a `mod x;`
//! names is a child of the one that mounts it, its private names are readable
//! everywhere under it, and the mount's own reach fences everything below — a
//! `pub` item of a privately mounted module is nameable in the mounting
//! namespace and nowhere else, not on any unit's published surface. An include
//! pastes one file into another: the including file sees every name the
//! included one declares, whatever reach it carries.

mod common;

use common::{keeper_kinds, reaches, reported};
use kndo::Category;
use kndo_contract::plugin::PublishedSurface;
use kndo_testkit::{MockPlugin, TempProject};

/// The kmock language whose units publish every export, like a jar or a crate.
fn publishing() -> MockPlugin {
    MockPlugin::with(|spec| spec.published_surface(PublishedSurface::Exports))
}

#[test]
fn a_private_mount_fences_everything_under_it_off_the_published_surface() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    .file(
        "src/lib.kmock",
        "mount hidden ./hidden\npub mount shown ./shown\n",
    )
    // Under a private mount: exported as declared, its own parent's namespace
    // in effect, and nothing outside that namespace can name it.
    .file("src/hidden.kmock", "pub fn fenced\n")
    // Under an exported one: the unit hands it out.
    .file("src/shown.kmock", "pub fn open\n");
    let snap = common::analyze(&p, vec![Box::new(publishing())]);

    assert_eq!(
        reaches(&snap, "src/hidden.kmock#fenced"),
        ("exported".to_string(), "namespace+1".to_string())
    );
    assert_eq!(
        reaches(&snap, "src/shown.kmock#open"),
        ("exported".to_string(), "exported".to_string())
    );
    assert_eq!(keeper_kinds(&snap, "src/shown.kmock#open"), ["published"]);
    assert!(keeper_kinds(&snap, "src/hidden.kmock#fenced").is_empty());
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.contains(&"src/hidden.kmock — fenced".to_string()),
        "{unused:?}"
    );
    assert!(
        !unused.iter().any(|s| s.contains("open")),
        "a published export is nobody's to accuse: {unused:?}"
    );
}

#[test]
fn a_namespace_reaches_down_the_mounts_it_holds() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    .file(
        "src/lib.kmock",
        // Private to this module — and this module is the whole tree below.
        "mount a ./a\nns fn shared\n",
    )
    .file("src/a.kmock", "mount b ./b\ncall shared\n")
    // Two namespaces down, and still inside the one that holds `shared`.
    .file("src/b.kmock", "call shared\n");
    let snap = common::analyze(&p, vec![Box::new(publishing())]);

    assert_eq!(
        reaches(&snap, "src/lib.kmock#shared"),
        ("namespace".to_string(), "namespace".to_string())
    );
    // Every descendant naming it is a use in its pool: alive on references
    // alone, with no surface handed out and nothing published.
    let kinds = keeper_kinds(&snap, "src/lib.kmock#shared");
    assert!(
        !kinds.is_empty() && kinds.iter().all(|k| k == "reference"),
        "{kinds:?}"
    );
    assert!(reported(&snap, &Category::UNUSED).is_empty());
}

#[test]
fn a_reach_that_climbs_a_mount_pools_the_namespace_it_names() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    .file("src/lib.kmock", "mount a ./a\n")
    .file("src/a.kmock", "mount b ./b\nmount c ./c\ncall from_b\n")
    // `ns(1)` from `a::b` is `a` — its sibling `c` is inside that namespace,
    // its parent's parent is not.
    .file("src/b.kmock", "ns(1) fn from_b\nns fn own_b\ncall own_b\n")
    .file("src/c.kmock", "call from_b\n");
    let snap = common::analyze(&p, vec![Box::new(publishing())]);

    assert_eq!(
        reaches(&snap, "src/b.kmock#from_b"),
        ("namespace+1".to_string(), "namespace+1".to_string())
    );
    let kinds = keeper_kinds(&snap, "src/b.kmock#from_b");
    assert!(
        !kinds.is_empty() && kinds.iter().all(|k| k == "reference"),
        "{kinds:?}"
    );
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    // `own_b` is used in its own file alone, and its namespace is exactly its
    // own file's: nothing to advise, and nothing to accuse.
    assert!(reported(&snap, &Category::INTERNAL_ONLY).is_empty());
}

#[test]
fn a_unit_reach_pools_the_tree_when_no_manifest_named_the_unit() {
    let p = TempProject::new();
    // No manifest: the mounts are all the structure there is, and a
    // unit-reaching name pools the tree they spell.
    p.file("lib.kmock", "root-file\nmount a ./a\nunit fn wide\n")
        .file("a.kmock", "call wide\n")
        // Another tree entirely: naming `wide` from here is a different name.
        .file("other.kmock", "root-file\ncall wide\n");
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    assert_eq!(
        reaches(&snap, "lib.kmock#wide"),
        ("unit".to_string(), "unit".to_string())
    );
    assert_eq!(keeper_kinds(&snap, "lib.kmock#wide"), ["reference"]);
}

#[test]
fn an_include_makes_the_target_content_this_file_s_own() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    .file("src/lib.kmock", "include ./gen\ncall helper\n")
    // Private to its own file — and the file including it reads it anyway,
    // because the language pastes the content in rather than importing it.
    .file("src/gen.kmock", "ns fn helper\nns fn nobody\n");
    let snap = common::analyze(&p, vec![Box::new(publishing())]);

    assert_eq!(keeper_kinds(&snap, "src/gen.kmock#helper"), ["reference"]);
    let unused = reported(&snap, &Category::UNUSED);
    assert_eq!(
        unused,
        ["src/gen.kmock — nobody"],
        "sight is not a surface: what the includer never names is still dead"
    );
}

#[test]
fn a_covisible_namespace_reaches_its_own_files_but_never_through_a_test() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/lib.kmock\n",
    )
    // One namespace, three files: only the first exports, and the language
    // says its namespace compiles as one.
    .file("src/lib.kmock", "package pkg\nexport fn Api\n")
    .file("src/helpers.kmock", "package pkg\nns fn helper\n")
    // A test file of the same namespace: it may name what the namespace holds,
    // and the production colour neither enters nor leaves through it. Two
    // statements, not one: `test-file` is the runner's entry, `test-only` the
    // membership that fences the colour.
    .file(
        "src/lib_test.kmock",
        "package pkg\ntest-file\ntest-only\nexport fn TestApi\ncall helper\n",
    )
    // Another namespace entirely, exporting nothing and imported by nobody.
    .file("src/other/dead.kmock", "package other\nns fn alone\n");
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    // The exported file roots on the unit's published surface, and the
    // namespace carries that colour to the file that exports nothing.
    assert_eq!(common::color(&snap, "src/helpers.kmock"), "production");
    // Not through the test file, and not into it.
    assert_eq!(common::color(&snap, "src/lib_test.kmock"), "test-only");
    // A namespace nothing reaches stays unreached, whole file and all.
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.iter().any(|s| s.contains("src/other/dead.kmock")),
        "{unused:?}"
    );
    assert!(
        !unused.iter().any(|s| s.contains("helpers")),
        "a file the namespace reaches is not dead: {unused:?}"
    );
}

//! A declaration reaches as far as its language says, and no farther than its
//! owner lets it: the engine pools by the effective reach — the declared one
//! after every owner above caps it — and judges by the pool.

mod common;

use common::{keeper_kinds, reaches, reported};
use kndo::Category;
use kndo_contract::plugin::{Rung, Step};
use kndo_testkit::{MockPlugin, TempProject};

fn pair(declared: &str, effective: &str) -> (String, String) {
    (declared.to_string(), effective.to_string())
}

/// The kmock language with every rung the engine pools on its ladder — one
/// step per rung, under the word these tests read back.
fn laddered() -> MockPlugin {
    let words = [
        (Rung::Owner, "own"),
        (Rung::File, "local"),
        (Rung::Namespace, "ns"),
        (Rung::Directory, "tree"),
        (Rung::Heirs, "heirs"),
        (Rung::Unit, "unit"),
        (Rung::Group, "package"),
        (Rung::Exported, "pub"),
    ];
    let steps: Vec<Step> = words
        .into_iter()
        .map(|(rung, word)| match rung {
            Rung::Owner | Rung::Heirs => Step::for_members(rung, word),
            Rung::File => Step::for_free(rung, word),
            _ => Step::new(rung, word),
        })
        .collect();
    MockPlugin::laddered(&steps)
}

#[test]
fn a_members_reach_is_capped_by_its_owners() {
    let p = TempProject::new();
    p.file("kmock.pkg", "unit core library roots=src\n").file(
        "src/api.kmock",
        "pub type Shown\npub member Shown.show\nfile type Hidden\npub member Hidden.show\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    assert_eq!(
        reaches(&snap, "src/api.kmock#Shown.show"),
        pair("exported", "exported")
    );
    assert_eq!(
        reaches(&snap, "src/api.kmock#Hidden.show"),
        pair("exported", "file"),
        "a public member of a file-private type reaches the file"
    );
    // The published surface hands out the first and never the second.
    assert_eq!(
        keeper_kinds(&snap, "src/api.kmock#Shown.show"),
        ["published"]
    );
    assert!(keeper_kinds(&snap, "src/api.kmock#Hidden.show").is_empty());
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.contains(&"src/api.kmock — Hidden.show".to_string()),
        "{unused:?}"
    );
    assert!(!unused.iter().any(|s| s.contains("Shown")), "{unused:?}");
}

#[test]
fn an_inherited_member_reaches_as_its_owner_does() {
    let p = TempProject::new();
    p.file("kmock.pkg", "unit core library roots=src\n").file(
        "src/api.kmock",
        "pub type Contract\ninherited member Contract.run\nfile type Local\ninherited member Local.run\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    assert_eq!(
        reaches(&snap, "src/api.kmock#Contract.run"),
        pair("inherited", "exported")
    );
    assert_eq!(
        reaches(&snap, "src/api.kmock#Local.run"),
        pair("inherited", "file")
    );
    assert_eq!(
        keeper_kinds(&snap, "src/api.kmock#Contract.run"),
        ["published"]
    );
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.contains(&"src/api.kmock — Local.run".to_string()),
        "{unused:?}"
    );
    assert!(!unused.iter().any(|s| s.contains("Contract")), "{unused:?}");
}

#[test]
fn a_directory_reach_pools_the_subtree_above_the_file() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots= entries=src/app.kmock,elsewhere/far.kmock\n",
    )
    // Inside the fence (src/), binding one name: no whole-surface import,
    // which would keep everything the file hands out.
    .file(
        "src/app.kmock",
        "import ./internal/util { helper }\ncall helper\n",
    )
    .file(
        "src/internal/util.kmock",
        "dir(1) fn helper\ndir(1) fn lonely\ndir(1) fn fenced\ncall lonely\ncall fenced\n",
    )
    // Outside the fence: a bare use the pool does not hold.
    .file("elsewhere/far.kmock", "call fenced\n");
    let snap = common::analyze(&p, vec![Box::new(laddered())]);

    assert_eq!(
        reaches(&snap, "src/internal/util.kmock#helper"),
        pair("directory+1", "directory+1")
    );
    let keepers = keeper_kinds(&snap, "src/internal/util.kmock#helper");
    assert!(keepers.contains(&"binding".to_string()), "{keepers:?}");
    // `fenced` is used from outside the fence, which is not a use of this
    // declaration: like `lonely`, it reads as used in its own file alone, and
    // the ladder's step below the directory rung is the advice for both.
    assert_eq!(
        reported(&snap, &Category::INTERNAL_ONLY),
        [
            "src/internal/util.kmock — fenced",
            "src/internal/util.kmock — lonely"
        ]
    );
}

#[test]
fn a_group_reach_pools_the_units_one_manifest_aggregates() {
    let p = TempProject::new();
    p.file("kmock.pkg", "member a/kmock.pkg\nmember b/kmock.pkg\n")
        .file("a/kmock.pkg", "unit a library entries=a/lib.kmock\n")
        .file("b/kmock.pkg", "unit b library entries=b/main.kmock\n")
        .file("d/kmock.pkg", "unit d library entries=d/other.kmock\n")
        .file(
            "a/lib.kmock",
            "group fn shared\ngroup fn alone\ngroup fn afar\ncall alone\ncall afar\n",
        )
        // `b` is aggregated beside `a`: its use is `shared`'s.
        .file(
            "b/main.kmock",
            "import ./../a/lib { shared }\ncall shared\n",
        )
        // `d` is aggregated by nobody: its bare use is not `afar`'s.
        .file("d/other.kmock", "call afar\n");
    let snap = common::analyze(&p, vec![Box::new(laddered())]);

    assert_eq!(reaches(&snap, "a/lib.kmock#shared"), pair("group", "group"));
    let keepers = keeper_kinds(&snap, "a/lib.kmock#shared");
    assert!(keepers.contains(&"binding".to_string()), "{keepers:?}");
    assert_eq!(
        reported(&snap, &Category::INTERNAL_ONLY),
        ["a/lib.kmock — afar", "a/lib.kmock — alone"]
    );
}

#[test]
fn a_named_reach_pools_the_namespace_it_spells() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/x.kmock\n",
    )
    .file(
        "src/x.kmock",
        "package a.b\nnamed(a.b) fn f\nnamed(a.b) fn g\nimport ./y\nimport ./z\n",
    )
    .file("src/y.kmock", "package a.b\ncall f\n")
    .file("src/z.kmock", "package other\ncall g\n");
    let snap = common::analyze(&p, vec![Box::new(laddered())]);

    assert_eq!(
        reaches(&snap, "src/x.kmock#f"),
        pair("named:a.b", "named:a.b")
    );
    assert_eq!(keeper_kinds(&snap, "src/x.kmock#f"), ["reference"]);
    assert!(keeper_kinds(&snap, "src/x.kmock#g").is_empty());
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.contains(&"src/x.kmock — g".to_string()),
        "{unused:?}"
    );
}

#[test]
fn a_heirs_reach_pools_the_owner_its_subtypes_and_at_most_its_package() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        // An npm-shaped library: every file is an entry, and what an entry
        // does not hand out is internal — which is what makes the ladder's
        // advice sayable at all.
        "unit core library roots=src entries=src/base.kmock,src/sub.kmock,src/other.kmock,src/far.kmock publish=by-entry\n",
    )
    .file(
        "src/base.kmock",
        "package a\npub type Base\nheirs member Base.hook\nheirs member Base.lonely\nheirs+ns member Base.local\nheirs member Base.fenced\ncall lonely\n",
    )
    // A subtype, in another file: in the pool of every heirs member.
    .file(
        "src/sub.kmock",
        "package a\nimport ./base { Base }\npub type Sub\nextends Sub Base\ncall s.hook\n",
    )
    // The same package, no subtype: in the pool of `local` alone.
    .file(
        "src/other.kmock",
        "package a\nimport ./base { Base }\ncall Base.local\ncall Base.fenced\n",
    )
    // Another package, no subtype: in nobody's pool.
    .file("src/far.kmock", "package z\nimport ./base { Base }\ncall f.fenced\n");
    let snap = common::analyze(&p, vec![Box::new(laddered())]);

    assert_eq!(
        reaches(&snap, "src/base.kmock#Base.hook"),
        pair("subtypes", "subtypes")
    );
    assert_eq!(
        reaches(&snap, "src/base.kmock#Base.local"),
        pair("subtypes+namespace", "subtypes+namespace")
    );
    // `hook` is used from its subtype: its reach is what it needs. `lonely`
    // is used in its own file alone, `local` from its package and no subtype
    // (the package's rung). `fenced` is used from a package it does not
    // reach: not a use this pool counts, so no advice is drawn from it — and
    // a member dispatches through values, so that use still keeps it alive.
    assert_eq!(
        reported(&snap, &Category::INTERNAL_ONLY),
        [
            "src/base.kmock — Base.local",
            "src/base.kmock — Base.lonely"
        ]
    );
    let unused = reported(&snap, &Category::UNUSED);
    assert!(!unused.iter().any(|s| s.contains("Base.")), "{unused:?}");
}

#[test]
fn a_heirs_member_of_a_published_type_is_published_surface() {
    let p = TempProject::new();
    p.file("kmock.pkg", "unit core library roots=src\n").file(
        "src/api.kmock",
        "pub type Base\nheirs member Base.hook\nfile type Local\nheirs member Local.hook\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    // A subtype outside the tree may name it: kept, and never advised.
    assert_eq!(
        keeper_kinds(&snap, "src/api.kmock#Base.hook"),
        ["published"]
    );
    assert!(reported(&snap, &Category::INTERNAL_ONLY).is_empty());
    // Its owner's fence caps it to the file: nobody outside can, and nobody
    // inside does.
    assert_eq!(
        reaches(&snap, "src/api.kmock#Local.hook"),
        pair("subtypes", "file")
    );
    let unused = reported(&snap, &Category::UNUSED);
    assert!(
        unused.contains(&"src/api.kmock — Local.hook".to_string()),
        "{unused:?}"
    );
}

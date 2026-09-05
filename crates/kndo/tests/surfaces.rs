//! A surface hands out what the language exports and nothing else. An entry
//! point's exported API, and a namespace import that takes a file whole, keep
//! the members they can name — and never a private one, whose name cannot be
//! spelled outside the file that declares it.

mod common;

use common::{keeper_kinds, reported};
use kndo::Category;
use kndo_testkit::{MockExtension, TempProject};

#[test]
fn a_private_member_rides_no_surface() {
    let p = TempProject::new();
    // `lib` is an entry (its exports are the outside world's surface) and is
    // taken whole by a namespace importer. Both keep what the language hands
    // out; neither can name `hidden`.
    p.file(
        "lib.kmock",
        "root-file\npub type Widget\npub member Widget.shown\nmember Widget.hidden\n",
    )
    .file("app.kmock", "root-file\nimport ./lib\n");
    let snap = common::analyze(&p, vec![Box::new(MockExtension::new())]);

    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["lib.kmock — Widget.hidden"]
    );
    assert_eq!(
        keeper_kinds(&snap, "lib.kmock#Widget.shown"),
        ["surface-import", "entry-surface"],
        "both surfaces hand out an exported member"
    );
    assert!(keeper_kinds(&snap, "lib.kmock#Widget.hidden").is_empty());
}

#[test]
fn a_reference_anywhere_still_keeps_a_private_member() {
    // Dispatch is not lexical: `x.hidden()` names no owner, so any reachable
    // reference to the name keeps the member. Removing the surface keepers
    // narrows what a private member rides, never what a call site proves.
    let p = TempProject::new();
    p.file(
        "lib.kmock",
        "root-file\npub type Widget\nmember Widget.hidden\n",
    )
    .file("app.kmock", "root-file\nimport ./lib\ncall hidden\n");
    let snap = common::analyze(&p, vec![Box::new(MockExtension::new())]);
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    assert_eq!(
        keeper_kinds(&snap, "lib.kmock#Widget.hidden"),
        ["reference"]
    );
}

#[test]
fn a_member_on_a_promised_surface_is_kept_by_the_promise() {
    // `Impl` promises `Base`'s surface, and `Base` declares `handle`. No call
    // site can be required to exist for `Impl.handle`: every caller holds a
    // `Base` and dispatches through it.
    let p = TempProject::new();
    p.file(
        "base.kmock",
        "root-file\npub type Base\npub member Base.handle\n",
    )
    .file(
        "impl.kmock",
        "root-file\npub type Impl\nextends Impl Base\nmember Impl.handle\nmember Impl.helper\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockExtension::new())]);

    // The witness is kept; its sibling, promising nothing, is not.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["impl.kmock — Impl.helper"]
    );
    assert_eq!(
        keeper_kinds(&snap, "impl.kmock#Impl.handle"),
        ["witness"],
        "the promise alone keeps it — no reference names it anywhere"
    );
}

#[test]
fn a_namespace_pools_over_the_unit_that_compiles_it() {
    // Two source roots, ONE unit: the build compiles them together, so the
    // sibling's use is a use — a directory mirror rule could not say this,
    // and guava's `benchmark`/`test` pair is exactly this shape.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src,bench entries=src/com/foo/lib.kmock,bench/com/foo/other.kmock\n",
    )
    .file(
        "src/com/foo/lib.kmock",
        "package com.foo\nns fn internals\n",
    )
    .file(
        "bench/com/foo/other.kmock",
        "package com.foo\ncall internals\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockExtension::new())]);
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "one unit is one compilation, however many roots it names: {:?}",
        reported(&snap, &Category::UNUSED)
    );

    // Two UNITS with nothing between them: the same package name twice, and
    // neither may name the other — a mirrored flavor of a library.
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/com/foo/lib.kmock\n\
         unit mirror library roots=mirror entries=mirror/com/foo/other.kmock\n",
    )
    .file(
        "src/com/foo/lib.kmock",
        "package com.foo\nns fn internals\n",
    )
    .file(
        "mirror/com/foo/other.kmock",
        "package com.foo\ncall internals\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockExtension::new())]);
    assert!(
        reported(&snap, &Category::UNUSED)
            .contains(&"src/com/foo/lib.kmock — internals".to_string()),
        "another compilation cannot name it: {:?}",
        reported(&snap, &Category::UNUSED)
    );
}

#[test]
fn a_namespace_spans_the_unit_compiled_against_it_when_the_language_says_so() {
    // A separate test artifact that compiles against the library and declares
    // the same namespace: on one classpath, so it may name what the namespace
    // holds. guava's `guava-tests` against `guava` is this, and no `src/main`
    // ↔ `src/test` mirror rule reaches it.
    let project = || {
        let p = TempProject::new();
        p.file(
            "kmock.pkg",
            "unit core library roots=src entries=src/com/foo/lib.kmock\n\
             unit suite test roots=tests needs=core entries=tests/com/foo/spec.kmock\n",
        )
        .file(
            "src/com/foo/lib.kmock",
            "package com.foo\nns fn internals\n",
        )
        .file(
            "tests/com/foo/spec.kmock",
            "package com.foo\ncall internals\n",
        );
        p
    };

    let snap = common::analyze(&project(), vec![Box::new(MockExtension::spanning())]);
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "the suite compiles against the library, so its call is a use: {:?}",
        reported(&snap, &Category::UNUSED)
    );

    // The default span keeps every namespace inside its own unit, so the same
    // project accuses — the capability, not the manifest, is what decides.
    let snap = common::analyze(&project(), vec![Box::new(MockExtension::new())]);
    assert!(
        reported(&snap, &Category::UNUSED)
            .contains(&"src/com/foo/lib.kmock — internals".to_string()),
        "an undeclared span pools nothing across units: {:?}",
        reported(&snap, &Category::UNUSED)
    );
}

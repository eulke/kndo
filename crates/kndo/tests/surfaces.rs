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

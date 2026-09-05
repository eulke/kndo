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

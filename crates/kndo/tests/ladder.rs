//! `internal-only` reads the ladder and nothing else about a language: the
//! advice names the narrowest keyword the declaration's shape can take, and a
//! language that spells nothing between the declared rung and the rung its
//! uses need gives no advice at all.

mod common;

use common::reported;
use kndo::Category;
use kndo_contract::plugin::{Rung, Step};
use kndo_testkit::{MockPlugin, TempProject};

/// A mock language spelling four rungs: `own` for members alone, `local` for
/// top-level declarations alone, `unit`, and `pub`.
fn four_rungs() -> MockPlugin {
    MockPlugin::laddered(&[
        Step::for_members(Rung::Owner, "own"),
        Step::for_free(Rung::File, "local"),
        Step::new(Rung::Unit, "unit"),
        Step::new(Rung::Exported, "pub"),
    ])
}

fn advice(snap: &kndo::Snapshot) -> Vec<String> {
    let mut out: Vec<String> = snap
        .findings
        .iter()
        .filter(|f| f.category == Category::INTERNAL_ONLY)
        .map(|f| format!("{} — {}", f.subject.label(), f.message))
        .collect();
    out.sort();
    out
}

#[test]
fn a_unit_wide_name_used_only_in_its_file_falls_to_the_file_step() {
    let p = TempProject::new();
    // The manifest NAMES the unit: without one the reach has no bound, and an
    // unbounded reach is keep-alive with no advice to give.
    p.file("kmock.pkg", "unit core library roots=src\n")
        .file(
            "src/lib.kmock",
            "root-file\nunit fn helper\ncall helper\nunit fn shared\ncall shared\n",
        )
        .file("src/other.kmock", "root-file\ncall shared\n");
    let snap = common::analyze(&p, vec![Box::new(four_rungs())]);
    assert_eq!(
        advice(&snap),
        [
            "helper — declared `unit`, but every use is within its own file — `local` would \
             suffice for this function"
        ],
        "{:#?}",
        snap.findings
    );
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "the unit is the pool, and the sibling's call is a use: {:?}",
        reported(&snap, &Category::UNUSED)
    );
}

#[test]
fn a_member_takes_only_a_step_its_shape_can_stand_on() {
    // `local` exists for top-level declarations alone, and `own` covers less
    // than a use elsewhere in the file needs: nothing between them exists for
    // a member, so there is no advice — a keyword the member cannot take is
    // never named.
    let p = TempProject::new();
    p.file(
        "src/lib.kmock",
        "root-file\ntype Box\ncall Box\nunit member Box.grow\ncall grow\n",
    );
    let snap = common::analyze(&p, vec![Box::new(four_rungs())]);
    assert!(advice(&snap).is_empty(), "{:?}", advice(&snap));
    assert!(
        reported(&snap, &Category::UNUSED).is_empty(),
        "{:?}",
        reported(&snap, &Category::UNUSED)
    );
}

#[test]
fn a_language_spelling_nothing_narrower_gives_no_advice() {
    let p = TempProject::new();
    p.file("src/lib.kmock", "root-file\nunit fn helper\ncall helper\n");
    let two_rungs = MockPlugin::laddered(&[
        Step::new(Rung::Unit, "unit"),
        Step::new(Rung::Exported, "pub"),
    ]);
    let snap = common::analyze(&p, vec![Box::new(two_rungs)]);
    assert!(advice(&snap).is_empty(), "{:?}", advice(&snap));
    // No ladder at all: the same silence.
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    assert!(advice(&snap).is_empty(), "{:?}", advice(&snap));
}

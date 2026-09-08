//! A language's own conventions, declared as data and applied by the engine:
//! `go test` compiles exactly the `_test.go` files, pytest collects `test_*.py`,
//! a page is its own entry. The adapter states the glob and the colour
//! ([`kndo_contract::plugin::FileRole`]) and never reads a path to conclude
//! a root, so the precedence — a manifest that named the file's role outranks
//! every convention — is the engine's to apply once and not nine adapters' to
//! remember.

mod common;

use common::{color, reported};
use kndo::Category;
use kndo_contract::evidence::RootKind;
use kndo_contract::plugin::FileRole;
use kndo_testkit::{MockPlugin, TempProject};

/// The mock language's conventions: anything under `spec/` is the runner's, a
/// `*.entry.kmock` is a page-shaped entry of its own, and the two overlap
/// freely — a `spec/x.entry.kmock` is both.
fn conventional() -> MockPlugin {
    MockPlugin::with(|b| {
        b.file_roles(&[
            FileRole::certain("spec/**", RootKind::Test),
            FileRole::certain("**/spec/**", RootKind::Test),
            FileRole::probable("**/*.entry.kmock", RootKind::Production),
        ])
    })
}

#[test]
fn a_declared_convention_roots_where_no_manifest_spoke() {
    let p = TempProject::new();
    p.file(
        "spec/api.kmock",
        "import ./../src/lib { shared }\ncall shared\n",
    )
    .file("src/lib.kmock", "pub fn shared\n")
    .file("src/orphan.kmock", "pub fn floats\n");
    let snap = common::analyze(&p, vec![Box::new(conventional())]);

    // `spec/**` is Test: what it reaches is test-only, and what nothing
    // reaches at all is still dead.
    assert_eq!(color(&snap, "spec/api.kmock"), "test-only");
    assert_eq!(reported(&snap, &Category::TEST_ONLY), ["src/lib.kmock"]);
    assert_eq!(reported(&snap, &Category::UNUSED), ["src/orphan.kmock"]);
}

#[test]
fn a_unit_that_named_the_role_outranks_the_convention() {
    let p = TempProject::new();
    // The manifest compiles `spec/` as a LIBRARY: the directory is this
    // project's source, whatever the language's habit says about the name.
    p.file(
        "kmock.pkg",
        "unit core library roots=spec entries=spec/api.kmock\n",
    )
    .file(
        "spec/api.kmock",
        "import ./helper { shared }\ncall shared\n",
    )
    .file("spec/helper.kmock", "pub fn shared\n");
    let snap = common::analyze(&p, vec![Box::new(conventional())]);

    assert_eq!(
        color(&snap, "spec/api.kmock"),
        "production",
        "the unit said what its files are; the convention is not consulted"
    );
    assert!(
        reported(&snap, &Category::TEST_ONLY).is_empty(),
        "{:?}",
        reported(&snap, &Category::TEST_ONLY)
    );
}

#[test]
fn overlapping_globs_both_apply_and_the_test_half_does_not_seed_production() {
    let p = TempProject::new();
    // Both globs match this file: it is a page (Production) AND it is the
    // runner's (Test). The adapter states both facts without either having to
    // subtract the other, because a file the test build alone compiles seeds
    // no production flood whatever colour a root on it claims.
    p.file(
        "spec/page.entry.kmock",
        "test-only\nimport ./../src/lib { shared }\ncall shared\n",
    )
    .file("src/lib.kmock", "pub fn shared\n");
    let snap = common::analyze(&p, vec![Box::new(conventional())]);

    assert_eq!(color(&snap, "spec/page.entry.kmock"), "test-only");
    assert_eq!(
        reported(&snap, &Category::TEST_ONLY),
        ["src/lib.kmock"],
        "the Production glob matched too, and the attachment kept it from \
         painting what the test reaches"
    );
}

#[test]
fn a_language_declaring_nothing_reads_no_path_at_all() {
    let p = TempProject::new();
    p.file(
        "spec/api.kmock",
        "import ./../src/lib { shared }\ncall shared\n",
    )
    .file("src/lib.kmock", "pub fn shared\n");
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);

    // No convention declared, so `spec/` is a directory like any other and
    // nothing colours it: the default is silence, not a guess from the name.
    assert_eq!(
        color(&snap, "spec/api.kmock"),
        "unreachable",
        "the same tree the declaring language calls a test run: unreached here"
    );
    assert!(
        reported(&snap, &Category::TEST_ONLY).is_empty(),
        "{:?}",
        reported(&snap, &Category::TEST_ONLY)
    );
}

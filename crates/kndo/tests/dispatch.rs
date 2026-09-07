//! Markers mean what the claiming extension's dispatch rules say: a marker the
//! rules root becomes an entry of that color, a marker they exempt keeps the
//! declaration out of the unused judgment, a marker naming the file a
//! generator's output takes what it DECLARES out of every judgment while
//! leaving the file itself judged, and a file-level exemption is said aloud in
//! the report — the same engine path every language rides.

mod common;

use common::{keeper_kinds, reported};
use kndo::Category;
use kndo_contract::evidence::RootKind;
use kndo_contract::extension::{DispatchRule, Effect, Trigger};
use kndo_contract::vocab::Confidence;
use kndo_testkit::{MockExtension, TempProject};

fn rules() -> Vec<DispatchRule> {
    let certain = |when: Trigger, then: Effect| DispatchRule {
        when,
        then,
        confidence: Confidence::Certain,
    };
    vec![
        certain(Trigger::marker("test"), Effect::Root(RootKind::Test)),
        certain(Trigger::marker_with("allow", "dead_code"), Effect::Exempt),
        certain(Trigger::marker("generated"), Effect::Generated),
    ]
}

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockExtension::dispatching(rules()))])
}

#[test]
fn a_rooting_marker_is_an_entry_of_its_color() {
    let p = TempProject::new();
    // `check` carries the test marker and reaches `lib`; nothing production
    // does, so the file is test-only territory and nothing in it is unused.
    p.file(
        "check.kmock",
        "fn check\nmark check test\nimport ./lib { helper }\n",
    )
    .file("lib.kmock", "pub fn helper\n");
    let snap = run(&p);
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    assert_eq!(reported(&snap, &Category::TEST_ONLY), ["lib.kmock"]);
    assert_eq!(keeper_kinds(&snap, "check.kmock#check"), ["dispatch:test"]);

    // The same marker with no rule for it derives nothing: the file has no
    // root, so the run abstains from judging it at all.
    let plain = common::analyze(&p, vec![Box::new(MockExtension::new())]);
    assert!(plain.report().findings.is_empty());
    assert!(
        plain
            .report()
            .abstained
            .iter()
            .any(|a| a.category == Category::UNUSED),
        "{:?}",
        plain.report().abstained
    );
}

#[test]
fn an_exempting_marker_stands_the_unused_judgment_down() {
    let p = TempProject::new();
    p.file(
        "main.kmock",
        "root-file\nfn kept\nmark kept allow dead_code\nfn stale\nfn noted\nmark noted allow unused_imports\n",
    );
    let snap = run(&p);
    // Only the exemption the rules name counts: `unused_imports` is not it.
    assert_eq!(
        reported(&snap, &Category::UNUSED),
        ["main.kmock — noted", "main.kmock — stale"]
    );
    assert_eq!(keeper_kinds(&snap, "main.kmock#kept"), ["exempt"]);
    assert!(snap.report().diagnostics.is_empty());
}

#[test]
fn a_file_level_exemption_covers_everything_and_is_said_aloud() {
    let p = TempProject::new();
    p.file(
        "scratch.kmock",
        "root-file\nmark-file allow dead_code\nfn draft_one\nfn draft_two\n",
    );
    let snap = run(&p);
    assert!(reported(&snap, &Category::UNUSED).is_empty());
    let report = snap.report();
    assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(report.diagnostics[0].path.as_str(), "scratch.kmock");
    assert_eq!(
        report.diagnostics[0].message,
        "`allow(dead_code)` at file level exempts every declaration here (2) from the unused judgment"
    );
}

#[test]
fn a_generators_output_declares_nothing_this_project_answers_for() {
    let p = TempProject::new();
    // `codegen.kmock` is a generator's output that the app imports: the names
    // in it are the generator's, so the two nobody calls are not accusations
    // against this project — while `stale`, in hand-written code, still is.
    p.file(
        "main.kmock",
        "root-file\nimport ./codegen { Wire }\nfn stale\n",
    )
    .file(
        "codegen.kmock",
        "mark-file generated\npub fn Wire\npub fn Unused\npub fn AlsoUnused\n",
    );
    let snap = run(&p);
    assert_eq!(reported(&snap, &Category::UNUSED), ["main.kmock — stale"]);
    let report = snap.report();
    assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(report.diagnostics[0].path.as_str(), "codegen.kmock");
    assert_eq!(
        report.diagnostics[0].message,
        "`generated` marks this file a generator's output: what it declares is not judged, \
         what it imports and names still is"
    );
}

#[test]
fn a_generators_output_nobody_imports_is_still_dead_weight() {
    let p = TempProject::new();
    // Being generated is not a root: an orphan `.pb` nobody ever imports is
    // as removable as an orphan anyone wrote, and the FILE is the finding —
    // no declaration inside it is ever named.
    p.file("main.kmock", "root-file\n").file(
        "orphan.kmock",
        "mark-file generated\npub fn Wire\npub fn Unused\n",
    );
    let snap = run(&p);
    assert_eq!(reported(&snap, &Category::UNUSED), ["orphan.kmock"]);
}

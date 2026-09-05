//! Markers mean what the claiming extension's dispatch rules say: a marker the
//! rules root becomes an entry of that color, a marker they exempt keeps the
//! declaration out of the unused judgment, and a file-level exemption is said
//! aloud in the report — the same engine path every language rides.

mod common;

use common::reported;
use kndo::Category;
use kndo::query::{Answer, Outcome, Request, Verb};
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
    ]
}

fn run(project: &TempProject) -> kndo::Snapshot {
    common::analyze(project, vec![Box::new(MockExtension::dispatching(rules()))])
}

fn keeper_kinds(snap: &kndo::Snapshot, selector: &str) -> Vec<String> {
    let response = snap.query(&Request {
        verb: Verb::UsedBy,
        inputs: vec![selector.to_string()],
        options: Default::default(),
    });
    match &response.results[0] {
        Outcome::Ok {
            answer: Answer::UsedBy(a),
        } => a.kept_by.iter().map(|e| e.kind.to_string()).collect(),
        _ => panic!("used-by {selector}: not an answer"),
    }
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

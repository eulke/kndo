//! Suppression and the baseline, end-to-end: pragmas silence exactly their scope,
//! a stale allow is a finding, an allow over an abstained category is NOT stale
//! (the flicker rule), and known findings hold no gate hostage.

use kndo::{Category, Config, GatePolicy, RunMode, RunOutcome, Severity, Subject};
use kndo_testkit::TempProject;

fn run(p: &TempProject) -> (kndo::Session, kndo::Snapshot) {
    let session = kndo::open(p.root(), Config::default()).expect("open");
    let snap = session.analyze(RunMode::Full).expect("analyze");
    (session, snap)
}

fn categories(snap: &kndo::Snapshot) -> Vec<&str> {
    snap.findings.iter().map(|f| f.category.as_str()).collect()
}

#[test]
fn line_scope_suppresses_next_line_and_stale_is_a_finding() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file(
        "src/index.js",
        "// kndo:allow unused -- transitional\nfunction dead() {}\nexport function api() { return 1; }\n// kndo:allow unused\nexport function used() { return api(); }\n",
    );

    let (_, snap) = run(&p);
    // `dead` is suppressed by the first pragma; the second suppresses nothing and
    // is stale; `unused` still judges the rest of the graph.
    assert!(
        !snap
            .findings
            .iter()
            .any(|f| format!("{:?}", f.subject).contains("dead")),
        "{:#?}",
        snap.findings
    );
    assert_eq!(snap.suppressed.total, 1);
    let stale: Vec<_> = snap
        .findings
        .iter()
        .filter(|f| f.category == Category::STALE)
        .collect();
    assert_eq!(stale.len(), 1, "{:#?}", snap.findings);
    assert!(matches!(stale[0].subject, Subject::Suppression { .. }));
}

#[test]
fn an_allow_over_an_abstained_category_is_not_stale() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    // No test roots anywhere → untested abstains; the allow must NOT flip to stale.
    p.file(
        "src/index.js",
        "// kndo:allow untested\nexport function api() { return 1; }\n",
    );

    let (_, snap) = run(&p);
    assert!(
        snap.abstained
            .iter()
            .any(|a| a.category == Category::UNTESTED),
        "{:#?}",
        snap.abstained
    );
    assert!(
        !snap.findings.iter().any(|f| f.category == Category::STALE),
        "the flicker rule: an allow over an abstained category is not stale: {:#?}",
        snap.findings
    );
}

#[test]
fn baseline_splits_new_from_known_and_gates_only_the_new() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file("src/index.js", "export function api() { return 1; }\n");
    p.file("src/dead.js", "export function nobody() {}\n");

    let (session, snap) = run(&p);
    assert!(!snap.findings.is_empty());
    session.write_baseline(&snap).expect("write baseline");

    // Same tree, baseline accepted: nothing new, gate passes, everything counted.
    let (_, snap2) = run(&p);
    let report = snap2.report();
    assert_eq!(report.findings.len(), 0, "{:#?}", report.findings);
    assert_eq!(report.baselined, snap.findings.len() as u32);
    assert!(report.fixed.is_empty());
    assert_eq!(
        snap2.gate(&GatePolicy {
            fail_on: Some(Severity::Warning)
        }),
        RunOutcome::Pass
    );

    // A new dead file is NEW; fixing the old one shows as fixed.
    p.file("src/dead2.js", "export function nobodyEither() {}\n");
    std::fs::remove_file(p.root().join("src/dead.js")).expect("rm");
    let (_, snap3) = run(&p);
    let report = snap3.report();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.subject.path().as_str() == "src/dead2.js"),
        "{:#?}",
        report.findings
    );
    assert!(
        report
            .fixed
            .iter()
            .any(|f| f.subject.path().as_str() == "src/dead.js"),
        "{:#?}",
        report.fixed
    );
    assert!(matches!(
        snap3.gate(&GatePolicy {
            fail_on: Some(Severity::Warning)
        }),
        RunOutcome::FailFindings { .. }
    ));
    let _ = categories(&snap3);
}

//! The manifest capability end-to-end — the conformance case the capability rule
//! demands: a package.json anchors its entry, reachability flows from it, the entry's
//! exported surface belongs to the package consumer, and the dead stay dead.

use kndo::{Config, RunMode, Subject};
use kndo_testkit::TempProject;

#[test]
fn package_json_turns_judgment_on() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file(
        "src/index.js",
        "import { used } from \"./used.js\";\nexport function api() { return used(); }\nfunction stale() {}\n",
    );
    p.file("src/used.js", "export function used() { return 1; }\n");
    p.file("src/dead.js", "export function nobody() {}\n");

    let session = kndo::open(p.root(), Config::default()).expect("open");
    let snap = session.analyze(RunMode::Full).expect("analyze");

    // Production roots exist, so `unused` judges; the fixture has no test roots, so
    // the test-evidence analyses abstain — and say so, rather than accusing.
    assert!(
        !snap
            .abstained
            .iter()
            .any(|a| a.category == kndo::Category::UNUSED),
        "unused judges when roots exist: {:#?}",
        snap.abstained
    );
    assert_eq!(
        snap.abstained.len(),
        2,
        "test-only and untested abstain without test roots: {:#?}",
        snap.abstained
    );
    let subjects: Vec<String> = snap
        .findings
        .iter()
        .map(|f| match &f.subject {
            Subject::File { path } => format!("file:{}", path.as_str()),
            Subject::Symbol { path, selector, .. } => {
                format!("symbol:{}:{}", path.as_str(), selector.render())
            }
            other => format!("{other:?}"),
        })
        .collect();
    assert!(
        subjects.contains(&"file:src/dead.js".to_string()),
        "the unimported file is unused: {subjects:#?}"
    );
    assert!(
        subjects.contains(&"symbol:src/index.js:stale".to_string()),
        "a private uncalled function in the entry is dead: {subjects:#?}"
    );
    // The entry's exported surface belongs to the package consumer; the imported
    // helper is bound by the entry.
    assert!(!subjects.iter().any(|s| s.contains("api")), "{subjects:#?}");
    assert!(
        !subjects.iter().any(|s| s.contains(":used")),
        "{subjects:#?}"
    );
    assert_eq!(snap.findings.len(), 2, "{subjects:#?}");
}

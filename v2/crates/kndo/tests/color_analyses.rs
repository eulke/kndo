//! The color analyses end-to-end: test reachability keeps files from `unused` but
//! surfaces them as `test-only`; test references decide `untested`; and with test
//! evidence present, nothing abstains.

use kndo::{Category, Config, RunMode, Subject};
use kndo_testkit::TempProject;

fn subjects(snap: &kndo::Snapshot, category: &Category) -> Vec<String> {
    snap.findings
        .iter()
        .filter(|f| &f.category == category)
        .map(|f| match &f.subject {
            Subject::File { path } => format!("file:{}", path.as_str()),
            Subject::Symbol { path, selector, .. } => {
                format!("symbol:{}:{}", path.as_str(), selector.render())
            }
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn test_color_separates_test_only_from_unused_and_untested() {
    let p = TempProject::new();
    p.file(
        "package.json",
        r#"{ "name": "demo", "main": "src/index.js" }"#,
    );
    p.file(
        "src/index.js",
        "import { used } from \"./used.js\";\nexport function api() { return used(); }\nexport function uncovered() {}\n",
    );
    p.file("src/used.js", "export function used() { return 1; }\n");
    p.file(
        "src/fixtures-helper.js",
        "export function makeFixture() { return {}; }\n",
    );
    p.file(
        "test/app.test.js",
        "import { api } from \"../src/index.js\";\nimport { makeFixture } from \"../src/fixtures-helper.js\";\napi(); makeFixture();\n",
    );

    let session = kndo::open(p.root(), Config::default()).expect("open");
    let snap = session.analyze(RunMode::Full).expect("analyze");

    assert!(
        snap.abstained.is_empty(),
        "test evidence exists — nothing abstains: {:#?}",
        snap.abstained
    );

    // The helper only tests reach: alive (not unused), but flagged test-only.
    assert_eq!(
        subjects(&snap, &Category::TEST_ONLY),
        ["file:src/fixtures-helper.js"],
        "{:#?}",
        snap.findings
    );
    assert!(
        !subjects(&snap, &Category::UNUSED)
            .iter()
            .any(|s| s.contains("fixtures-helper")),
        "test-reachable is not unused: {:#?}",
        snap.findings
    );

    // `api` is exercised by the test; `uncovered` and `used` are not.
    let untested = subjects(&snap, &Category::UNTESTED);
    assert!(
        untested.contains(&"symbol:src/index.js:uncovered".to_string()),
        "{untested:#?}"
    );
    assert!(
        untested.contains(&"symbol:src/used.js:used".to_string()),
        "{untested:#?}"
    );
    assert!(
        !untested.iter().any(|s| s.contains(":api")),
        "{untested:#?}"
    );
    // The test file's own contents are never judged untested.
    assert!(
        !untested.iter().any(|s| s.contains("app.test")),
        "{untested:#?}"
    );
}

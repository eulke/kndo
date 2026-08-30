//! The color analyses end-to-end: test reachability keeps files from `unused` but
//! surfaces them as `test-only`; test reachability decides `untested` at file
//! granularity (anything a test imports, however indirectly, is exercised — and a
//! manifest-anchored entry is wiring, never judged); and with test evidence
//! present, nothing abstains.

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
        r#"{ "name": "demo", "main": "src/index.js", "bin": { "demo": "src/cli.js" } }"#,
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
    // A second production entry no test imports: its helper is the untested one.
    p.file(
        "src/cli.js",
        "import { runCli } from \"./cli-helper.js\";\nrunCli();\n",
    );
    p.file(
        "src/cli-helper.js",
        "export function runCli() { return 0; }\n",
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

    // Everything the test imports — `index.js` and, through it, `used.js` — is
    // exercised; the cli entry's helper is production-reachable with no test
    // anywhere above it. The anchored entries themselves are wiring, never judged.
    assert_eq!(
        subjects(&snap, &Category::UNTESTED),
        ["file:src/cli-helper.js"],
        "{:#?}",
        snap.findings
    );
}

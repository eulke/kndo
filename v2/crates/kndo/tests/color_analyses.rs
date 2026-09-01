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

#[test]
fn export_narrowing_fires_only_where_nothing_else_spells_the_name() {
    let p = TempProject::new();
    p.file("package.json", r#"{ "name": "demo", "main": "main.js" }"#);
    // The entry: whole-file-rooted, so its own exports are outside surface.
    p.file(
        "main.js",
        "import { used } from './lib.js';\n\
         import * as everything from './ns.js';\n\
         export function fromEntry() { return used() + everything.viaNs(); }\n",
    );
    // `local`: exported, used only here — the finding. `used`: bound by the
    // entry's import. `spelled`: used only here too, BUT an unreachable file
    // still imports it — the whole compilation disqualifies, not the live
    // subgraph.
    p.file(
        "lib.js",
        "export function used() { return local(); }\n\
         export function local() { return local; }\n\
         export function spelled() { return spelled; }\n",
    );
    // Namespace-imported: anything here may be used through the namespace.
    p.file("ns.js", "export function viaNs() { return viaNs; }\n");
    // Unreachable (nothing imports it, nothing roots it), and it BINDS the
    // name without ever referencing it — only the whole-compilation binding
    // set can protect `spelled` here; a reachability-gated one would accuse.
    p.file(
        "dead.js",
        "import { spelled } from './lib.js';\nexport const d = 1;\n",
    );
    let snap = kndo::open(p.root().to_path_buf(), Config::default())
        .unwrap()
        .analyze(RunMode::Full)
        .unwrap();
    let internal = subjects(&snap, &Category::INTERNAL_ONLY);
    assert_eq!(
        internal,
        vec!["symbol:lib.js:local".to_string()],
        "exactly the locally-used, never-imported export fires: {internal:?}"
    );
    let finding = snap
        .findings
        .iter()
        .find(|f| f.category == Category::INTERNAL_ONLY)
        .unwrap();
    assert!(
        finding.message.contains("declared exported"),
        "the Exported rung speaks its own message: {}",
        finding.message
    );
}

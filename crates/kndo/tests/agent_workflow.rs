//! The agent workflow, end to end, run against the real engine with the real adapters on a
//! real (temp) project: `find` → `used-by` → `impact --if-deleted` → *edit* → `check`. Four
//! bounded calls replace reading five files into context, and the final `check` is the
//! machine-verifiable proof the cleanup is complete.
//!
//! The fixture: a legacy tax path (`calcLegacyTax` + its `TaxTable`, in their own file,
//! importing `decimal.js`) consumed only by tests, next to the live path the app actually uses.

use std::fs;
use std::path::Path;

use kndo::engine::{ConfigOverrides, RunMode};
use kndo::query_envelope::{QueryFlags, QueryRequest, ResultEntry, Verb};

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{
  "name": "tax-demo",
  "version": "1.0.0",
  "main": "src/api.ts",
  "dependencies": { "decimal.js": "^10.4.0" }
}
"#,
    );
    write(
        root,
        "src/api.ts",
        "import { calcTax } from './tax';\n\nexport function api(x: number): number {\n  return calcTax(x);\n}\n",
    );
    write(
        root,
        "src/tax.ts",
        "export function calcTax(x: number): number {\n  return x * 2;\n}\n",
    );
    write(
        root,
        "src/legacy.ts",
        "import Decimal from 'decimal.js';\n\nexport const TaxTable = [1, 2, 3];\n\nexport function calcLegacyTax(x: number): number {\n  return new Decimal(x).toNumber() + TaxTable[0];\n}\n",
    );
    write(
        root,
        "src/legacy.test.ts",
        "import { calcLegacyTax } from './legacy';\n\nexport function testLow(): number {\n  return calcLegacyTax(1);\n}\n\nexport function testHigh(): number {\n  return calcLegacyTax(100);\n}\n",
    );
}

fn engine(root: &Path) -> kndo::engine::Engine {
    kndo::open(
        root,
        ConfigOverrides {
            use_cache: false,
            ..ConfigOverrides::default()
        },
    )
    .unwrap()
}

fn query(
    engine: &mut kndo::engine::Engine,
    verb: Verb,
    selector: &str,
    flags: QueryFlags,
) -> ResultEntry {
    let mut result = engine.query(QueryRequest {
        id: None,
        verb,
        selectors: vec![selector.to_string()],
        flags,
    });
    assert_eq!(result.results.len(), 1, "one selector, one result");
    result.results.remove(0)
}

#[test]
fn find_used_by_impact_check_closes_the_loop() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());

    // 1. find calcLegacyTax → the selector, no path knowledge needed up front.
    let mut e = engine(root.path());
    let found = query(&mut e, Verb::Find, "calcLegacyTax", QueryFlags::default());
    let ResultEntry::Find(found) = found else {
        panic!("find failed: {found:?}");
    };
    assert_eq!(found.matches.len(), 1);
    let selector = found.matches[0].selector.clone();
    assert_eq!(selector, "src/legacy.ts#calcLegacyTax");

    // 2. used-by --split-by-color → 0 production consumers, test consumers only: the
    //    "safe to delete (with its tests)" signal.
    let used_by = query(&mut e, Verb::UsedBy, &selector, QueryFlags::default());
    let ResultEntry::Neighbors(used_by) = used_by else {
        panic!("used-by failed: {used_by:?}");
    };
    assert_eq!(used_by.by_color.production, 0, "no production consumer");
    assert!(
        used_by.by_color.test_only >= 1,
        "consumed by tests: {:?}",
        used_by.by_color
    );

    // 3. impact src/legacy.ts --if-deleted → the full cleanup set BEFORE editing a line:
    //    deleting the legacy file frees decimal.js (its only importer) and orphans nothing
    //    the plan doesn't already cover.
    let impact = query(
        &mut e,
        Verb::Impact,
        "src/legacy.ts",
        QueryFlags {
            if_deleted: true,
            ..QueryFlags::default()
        },
    );
    let ResultEntry::Impact(impact) = impact else {
        panic!("impact failed: {impact:?}");
    };
    let sim = impact.if_deleted.as_ref().expect("--if-deleted requested");
    assert_eq!(
        sim.freed_dependencies,
        vec!["decimal.js".to_string()],
        "the legacy file is decimal.js's only importer"
    );

    // Pre-edit check: the legacy path shows up as test-only debt (the finding the cleanup
    // will fix), and decimal.js — imported only by test-reachable code — as a
    // dependency verdict too.
    let before = e.check(RunMode::Full);
    assert!(
        before.findings.iter().any(|f| f.category == "test-only"
            && f.location
                .path
                .as_ref()
                .is_some_and(|p| p.0.contains("legacy"))),
        "expected a test-only finding on the legacy path, got: {:?}",
        before
            .findings
            .iter()
            .map(|f| (&f.category, &f.location.path))
            .collect::<Vec<_>>()
    );

    // 4. The agent edits: delete function, tests, table, dependency — exactly the plan the
    //    three queries computed.
    fs::remove_file(root.path().join("src/legacy.ts")).unwrap();
    fs::remove_file(root.path().join("src/legacy.test.ts")).unwrap();
    write(
        root.path(),
        "package.json",
        r#"{
  "name": "tax-demo",
  "version": "1.0.0",
  "main": "src/api.ts"
}
"#,
    );

    // 5. check → the machine-verifiable proof: every finding the cleanup targeted is gone
    //    and the edit introduced nothing new.
    let mut e = engine(root.path());
    let after = e.check(RunMode::Full);
    assert!(
        after.findings.is_empty(),
        "cleanup must be complete and introduce nothing: {:?}",
        after
            .findings
            .iter()
            .map(|f| (&f.category, &f.message))
            .collect::<Vec<_>>()
    );

    let _ = fs::remove_dir_all(root.path());
}

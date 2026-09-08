//! Finding identity is unique per run: every subject a file can hold more
//! than once under one spelling carries its position, so two findings are
//! never one finding reported twice — and a baseline naming one can never
//! silence the other.

mod common;

use kndo::Category;
use kndo_testkit::{MockPlugin, TempProject};

#[test]
fn an_import_written_twice_is_two_unresolved_findings() {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit app executable roots=src entries=src/main.kmock\n",
    )
    .file(
        "src/main.kmock",
        "import ./missing\nfn work\nimport ./missing\ncall work\n",
    );
    let snap = common::analyze(&p, vec![Box::new(MockPlugin::new())]);
    let unresolved: Vec<_> = snap
        .findings
        .iter()
        .filter(|f| f.category == Category::UNRESOLVED)
        .collect();
    assert_eq!(unresolved.len(), 2, "{:#?}", snap.findings);
    assert_ne!(
        unresolved[0].id, unresolved[1].id,
        "two statements, two identities"
    );
    let mut labels: Vec<String> = unresolved.iter().map(|f| f.subject.label()).collect();
    labels.sort();
    assert_eq!(labels, ["import './missing'", "import './missing' #2"]);
}

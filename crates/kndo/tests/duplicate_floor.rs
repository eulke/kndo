//! A byte-identical file is a duplicate only above the floor: a file with too
//! few bytes outside its comments is identical to another by having nothing in
//! it, not by having been copied — and the measure ignores comments, so a
//! license header cannot make a stub look substantial.

mod common;

use common::reported;
use kndo::Category;
use kndo_contract::evidence::{EvidenceStream, EvidenceStreams};
use kndo_testkit::{MockExtension, TempProject};

/// The kmock language declaring the metrics stream, which `duplicate`
/// requires before it judges a file at all; the mock emits none, so only the
/// byte-identity pass has anything to say.
fn measured_for_duplicates() -> MockExtension {
    MockExtension::with(|spec| {
        spec.emits(EvidenceStreams::of(&[
            EvidenceStream::Comments,
            EvidenceStream::Markers,
            EvidenceStream::Relations,
            EvidenceStream::Qualifiers,
            EvidenceStream::Metrics,
        ]))
    })
}

fn duplicates(files: &[(&str, &str)]) -> Vec<String> {
    let p = TempProject::new();
    p.file(
        "kmock.pkg",
        "unit core library roots=src entries=src/a.kmock\n",
    );
    for (path, content) in files {
        p.file(path, content);
    }
    let snap = common::analyze(&p, vec![Box::new(measured_for_duplicates())]);
    reported(&snap, &Category::DUPLICATE)
}

/// A kmock module of `n` functions, the first of them imported by the entry.
fn module(n: usize) -> String {
    (0..n).map(|i| format!("pub fn f{i}\n")).collect()
}

#[test]
fn a_stub_is_identical_without_having_been_copied() {
    let entry = "import ./b { f0 }\nimport ./c { f0 }\ncall f0\n";
    // Twelve short lines: well under the floor, identical, and no finding.
    let small = module(12);
    assert!(small.len() < 200);
    assert!(
        duplicates(&[
            ("src/a.kmock", entry),
            ("src/b.kmock", &small),
            ("src/c.kmock", &small)
        ])
        .is_empty()
    );
    // Enough to have been copied: the second copy is the finding.
    let large = module(40);
    assert!(large.len() > 200);
    assert_eq!(
        duplicates(&[
            ("src/a.kmock", entry),
            ("src/b.kmock", &large),
            ("src/c.kmock", &large)
        ]),
        ["src/c.kmock"]
    );
}

#[test]
fn a_license_header_does_not_make_a_stub_substantial() {
    let entry = "import ./b { f0 }\nimport ./c { f0 }\ncall f0\n";
    let header: String = (0..8)
        .map(|_| "# Copyright (C) 2026 The Authors. Licensed under the Apache License 2.0.\n")
        .collect();
    let padded = format!("{header}{}", module(4));
    assert!(padded.len() > 400, "the header alone is over the floor");
    assert!(
        duplicates(&[
            ("src/a.kmock", entry),
            ("src/b.kmock", &padded),
            ("src/c.kmock", &padded)
        ])
        .is_empty(),
        "comments are not what could have been copied"
    );
}

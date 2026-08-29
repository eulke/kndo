//! Conformance for what happens BETWEEN adapters — the one thing a per-adapter fixture suite
//! structurally cannot cover, because each of those registers exactly one adapter.
//!
//! A misattribution like a Jazzy-generated `.js` file under `docs/` in a Swift repository having
//! its bare `require('jquery')` charged to the nearest ancestor manifest — `Package.swift` — as
//! a phantom undeclared dependency needs a JS adapter (to claim the file) and a Swift adapter
//! (to own the manifest it gets misattributed to) present at the same time — exactly what
//! `kndo::default_adapters()` gives and no single-adapter fixture can reproduce.
//!
//! Same fixture format as every adapter's own suite — `project/` plus `expected.json`, run
//! through the shared harness with the real `Engine`.

use std::path::Path;

use kndo_core::conformance::{run_fixture, run_fixture_dir};

#[test]
fn cross_language_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if let Err(msg) = run_fixture_dir(&root, |fixture| {
        run_fixture(fixture, kndo::default_adapters())
    }) {
        panic!("{msg}");
    }
}

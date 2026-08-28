//! Conformance for what happens BETWEEN adapters — the one thing a per-adapter fixture suite
//! structurally cannot cover, because each of those registers exactly one adapter.
//!
//! This is the suite that would have caught the jquery/Jazzy misattribution: a Jazzy-generated
//! `.js` file under `docs/` in a Swift repository had its bare `require('jquery')` charged to
//! the nearest ancestor manifest — `Package.swift` — and every Swift repo in the field audit
//! reported a phantom undeclared dependency for it. No Swift fixture could reproduce it (there
//! was no JS adapter to claim the file) and no JS fixture could either (there was no
//! `Package.swift` to misattribute to). It took both at once, which is exactly what
//! `kndo::default_adapters()` gives.
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

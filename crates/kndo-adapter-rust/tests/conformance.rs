//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `RustAdapter` — no mock, real discovery,
//! real extraction, real resolution, the real `Engine`. Mirrors `kndo-adapter-js`'s conformance
//! test exactly — the harness itself is fully adapter-agnostic.

use std::path::Path;

use kndo_adapter_rust::RustAdapter;
use kndo_core::conformance::{run_fixture, run_fixture_dir};

#[test]
fn rust_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if let Err(msg) = run_fixture_dir(&root, |fixture| {
        run_fixture(fixture, vec![Box::new(RustAdapter)])
    }) {
        panic!("{msg}");
    }
}

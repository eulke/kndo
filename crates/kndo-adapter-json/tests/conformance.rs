//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `JsonAdapter` alongside the real
//! `JsTsAdapter` — no mock, real discovery, real extraction, real resolution, the real
//! `Engine`. Both adapters are registered because JSON's entire value is cross-language:
//! a fixture with JSON alone could never demonstrate what an importer sees, nor the
//! "manifests are not claimed" guarantee for `package.json`.

use std::path::Path;

use kndo_adapter_js::JsTsAdapter;
use kndo_adapter_json::JsonAdapter;
use kndo_core::conformance::{run_fixture, run_fixture_dir};

#[test]
fn json_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if let Err(msg) = run_fixture_dir(&root, |fixture| {
        run_fixture(fixture, vec![Box::new(JsonAdapter), Box::new(JsTsAdapter)])
    }) {
        panic!("{msg}");
    }
}

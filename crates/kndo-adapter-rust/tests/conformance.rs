//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `RustAdapter` — no mock, real discovery,
//! real extraction, real resolution, the real `Engine`. Mirrors `kndo-adapter-js`'s conformance
//! test exactly — the harness itself is fully adapter-agnostic.

use std::path::Path;

use kndo_adapter_rust::RustAdapter;
use kndo_core::adapter::LanguageAdapter;
use kndo_core::conformance::{discover_fixtures, run_fixture, ConformanceVerdict};

#[test]
fn rust_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixtures = discover_fixtures(&root);
    assert!(
        !fixtures.is_empty(),
        "no conformance fixtures found under {}",
        root.display()
    );

    let mut failures = Vec::new();
    for fixture in &fixtures {
        let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(RustAdapter)];
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        match run_fixture(fixture, adapters) {
            Ok(ConformanceVerdict::Pass) => {}
            Ok(ConformanceVerdict::Mismatch(mismatch)) => {
                failures.push(format!("{name}:\n{mismatch}"))
            }
            Err(err) => failures.push(format!("{name}: {err}")),
        }
    }
    assert!(
        failures.is_empty(),
        "conformance fixture failures:\n\n{}",
        failures.join("\n")
    );
}

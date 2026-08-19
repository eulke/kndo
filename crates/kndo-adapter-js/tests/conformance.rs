//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`, RFC 0002 §8) with the real `JsTsAdapter` — no `MockAdapter`, real
//! discovery, real extraction, real resolution, the real `Engine`. This is the js-ts half of
//! the ROADMAP M1 exit criterion ("correct findings on fixture corpus").

use std::path::Path;

use kndo_adapter_js::JsTsAdapter;
use kndo_core::adapter::LanguageAdapter;
use kndo_core::conformance::{discover_fixtures, run_fixture};

#[test]
fn js_ts_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixtures = discover_fixtures(&root);
    assert!(
        !fixtures.is_empty(),
        "no conformance fixtures found under {}",
        root.display()
    );

    let mut failures = Vec::new();
    for fixture in &fixtures {
        let adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(JsTsAdapter)];
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        match run_fixture(fixture, adapters) {
            Ok(Ok(())) => {}
            Ok(Err(mismatch)) => failures.push(format!("{name}:\n{mismatch}")),
            Err(err) => failures.push(format!("{name}: {err}")),
        }
    }
    assert!(
        failures.is_empty(),
        "conformance fixture failures:\n\n{}",
        failures.join("\n")
    );
}

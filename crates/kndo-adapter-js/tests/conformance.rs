//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `JsTsAdapter` — no `MockAdapter`, real
//! discovery, real extraction, real resolution, the real `Engine`.

use std::path::Path;

use kndo_adapter_js::JsTsAdapter;
use kndo_core::adapter::LanguageAdapter;
use kndo_core::conformance::{discover_fixtures, run_fixture_with, ConformanceVerdict};
use kndo_core::plugin::Plugin;

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
        // The lcov ingester rides along the way the product composes it — the crap
        // fixture ships a coverage/lcov.info its expectations depend on.
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(kndo_plugin_coverage::LcovPlugin)];
        let name = fixture.file_name().unwrap().to_string_lossy().to_string();
        match run_fixture_with(fixture, adapters, plugins) {
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

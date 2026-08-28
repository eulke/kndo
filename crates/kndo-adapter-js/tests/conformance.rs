//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `JsTsAdapter` — no `MockAdapter`, real
//! discovery, real extraction, real resolution, the real `Engine`.

use std::path::Path;

use kndo_adapter_js::JsTsAdapter;
use kndo_core::conformance::{run_fixture_dir, run_fixture_with};
use kndo_core::plugin::Plugin;

#[test]
fn js_ts_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if let Err(msg) = run_fixture_dir(&root, |fixture| {
        // The lcov ingester rides along the way the product composes it — the crap
        // fixture ships a coverage/lcov.info its expectations depend on.
        let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(kndo_plugin_coverage::LcovPlugin)];
        run_fixture_with(fixture, vec![Box::new(JsTsAdapter)], plugins)
    }) {
        panic!("{msg}");
    }
}

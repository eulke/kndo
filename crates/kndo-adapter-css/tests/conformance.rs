//! Runs every fixture under `tests/fixtures/` through the shared conformance harness
//! (`kndo_core::conformance`) with the real `CssAdapter` alongside the real
//! `JsTsAdapter` — no mock, real discovery, real extraction, real resolution, the real
//! `Engine`. Both adapters are registered because CSS has no root of its own —
//! every fixture needs a JS-TS entry point to root the graph at all.

use std::path::Path;

use kndo_adapter_css::CssAdapter;
use kndo_adapter_js::JsTsAdapter;
use kndo_core::conformance::{run_fixture, run_fixture_dir};

#[test]
fn css_conformance_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if let Err(msg) = run_fixture_dir(&root, |fixture| {
        run_fixture(fixture, vec![Box::new(CssAdapter), Box::new(JsTsAdapter)])
    }) {
        panic!("{msg}");
    }
}

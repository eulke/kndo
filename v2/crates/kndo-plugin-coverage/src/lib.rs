//! The built-in coverage ingester: lcov from its conventional well-known paths,
//! parsed by `kndo-coverage` — the same parser the reference WASM ingester
//! compiles, so native and external coverage can never drift apart by prose.
//! Coverage is run output and usually gitignored, so the discovery walk
//! deliberately never sees it; the spec's `reads_reports` paths are the
//! sanctioned way in, and the ENGINE does the reading — this extension only
//! turns bytes into records.

use kndo_contract::evidence::CoverageRecords;
use kndo_contract::extension::{Activation, Extension, ExtensionSpec, MutatesGraph};
use std::sync::LazyLock;

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    ExtensionSpec::builder("kndo:coverage-lcov", 1)
        // MutatesGraph::No is load-bearing: an ingester contributes analysis
        // input, never graph facts, and Yes here would turn the persisted graph
        // cache off for every project, because this extension is always on.
        .conduct(Activation::Always, MutatesGraph::No)
        // The conventional lcov locations, tried in order; the first report that
        // parses AND maps onto the project wins.
        .reads_reports(&["lcov.info", "coverage/lcov.info"])
        .build()
});

pub struct LcovPlugin;

impl Extension for LcovPlugin {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        kndo_coverage::parse_lcov_records(std::str::from_utf8(content).ok()?)
    }
}

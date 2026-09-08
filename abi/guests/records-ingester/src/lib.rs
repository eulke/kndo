//! The reference external coverage ingester: `kndo-coverage`'s OWN lcov parser,
//! compiled to WASM, behind the REAL [`Plugin`] trait — the same `ingest`
//! hook the built-in implements, returning the same contract records (the SDK
//! does the wire conversion). The built-in native ingester and this component
//! share their parsing code by construction — the "synchronized by prose"
//! native/host pair the v1 postmortem recorded cannot exist here, because there
//! is one parser AND one hook.

use kndo_contract::evidence::CoverageRecords;
use kndo_contract::plugin::{Activation, Plugin, PluginSpec, MutatesGraph};
use std::sync::LazyLock;

static SPEC: LazyLock<PluginSpec> = LazyLock::new(|| {
    PluginSpec::builder("demo:lcov-records", 1)
        .conduct(Activation::Always, MutatesGraph::No)
        .reads_reports(&["lcov.info", "coverage/lcov.info"])
        .build()
});

#[derive(Default)]
struct RecordsIngester;

impl Plugin for RecordsIngester {
    fn spec(&self) -> &PluginSpec {
        &SPEC
    }

    fn ingest(&self, _report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        kndo_coverage::parse_lcov_records(std::str::from_utf8(content).ok()?)
    }
}

kndo_sdk::export_extension!(RecordsIngester);

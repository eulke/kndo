//! The built-in coverage ingester: lcov from its conventional well-known paths,
//! parsed by `kndo-coverage` — the same parser the reference WASM ingester
//! compiles, so native and external coverage can never drift apart by prose.
//! Coverage is run output and usually gitignored, so the discovery walk
//! deliberately never sees it; the well-known channel is the sanctioned way in.

use kndo_contract::vocab::ProjectPath;
use kndo_core::plugin::{Activation, Plugin, PluginSpec, WellKnown};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The conventional lcov locations, tried in order; the first parseable one wins.
const WELL_KNOWN_LCOV: [&str; 2] = ["lcov.info", "coverage/lcov.info"];

static SPEC: LazyLock<PluginSpec> = LazyLock::new(|| {
    PluginSpec::builder("kndo:coverage-lcov", 1)
        .activation(Activation::Always)
        .build()
});

pub struct LcovPlugin;

impl Plugin for LcovPlugin {
    fn spec(&self) -> &PluginSpec {
        &SPEC
    }

    /// An ingester contributes analysis input, never graph facts — and `false` is
    /// load-bearing: `true` here would turn the persisted graph cache off for
    /// every project, because this plugin is always on.
    fn mutates_graph(&self) -> bool {
        false
    }

    fn ingest_coverage(
        &self,
        well_known: &WellKnown<'_>,
        contents: &BTreeMap<ProjectPath, &[u8]>,
    ) -> Option<kndo_coverage::Coverage> {
        WELL_KNOWN_LCOV
            .iter()
            .filter_map(|candidate| well_known.read(candidate))
            .find_map(|text| kndo_coverage::parse_lcov(&text, contents))
    }
}

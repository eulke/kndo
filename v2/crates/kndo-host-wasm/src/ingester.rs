//! A loaded coverage-ingester component as a [`kndo_core::Plugin`]. The world is
//! unidirectional — the host locates the report through the spec's
//! `requested-file-access` entries read as well-known report paths, pushes the
//! bytes in, and assembles judgeable coverage from the returned records with
//! `kndo_coverage::assemble`, the same mapping half every ingester shares. The
//! `mutates_graph() == false` posture is structural: the world has no graph hooks
//! to answer otherwise.

use crate::LoadError;
use crate::bindings::ingester::CoverageIngester;
use crate::bindings::ingester::kndo::vocab::types as iwire;
use crate::convert::ingester_wire;
use crate::engine::{budgeted_store, guest_limits, shared_engine};
use kndo_contract::vocab::ProjectPath;
use kndo_core::plugin::{Plugin, PluginSpec, WellKnown, is_reserved_coordinate};
use std::collections::BTreeMap;
use std::path::Path;
use wasmtime::component::{Component, Linker};

/// The world imports nothing; the store carries only the memory limiter.
pub(crate) struct NoImports {
    limits: wasmtime::StoreLimits,
}

pub struct WasmIngester {
    component: Component,
    linker: Linker<NoImports>,
    spec: PluginSpec,
}

impl WasmIngester {
    /// Load a component targeting the coverage-ingester world; same one-shot spec
    /// read and `kndo:` namespace rejection as the plugin world.
    pub fn load(path: &Path) -> Result<WasmIngester, LoadError> {
        let bytes = std::fs::read(path)?;
        let component = Component::from_binary(shared_engine(), &bytes)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let linker: Linker<NoImports> = Linker::new(shared_engine());

        let mut store = budgeted_store(NoImports {
            limits: guest_limits(),
        });
        store.limiter(|d| &mut d.limits);
        let guest = CoverageIngester::instantiate(&mut store, &component, &linker)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = ingester_wire::plugin_spec(
            guest
                .call_spec(&mut store)
                .map_err(|e| LoadError::Component(e.to_string()))?,
        );
        if is_reserved_coordinate(spec.coordinate()) {
            return Err(LoadError::ReservedCoordinate {
                coordinate: spec.coordinate().to_string(),
            });
        }
        Ok(WasmIngester {
            component,
            linker,
            spec,
        })
    }

    fn ingest_one(&self, path: &str, content: &[u8]) -> Option<iwire::CoverageRecords> {
        let mut store = budgeted_store(NoImports {
            limits: guest_limits(),
        });
        store.limiter(|d| &mut d.limits);
        let guest =
            CoverageIngester::instantiate(&mut store, &self.component, &self.linker).ok()?;
        guest.call_ingest(&mut store, path, content).ok()?
    }
}

impl Plugin for WasmIngester {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    /// Structurally false: the world has no graph hooks, so an ingester can never
    /// turn the graph fast paths off.
    fn mutates_graph(&self) -> bool {
        false
    }

    fn ingest_coverage(
        &self,
        well_known: &WellKnown<'_>,
        contents: &BTreeMap<ProjectPath, &[u8]>,
    ) -> Option<kndo_coverage::Coverage> {
        for candidate in self.spec.requested_file_access() {
            let Some(text) = well_known.read(candidate) else {
                continue;
            };
            let Some(records) = self.ingest_one(candidate, text.as_bytes()) else {
                continue;
            };
            if let Some(coverage) =
                kndo_coverage::assemble(ingester_wire::coverage_records(records), contents)
            {
                return Some(coverage);
            }
        }
        None
    }
}

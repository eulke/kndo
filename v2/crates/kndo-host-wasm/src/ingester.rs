//! A loaded coverage-ingester component as an [`Extension`]. The world is
//! unidirectional: the ENGINE locates each report through the spec's
//! `reads_reports` paths (the wire spells them as `requested-file-access`; the
//! bridge restates them on the field that means it), pushes the bytes here, and
//! this hook only turns them into records. The `mutates_graph == false` posture
//! is structural: the world has no graph hooks to answer otherwise.

use crate::LoadError;
use crate::bindings::ingester::CoverageIngester;
use crate::bindings::ingester::kndo::vocab::types as iwire;
use crate::convert::ingester_wire;
use crate::engine::{budgeted_store, guest_limits, shared_engine};
use kndo_contract::evidence::CoverageRecords;
use kndo_contract::extension::{Extension, ExtensionSpec};
use kndo_core::plugin::is_reserved_coordinate;
use std::path::Path;
use wasmtime::component::{Component, Linker};

/// The world imports nothing; the store carries only the memory limiter.
pub(crate) struct NoImports {
    limits: wasmtime::StoreLimits,
}

pub struct WasmIngester {
    component: Component,
    linker: Linker<NoImports>,
    spec: ExtensionSpec,
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
        let mut parts = ingester_wire::plugin_parts(
            guest
                .call_spec(&mut store)
                .map_err(|e| LoadError::Component(e.to_string()))?,
        );
        parts.reads_reports = std::mem::take(&mut parts.requested_file_access);
        let spec: ExtensionSpec = parts.into();
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

impl Extension for WasmIngester {
    fn spec(&self) -> &ExtensionSpec {
        &self.spec
    }

    fn ingest(&self, report_path: &str, content: &[u8]) -> Option<CoverageRecords> {
        self.ingest_one(report_path, content)
            .map(ingester_wire::coverage_records)
    }
}

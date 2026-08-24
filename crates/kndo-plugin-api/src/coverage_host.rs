//! The `coverage-ingester` world's host side — external WASM coverage ingesters as
//! [`Plugin`]s, third sibling of the `plugin`/`plugin-findings` bridge in `plugin_host.rs`.
//!
//! A deliberately smaller bridge than the graph-hooks one: the world has no imports (the
//! host locates the report, freshness-checks it, and pushes the bytes in — see the WIT's
//! world doc), so there is no `HostViewData`, no round lifecycle, and no shared instance —
//! each `ingest_coverage` call instantiates ephemerally, calls the one export, and drops
//! the instance. Sink discipline over the ABI: the guest returns facts
//! (`ingested-coverage`), and the host alone writes the [`CoverageSink`], records
//! provenance, and rebases paths — identically to the native ingesters in
//! `kndo-plugin-coverage`, whose `mutates_graph() == false` posture this type applies
//! structurally.

use std::path::Path;

use kndo_core::adapter::ProjectPath;
use kndo_core::coverage::CoverageSink;
use kndo_core::plugin::{Plugin, PluginDescriptor};

use crate::engine::FUEL_PER_CALL;
use crate::plugin_host::{ensure_unreserved, read_component_bytes, LoadError};

/// Generated standalone, NOT `with`-shared against `plugin_host::findings_bindings`: bindgen
/// only materializes the types a world's own functions reach, and the graph-hook worlds
/// never touch `coverage-line`/`ingested-coverage` (nor this world most of theirs) — there
/// is no common superset module to share. The overlap is exactly one record
/// (`plugin-descriptor` + its `activation-rule`), converted by the two small fns below.
mod coverage_bindings {
    wasmtime::component::bindgen!({
        path: "wit/plugin.wit",
        world: "coverage-ingester",
    });
}

use coverage_bindings::kndo::plugin::types as cw;
use coverage_bindings::CoverageIngester as WitCoverageBindings;

fn native_descriptor(raw: cw::PluginDescriptor) -> PluginDescriptor {
    PluginDescriptor {
        id: smol_str::SmolStr::new(&raw.id),
        version: smol_str::SmolStr::new(&raw.version),
        detection: raw.detection.iter().map(smol_str::SmolStr::new).collect(),
        requested_file_access: raw
            .requested_file_access
            .iter()
            .map(smol_str::SmolStr::new)
            .collect(),
        activation: raw.activation.into_iter().map(native_rule).collect(),
        dependencies: raw
            .dependencies
            .iter()
            .map(smol_str::SmolStr::new)
            .collect(),
    }
}

fn native_rule(rule: cw::ActivationRule) -> kndo_core::plugin::ActivationRule {
    match rule {
        cw::ActivationRule::FileExists(glob) => {
            kndo_core::plugin::ActivationRule::FileExists(smol_str::SmolStr::new(&glob))
        }
        cw::ActivationRule::ManifestDependency(name) => {
            kndo_core::plugin::ActivationRule::ManifestDependency(smol_str::SmolStr::new(&name))
        }
    }
}

pub struct WasmCoverageIngester {
    engine: wasmtime::Engine,
    component: wasmtime::component::Component,
    linker: wasmtime::component::Linker<()>,
    descriptor: PluginDescriptor,
    // Same role as `WasmPlugin::content_hash`: proof of *this exact* `.wasm` file for the
    // graph-cache key machinery, which treats every plugin uniformly.
    content_hash: [u8; 32],
}

impl WasmCoverageIngester {
    pub fn load(path: &Path) -> Result<WasmCoverageIngester, LoadError> {
        let bytes = read_component_bytes(path)?;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        let engine = crate::engine::shared_engine().clone();
        let component = wasmtime::component::Component::from_binary(&engine, &bytes)
            .map_err(|e| LoadError::Instantiate(e.to_string()))?;
        // No imports to link — an empty linker both instantiates the world and rejects
        // components of the other worlds (their imports are unsatisfiable here), which is
        // what lets the loader chain in `kndo` sort a mixed directory by trying each type.
        let linker = wasmtime::component::Linker::new(&engine);
        let (mut store, bindings) = instantiate(&engine, &component, &linker)?;
        let raw = bindings
            .call_descriptor(&mut store)
            .map_err(|e| LoadError::Instantiate(format!("descriptor() failed: {e}")))?;
        ensure_unreserved(&raw.id)?;
        Ok(WasmCoverageIngester {
            engine,
            component,
            linker,
            descriptor: native_descriptor(raw),
            content_hash,
        })
    }
}

fn instantiate(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
    linker: &wasmtime::component::Linker<()>,
) -> Result<(wasmtime::Store<()>, WitCoverageBindings), LoadError> {
    let mut store = wasmtime::Store::new(engine, ());
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;
    let bindings = WitCoverageBindings::instantiate(&mut store, component, linker)
        .map_err(|e| LoadError::Instantiate(e.to_string()))?;
    Ok((store, bindings))
}

impl Plugin for WasmCoverageIngester {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn content_hash(&self) -> Option<[u8; 32]> {
        Some(self.content_hash)
    }

    /// Structural for this world: it has no graph hooks to mutate anything with, and the
    /// declaration keeps both graph fast paths (snapshot cache, incremental patch) alive —
    /// the same contract the native ingesters state.
    fn mutates_graph(&self) -> bool {
        false
    }

    /// Ephemeral instantiate → one fuel-metered `ingest-coverage` call → drop. A trap or
    /// fuel exhaustion means this report contributed nothing (the adapter-bridge posture:
    /// degrade to absence, never to a partial half-trusted result) — the host's sink is
    /// only written from a successful return.
    fn ingest_coverage(&self, path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {
        let Ok((mut store, bindings)) = instantiate(&self.engine, &self.component, &self.linker)
        else {
            return;
        };
        let Ok(ingested) = bindings.call_ingest_coverage(&mut store, path.0.as_str(), content)
        else {
            return;
        };
        for line in ingested.lines {
            out.add_line(
                ProjectPath(smol_str::SmolStr::new(&line.path)),
                line.line,
                line.hits,
            );
        }
        for source in ingested.sources {
            out.add_source(source);
        }
    }
}

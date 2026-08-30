//! A loaded component as a [`kndo_core::Plugin`]. The containment model is not
//! re-implemented here — it is INHERITED: the bridge writes through the same
//! `PluginSink` a native plugin does, so undeclared rules, misdirected targets and
//! smuggled roots drop under the engine's own rules, and the spec's declared globs
//! and budget govern content through the engine's own `ContentView`.

use crate::LoadError;
use crate::bindings::plugin::{Plugin as PluginWorld, PluginImports};
use crate::convert::plugin_wire;
use crate::engine::{budgeted_store, guest_limits, shared_engine};
use kndo_core::plugin::{
    ContentView, GraphView, Plugin, PluginSink, PluginSpec, is_reserved_coordinate,
};
use std::collections::BTreeMap;
use std::path::Path;
use wasmtime::component::{Component, Linker};

/// One round's read surface, snapshotted before instantiation: the graph's paths,
/// and every declared-glob content match the engine's own `ContentView` allowed —
/// prefetched THROUGH it, so globs and the budget are charged by the one
/// authority. A WASM plugin's content budget is charged by declaration, not by
/// demand; the cut still lands on the contribution because the view remembers.
pub(crate) struct PluginStoreData {
    paths: Vec<String>,
    snapshot: BTreeMap<String, Vec<u8>>,
    limits: wasmtime::StoreLimits,
}

impl crate::bindings::plugin::kndo::vocab::types::Host for PluginStoreData {}

impl PluginImports for PluginStoreData {
    fn graph_paths(&mut self) -> Vec<String> {
        self.paths.clone()
    }

    fn graph_contains(&mut self, path: String) -> bool {
        self.paths.binary_search(&path).is_ok()
    }

    fn read_file(&mut self, path: String) -> Option<Vec<u8>> {
        self.snapshot.get(&path).cloned()
    }
}

pub struct WasmPlugin {
    component: Component,
    linker: Linker<PluginStoreData>,
    spec: PluginSpec,
    mutates: bool,
}

impl WasmPlugin {
    /// Load a component targeting the plugin world. The spec and the
    /// `mutates-graph` answer are read once and cached — the cache-bypass decision
    /// happens before any round, so the answer must be stable for the process.
    /// External components claiming the `kndo:` namespace are rejected here.
    pub fn load(path: &Path) -> Result<WasmPlugin, LoadError> {
        let bytes = std::fs::read(path)?;
        let component = Component::from_binary(shared_engine(), &bytes)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let mut linker: Linker<PluginStoreData> = Linker::new(shared_engine());
        PluginWorld::add_to_linker(&mut linker, |data| data)
            .map_err(|e| LoadError::Component(e.to_string()))?;

        let mut store = budgeted_store(PluginStoreData {
            paths: Vec::new(),
            snapshot: BTreeMap::new(),
            limits: guest_limits(),
        });
        store.limiter(|d| &mut d.limits);
        let guest = PluginWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = guest
            .call_spec(&mut store)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let mutates = guest
            .call_mutates_graph(&mut store)
            .map_err(|e| LoadError::Component(e.to_string()))?;
        let spec = plugin_wire::plugin_spec(spec);
        if is_reserved_coordinate(spec.coordinate()) {
            return Err(LoadError::ReservedCoordinate {
                coordinate: spec.coordinate().to_string(),
            });
        }
        Ok(WasmPlugin {
            component,
            linker,
            spec,
            mutates,
        })
    }

    fn round_data(&self, graph: &GraphView<'_>, content: &ContentView<'_>) -> PluginStoreData {
        let paths: Vec<String> = graph.paths().map(|p| p.as_str().to_string()).collect();
        let mut snapshot = BTreeMap::new();
        // The view's own enumeration and its own budgeted reads — the glob scope
        // and the charge live in one place, and a discovered-but-unclaimed file
        // (a config no adapter speaks) is readable here exactly as natively.
        for path in content.readable_paths() {
            if let Some(bytes) = content.read(path) {
                snapshot.insert(path.as_str().to_string(), bytes.to_vec());
            }
        }
        PluginStoreData {
            paths,
            snapshot,
            limits: guest_limits(),
        }
    }

    /// A fresh, budgeted instance for one hook call; a trap contributes nothing.
    fn call<T>(
        &self,
        data: PluginStoreData,
        f: impl FnOnce(&PluginWorld, &mut wasmtime::Store<PluginStoreData>) -> wasmtime::Result<T>,
    ) -> Option<T> {
        let mut store = budgeted_store(data);
        store.limiter(|d| &mut d.limits);
        let guest = PluginWorld::instantiate(&mut store, &self.component, &self.linker).ok()?;
        f(&guest, &mut store).ok()
    }
}

impl Plugin for WasmPlugin {
    fn spec(&self) -> &PluginSpec {
        &self.spec
    }

    fn mutates_graph(&self) -> bool {
        self.mutates
    }

    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        let Some(roots) = self.call(self.round_data(graph, content), |guest, store| {
            guest.call_contribute_roots(store)
        }) else {
            return;
        };
        for root in roots {
            out.root(
                plugin_wire::plugin_target(root.target),
                plugin_wire::root_kind(root.kind),
                plugin_wire::confidence(root.confidence),
            );
        }
    }

    fn report_findings(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut PluginSink,
    ) {
        let Some(findings) = self.call(self.round_data(graph, content), |guest, store| {
            guest.call_report_findings(store)
        }) else {
            return;
        };
        for finding in findings {
            out.finding(
                &finding.rule,
                plugin_wire::plugin_severity(finding.severity),
                plugin_wire::plugin_target(finding.target),
                finding.message,
            );
        }
    }
}

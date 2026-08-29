//! `kndo:wasmtime` — the built-in wasmtime component conventions plugin.
//!
//! `wasmtime::component::bindgen!` generates a trait per WIT interface and one per world's
//! imports, and the host implements them by hand. Nothing in the repo calls those methods: the
//! caller is generated glue that runs when a GUEST calls out, so the whole host surface reads
//! as unreachable to static reference analysis even though every method fires at runtime —
//! reachability through generated code is invisible by construction, not a hole in this
//! plugin's rules.
//!
//! The traits are recognized by the SHAPE `bindgen!` gives them rather than by a list of
//! names, because the names come from the project's own WIT: `Host` per interface, `Host<Res>`
//! per resource, `{World}Imports` per world. A fixed list is impossible in principle here, not
//! merely incomplete — and that shape IS wasmtime's convention, which is what makes this a
//! plugin for one tool rather than a guess about WASM in general.

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor,
};
use smol_str::SmolStr;

pub struct WasmtimePlugin;

/// `Host` per interface, `Host<Resource>` per resource type, `{World}Imports` per world —
/// the three shapes `bindgen!` emits, and the only wasmtime knowledge in this crate.
fn is_generated_host_trait(name: &str) -> bool {
    name.starts_with("Host") || name.ends_with("Imports")
}

/// Every member of such a trait is machinery-invoked by construction: a generated host trait
/// exists only so the guest can call it. That is why there is no member predicate here where
/// the other convention plugins have one — and why the fact being per-impl-block matters most
/// for this plugin: a host type also implementing `Debug` keeps its `fmt` unmarked.
fn machinery_drives(trait_name: &str) -> bool {
    is_generated_host_trait(trait_name)
}

impl Plugin for WasmtimePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:wasmtime"),
            version: SmolStr::new("1"),
            // Empty: the gate below IS a rule, so prose beside it would be the same fact twice.
            detection: vec![],
            // Nothing to read: the answer is entirely in the symbol table.
            requested_file_access: vec![],
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("wasmtime"))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    fn annotate_symbols(
        &self,
        graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        out: &mut AnnotationSink,
    ) {
        out.mark_machinery_impls(graph, |t, _| machinery_drives(t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_the_dependency() {
        let d = WasmtimePlugin.descriptor();
        assert_eq!(d.id, "kndo:wasmtime");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new("wasmtime"))]
        );
        assert!(WasmtimePlugin.mutates_graph());
        assert!(d.requested_file_access.is_empty());
    }

    #[test]
    fn the_generated_host_traits_are_recognized_by_shape() {
        // kndo's own `plugin_host.rs` implements `bindings::PluginImports for HostViewData`.
        assert!(is_generated_host_trait("Host"));
        assert!(is_generated_host_trait("HostViewData"));
        assert!(is_generated_host_trait("PluginImports"));
        assert!(!is_generated_host_trait("Display"));
        assert!(!is_generated_host_trait("Serialize"));
    }
}

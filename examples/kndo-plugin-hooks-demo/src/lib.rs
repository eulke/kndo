//! kndo-plugin-hooks-demo — the reference external `Plugin` for kndo-plugin-api's `kndo:plugin`
//! v1 ABI (docs/contracts/wasm-abi.md §5). Convention-based rather than framework-specific (a
//! real React/Next.js-style plugin is its own, separate design — RFC 0003 §3's "carried out of
//! M5" item), but it exercises every hook for real, including the bidirectional `list-files`/
//! `symbols-in` host-import queries the other three hooks depend on:
//!
//! - `classify_file`: any path containing `banner` gets reclassified `Generated` origin.
//! - `contribute_roots`: any symbol whose name starts with `root_` becomes a Production root.
//! - `contribute_edges`: any symbol whose name starts with `wire_` gets a `Call` reference from
//!   its own owning file (a real framework plugin would instead wire from wherever its own
//!   convention names — a route table, a DI container entry — but this demo's whole point is
//!   proving the query/contribute round trip works, not modeling one specific framework).
//! - `annotate_symbols`: any symbol whose name starts with `consumed_` is marked externally
//!   consumed.

// Marks the dependency used explicitly — the macro invocation below is a fully-qualified
// path with no `use`, which kndo's own Rust adapter (a static extractor, not a macro
// expander) has no way to trace back to the `wit-bindgen` crate on its own.
use wit_bindgen as _;

wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/plugin.wit",
    world: "plugin",
});

use crate::kndo::plugin::types::*;

struct DemoPlugin;

impl Guest for DemoPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "hooks-demo".to_string(),
            version: "1".to_string(),
            detection: vec!["a *.trigger file anywhere in the project".to_string()],
            requested_file_access: Vec::new(),
            // Exercises RFC 0003 §4's global-install activation path: a project only picks
            // this plugin up from a global directory if it actually contains a `*.trigger`
            // file — proven by kndo/tests/global_plugin_activation.rs.
            activation: vec![ActivationRule::FileExists("*.trigger".to_string())],
            // Deliberately empty (RFC 0015 §3): declaring one here would make every e2e run
            // report a missing coordinate — dependency semantics are covered by the native
            // fixpoint tests plus the WIT round-trip assertion in plugin_compliance.rs.
            dependencies: Vec::new(),
        }
    }

    fn classify_file(path: String, current: FileClass) -> Option<FileClass> {
        if path.contains("banner") {
            Some(FileClass {
                role: current.role,
                origin: FileOrigin::Generated,
            })
        } else {
            None
        }
    }

    fn contribute_roots() -> Vec<ContributedRoot> {
        let mut roots = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                if symbol.name.starts_with("root_") {
                    roots.push(ContributedRoot {
                        target: PluginTarget {
                            path: file.path.clone(),
                            symbol: Some(symbol.name.clone()),
                        },
                        kind: RootKind::Production,
                        confidence: Confidence::Probable,
                    });
                }
            }
        }
        roots
    }

    fn contribute_edges() -> Vec<ContributedEdge> {
        let mut edges = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                if symbol.name.starts_with("wire_") {
                    edges.push(ContributedEdge {
                        from: PluginTarget {
                            path: file.path.clone(),
                            symbol: None,
                        },
                        to: PluginTarget {
                            path: file.path.clone(),
                            symbol: Some(symbol.name.clone()),
                        },
                        kind: RefKind::Call,
                        confidence: Confidence::Probable,
                    });
                }
            }
        }
        edges
    }

    fn annotate_symbols() -> Vec<PluginTarget> {
        let mut targets = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                if symbol.name.starts_with("consumed_") {
                    targets.push(PluginTarget {
                        path: file.path.clone(),
                        symbol: Some(symbol.name.clone()),
                    });
                }
            }
        }
        targets
    }
}

export!(DemoPlugin);

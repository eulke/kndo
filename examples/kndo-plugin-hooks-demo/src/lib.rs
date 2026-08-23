//! kndo-plugin-hooks-demo — the reference external `Plugin` for kndo-plugin-api's `kndo:plugin`
//! v1 ABI. Convention-based rather than framework-specific (a
//! real React/Next.js-style plugin is its own, separate
//! design), but it exercises every hook for real, including the bidirectional `list-files`/
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
//! - `contribute_roots` also exercises the content channel: any symbol whose name
//!   starts with `content_` is rooted only if `read-file("content.demo")` returns exactly
//!   `b"promote"` — proving the host-mediated read reaches a real guest computation, not just
//!   that the WIT world type-checks.
//! - The read surface, exercised for real: `linked_` symbols are rooted only when
//!   `importers-of` reports their file has at least one importer, and `sited_` symbols only
//!   when `call-sites-in` shows a `use.site("promote")` call site in their file — proving the
//!   edge and call-site queries reach real guest computations.
//! - The round lifecycle, made observable through two statics: `staged_` symbols
//!   are wired by `contribute_edges` only when `contribute_roots` already ran in this same
//!   instance (state persists across one round's hooks), and `fresh_` symbols are rooted only
//!   on the instance's FIRST `contribute_roots` call (so a leaked instance from a previous
//!   round observably stops rooting them — every fresh round must root them again).

// Marks the dependency used explicitly — the macro invocation below is a fully-qualified
// path with no `use`, which kndo's own Rust adapter (a static extractor, not a macro
// expander) has no way to trace back to the `wit-bindgen` crate on its own.
use wit_bindgen as _;

// Targets the findings-capable world — the superset of `plugin` (the pinned compat
// components stay on `plugin`, proving the host's fallback).
wit_bindgen::generate!({
    path: "../../crates/kndo-plugin-api/wit/plugin.wit",
    world: "plugin-findings",
});

use crate::kndo::plugin::types::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Observable round state (a WASM guest is single-threaded; atomics are just
/// the no-`unsafe` way to hold mutable statics). Both reset with the instance — which is
/// exactly what the compliance suite asserts.
static ROOTS_RAN_THIS_INSTANCE: AtomicBool = AtomicBool::new(false);
static ROOTS_CALLS_THIS_INSTANCE: AtomicU32 = AtomicU32::new(0);

struct DemoPlugin;

impl Guest for DemoPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "hooks-demo".to_string(),
            version: "1".to_string(),
            detection: vec!["a *.trigger file anywhere in the project".to_string()],
            // Declares access to a companion file outside the language graph —
            // contribute_roots below reads it through the host-mediated channel.
            requested_file_access: vec!["content.demo".to_string()],
            // Exercises the global-install activation path: a project only picks
            // this plugin up from a global directory if it actually contains a `*.trigger`
            // file — proven by kndo/tests/global_plugin_activation.rs.
            activation: vec![ActivationRule::FileExists("*.trigger".to_string())],
            // Deliberately empty: declaring one here would make every e2e run
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
        ROOTS_RAN_THIS_INSTANCE.store(true, Ordering::Relaxed);
        let first_call_on_this_instance =
            ROOTS_CALLS_THIS_INSTANCE.fetch_add(1, Ordering::Relaxed) == 0;
        let content_promoted = read_file("content.demo").as_deref() == Some(b"promote".as_slice());
        let mut roots = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                let should_root = symbol.name.starts_with("root_")
                    || (content_promoted && symbol.name.starts_with("content_"))
                    || (first_call_on_this_instance && symbol.name.starts_with("fresh_"))
                    || (symbol.name.starts_with("linked_")
                        && !importers_of(&file.path).is_empty())
                    || (symbol.name.starts_with("sited_")
                        && call_sites_in(&file.path)
                            .iter()
                            .any(|c| c.callee == "use.site" && c.literal == "promote"));
                if should_root {
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
        // `staged_` wiring depends on state `contribute_roots` set in THIS instance — under
        // an instance-per-hook model this would be observably false here.
        let roots_already_ran = ROOTS_RAN_THIS_INSTANCE.load(Ordering::Relaxed);
        let mut edges = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                if symbol.name.starts_with("wire_")
                    || (roots_already_ran && symbol.name.starts_with("staged_"))
                {
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

    /// One declared rule, exercised end to end by the compliance suite.
    fn rules() -> Vec<RuleDescriptor> {
        vec![RuleDescriptor {
            name: "flag-marked".to_string(),
            description: "symbols named finding_* are flagged by this demo rule".to_string(),
            severity: FindingSeverity::Warning,
        }]
    }

    fn contribute_findings() -> Vec<ContributedFinding> {
        let mut findings = Vec::new();
        for file in list_files() {
            for symbol in symbols_in(&file.path) {
                if symbol.name.starts_with("finding_") {
                    findings.push(ContributedFinding {
                        rule: "flag-marked".to_string(),
                        target: PluginTarget {
                            path: file.path.clone(),
                            symbol: Some(symbol.name.clone()),
                        },
                        confidence: Confidence::Probable,
                        message: format!("symbol `{}` carries the demo marker", symbol.name),
                    });
                }
            }
        }
        findings
    }
}

export!(DemoPlugin);

//! `kndo:nextjs` — the built-in Next.js conventions plugin. Normative spec:
//! docs/plugins/nextjs.md (RFC 0015 §6 phase 4).
//!
//! Next.js is a file-system router: `pages/**`/`app/**` files are loaded by path convention
//! and well-known exports are called by name, none of which appears as an import. Rooting the
//! file alone is not enough — reachability has no file→contained-symbols edge (spec §1) — so
//! every convention file contributes a file root *and* roots for its framework-consumed
//! exports, plus externally-consumed annotations for the `internal-only`/`private-type-leak`
//! exemption (RFC 0005 §7).

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, GraphView, Plugin, PluginDescriptor, PluginTarget, RootSink,
};
use kndo_core::vocab::{Confidence, FileRole, RootKind};
use smol_str::SmolStr;

mod conventions;

pub struct NextjsPlugin;

impl Plugin for NextjsPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:nextjs"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new(
                "a package.json under the project root depends on next",
            )],
            requested_file_access: vec![],
            // One rule, deliberately (spec §2): every real Next project declares `next`
            // somewhere, and the manifest scan is gitignore-aware and monorepo-wide. A
            // recursive FileExists("**/next.config.*") would raw-glob through node_modules on
            // every run — cost and hazard for a signal the manifest rule already carries.
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("next"))],
            dependencies: vec![],
        }
    }

    fn contribute_roots(&self, graph: &GraphView<'_>, out: &mut RootSink) {
        for (file, tier) in convention_files(graph) {
            // Being routed/loaded by the framework is definitional once the directory is a
            // convention directory — Certain, not a heuristic (spec §4).
            out.add(
                PluginTarget::file(file.path.clone()),
                RootKind::Production,
                Confidence::Certain,
            );
            for symbol in framework_visible_exports(graph, file) {
                let confidence = if tier.certain_exports().contains(&symbol.name.as_str()) {
                    Confidence::Certain
                } else {
                    // The page component is a *default* export whose local name is arbitrary
                    // and `SymbolNode` carries no is-default flag — rooting every export is
                    // the only way to guarantee it's covered (spec §4.1's documented
                    // over-approximation, false-negative direction only).
                    Confidence::Probable
                };
                out.add(
                    PluginTarget::symbol(file.path.clone(), symbol.name.clone()),
                    RootKind::Production,
                    confidence,
                );
            }
        }
    }

    fn annotate_symbols(&self, graph: &GraphView<'_>, out: &mut AnnotationSink) {
        // The framework is the external consumer of every rooted export (spec §4.4) — a
        // page's exported props type must never be told to narrow its visibility.
        for (file, _tier) in convention_files(graph) {
            for symbol in framework_visible_exports(graph, file) {
                out.mark_externally_consumed(file.path.clone(), symbol.name.clone());
            }
        }
    }
}

/// Every claimed production `js-ts` file that lands in a convention tier (spec §3/§4) — the
/// shared iteration both hooks classify against.
fn convention_files<'a>(
    graph: &'a GraphView<'_>,
) -> Vec<(&'a kndo_core::graph::FileNode, conventions::Tier)> {
    let app_roots = conventions::app_roots(graph.files().map(|f| f.path.0.as_str()));
    graph
        .files()
        .filter(|f| is_production_js(f))
        .filter_map(|f| conventions::classify(f.path.0.as_str(), &app_roots).map(|t| (f, t)))
        .collect()
}

/// Production role only (spec §4): rooting a stray `pages/index.test.tsx` as production would
/// let production-reachability mask `test-only` findings — the role machinery already covers
/// test files. Unclaimed files (`.mdx` pages, images) have no language and are skipped.
fn is_production_js(file: &kndo_core::graph::FileNode) -> bool {
    file.language.as_deref() == Some("js-ts")
        && file
            .class
            .as_ref()
            .is_some_and(|c| c.role == FileRole::Production)
}

/// Exported top-level symbols — members (`member_of` set) are never rooted or annotated:
/// Next's conventions are module-level (spec §4.4).
fn framework_visible_exports<'a>(
    graph: &'a GraphView<'_>,
    file: &kndo_core::graph::FileNode,
) -> impl Iterator<Item = &'a kndo_core::graph::SymbolNode> + 'a {
    graph
        .symbols_in(&file.path)
        .filter(|s| s.exported && s.member_of.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_next() {
        let d = NextjsPlugin.descriptor();
        assert_eq!(d.id, "kndo:nextjs");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new("next"))]
        );
        assert!(d.dependencies.is_empty());
    }

    #[test]
    fn the_plugin_mutates_the_graph() {
        // Trait default — asserted because it is load-bearing: this plugin exists to
        // contribute roots, so it must cost the cache bypass when (and only when) active.
        assert!(NextjsPlugin.mutates_graph());
    }
}

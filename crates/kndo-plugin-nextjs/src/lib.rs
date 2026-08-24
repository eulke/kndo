//! `kndo:nextjs` — the built-in Next.js conventions plugin.
//!
//! Next.js is a file-system router: `pages/**`/`app/**` files are loaded by path convention
//! and well-known exports are called by name, none of which appears as an import. Rooting the
//! file alone is not enough — reachability has no file→contained-symbols edge — so
//! every convention file contributes a file root *and* roots for its framework-consumed
//! exports, plus externally-consumed annotations for the `internal-only`/`private-type-leak`
//! exemption.

use std::collections::BTreeMap;

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor, PluginTarget,
    RootSink,
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
            // Every app root's own next.config.* is read through the content channel, to
            // statically read `pageExtensions`. This is a distinct mechanism
            // from `activation` below — content-channel globs match only already-discovered,
            // gitignore-filtered paths (no disk walk of their own), so unlike an
            // ActivationRule::FileExists glob, recursion here never touches node_modules.
            requested_file_access: vec![SmolStr::new("**/next.config.*")],
            // One rule, deliberately: every real Next project declares `next`
            // somewhere, and the manifest scan is gitignore-aware and monorepo-wide. A
            // recursive FileExists("**/next.config.*") would raw-glob through node_modules on
            // every run — cost and hazard for a signal the manifest rule already carries.
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("next"))],
            dependencies: vec![],
        }
    }

    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        for (file, tier) in convention_files(graph, content) {
            // Being routed/loaded by the framework is definitional once the directory is a
            // convention directory — Certain, not a heuristic.
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
                    // the only way to guarantee it's covered (a deliberate
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

    fn annotate_symbols(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut AnnotationSink,
    ) {
        // The framework is the external consumer of every rooted export — a
        // page's exported props type must never be told to narrow its visibility.
        for (file, _tier) in convention_files(graph, content) {
            for symbol in framework_visible_exports(graph, file) {
                out.mark_externally_consumed(file.path.clone(), symbol.name.clone());
            }
        }
    }
}

/// Every claimed production `js-ts` file that lands in a convention tier — the
/// shared iteration both hooks classify against.
fn convention_files<'a>(
    graph: &'a GraphView<'_>,
    content: &ContentView<'_>,
) -> Vec<(&'a kndo_core::graph::FileNode, conventions::Tier)> {
    let app_roots = conventions::app_roots(graph.files().map(|f| f.path.0.as_str()));
    let page_extensions = page_extensions_by_root(&app_roots, content);
    graph
        .files()
        .filter(|f| is_production_js(f))
        .filter_map(|f| {
            conventions::classify(f.path.0.as_str(), &app_roots, &page_extensions).map(|t| (f, t))
        })
        .collect()
}

/// Each app root's own `next.config.*`, read through the content channel and statically
/// scanned for a `pageExtensions` array. A root with no config file, an unparsable one, or a
/// dynamic `pageExtensions` value simply gets no entry — `conventions::classify` treats a
/// missing entry as "accept the default extensions, unfiltered" for that root, never a guess.
fn page_extensions_by_root(
    app_roots: &[String],
    content: &ContentView<'_>,
) -> BTreeMap<String, Vec<String>> {
    let mut by_root = BTreeMap::new();
    for path in content.matching_paths() {
        let Some(root) = config_root(path.0.as_str(), app_roots) else {
            continue;
        };
        let Some(bytes) = content.read(path) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(extensions) = conventions::static_page_extensions(text) {
            by_root.insert(root, extensions);
        }
    }
    by_root
}

/// Which app root, if any, a matched `next.config.*` path sits directly in — mirrors how
/// `conventions::app_roots` itself derives roots from these same files.
fn config_root(path: &str, app_roots: &[String]) -> Option<String> {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    app_roots.iter().find(|root| root.as_str() == dir).cloned()
}

/// Production role only: rooting a stray `pages/index.test.tsx` as production would
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
/// Next's conventions are module-level.
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

//! `kndo:express` — the built-in Express conventions plugin. Normative spec:
//! docs/plugins/express.md (RFC 0015 §6 phase 4).
//!
//! Express is imperative — routes are registered by ordinary code the language adapter
//! already follows — so this plugin covers exactly the one convention that hides real code
//! from import analysis: the entry file is *launched by a script* (`node ./bin/www`,
//! `node server.js`), never imported. `bin/www` itself is extension-less and unclaimed, so
//! the expressible fix is rooting the conventional claimed entries it (or `node` directly)
//! launches. `views/**` templates are unclaimed too and stay out of scope (spec §1).

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor, PluginTarget,
    RootSink,
};
use kndo_core::vocab::{Confidence, FileRole, RootKind};
use smol_str::SmolStr;

mod conventions;

pub struct ExpressPlugin;

impl Plugin for ExpressPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:express"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new(
                "a package.json under the project root depends on express",
            )],
            // RFC 0016 §5: every app root's own package.json, read through the content
            // channel to derive the true entry from "main"/"scripts" (§3's documented gap) —
            // in-memory glob against already-discovered, gitignore-filtered paths, not a disk
            // walk, so (unlike a raw ActivationRule::FileExists glob) this never touches
            // node_modules regardless of recursion.
            requested_file_access: vec![SmolStr::new("**/package.json")],
            // Single rule (spec §2): express is always a declared runtime dependency, never
            // an implicit peer; wrapper frameworks reach this plugin through RFC 0015 §3
            // `dependencies` implication.
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("express"))],
            dependencies: vec![],
        }
    }

    fn contribute_roots(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        for file in entry_files(graph, content) {
            // Probable, not Certain (spec §3): a file named `app.ts` in an Express-using
            // project is *probably* its entry — a convention, unlike `pages/**` which is
            // definitionally routed. Probable still suppresses `unused` at every tier
            // (RFC 0005 §1 rule 4) while keeping the evidence honestly labeled. The
            // manifest-derived candidates get the same tier: `main`/`scripts.start` are a
            // stronger *signal* than the name heuristic, but still describe a convention, not
            // something the language itself guarantees is invoked.
            out.add(
                PluginTarget::file(file.path.clone()),
                RootKind::Production,
                Confidence::Probable,
            );
            for symbol in exported_top_level(graph, file) {
                out.add(
                    PluginTarget::symbol(file.path.clone(), symbol.name.clone()),
                    RootKind::Production,
                    Confidence::Probable,
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
        // The generator layout's `app.js` exports the app object solely for the unclaimed
        // `bin/www` to require — an external consumer the graph cannot see (spec §3).
        for file in entry_files(graph, content) {
            for symbol in exported_top_level(graph, file) {
                out.mark_externally_consumed(file.path.clone(), symbol.name.clone());
            }
        }
    }
}

/// Every claimed production `js-ts` file that is a conventional entry — by name (§3's original
/// heuristic) or by its app root's own `package.json` `"main"`/`"scripts"` (RFC 0016 §5's
/// content-channel upgrade) — deduplicated, in `graph.files()` order.
fn entry_files<'a>(
    graph: &'a GraphView<'_>,
    content: &ContentView<'_>,
) -> Vec<&'a kndo_core::graph::FileNode> {
    let app_roots = conventions::app_roots(graph.files().map(|f| f.path.0.as_str()));
    let manifest_candidates = manifest_entries(&app_roots, content);
    graph
        .files()
        .filter(|f| is_production_js(f))
        .filter(|f| {
            conventions::is_entry(f.path.0.as_str(), &app_roots)
                || manifest_candidates.contains(f.path.0.as_str())
        })
        .collect()
}

/// Reads each app root's `package.json` (already prefetched into the content channel by its
/// declared glob) and collects every candidate path `conventions::manifest_entry_candidates`
/// proposes. A root with no manifest, or one that fails to parse, simply contributes nothing —
/// the name heuristic still covers it.
fn manifest_entries(
    app_roots: &[String],
    content: &ContentView<'_>,
) -> std::collections::BTreeSet<String> {
    let mut candidates = std::collections::BTreeSet::new();
    for root in app_roots {
        let manifest_path = if root.is_empty() {
            "package.json".to_string()
        } else {
            format!("{root}/package.json")
        };
        if let Some(bytes) = content.read(&ProjectPath(SmolStr::new(manifest_path))) {
            candidates.extend(conventions::manifest_entry_candidates(root, &bytes));
        }
    }
    candidates
}

fn is_production_js(file: &kndo_core::graph::FileNode) -> bool {
    file.language.as_deref() == Some("js-ts")
        && file
            .class
            .as_ref()
            .is_some_and(|c| c.role == FileRole::Production)
}

/// Members (`member_of` set) are never rooted or annotated — the entry convention is
/// module-level (spec §3).
fn exported_top_level<'a>(
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
    fn descriptor_claims_the_reserved_namespace_and_gates_on_express() {
        let d = ExpressPlugin.descriptor();
        assert_eq!(d.id, "kndo:express");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new("express"))]
        );
        assert!(d.dependencies.is_empty());
    }

    #[test]
    fn the_plugin_mutates_the_graph() {
        assert!(ExpressPlugin.mutates_graph());
    }
}

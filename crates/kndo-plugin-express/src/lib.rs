//! `kndo:express` — the built-in Express conventions plugin. Normative spec:
//! docs/plugins/express.md (RFC 0015 §6 phase 4).
//!
//! Express is imperative — routes are registered by ordinary code the language adapter
//! already follows — so this plugin covers exactly the one convention that hides real code
//! from import analysis: the entry file is *launched by a script* (`node ./bin/www`,
//! `node server.js`), never imported. `bin/www` itself is extension-less and unclaimed, so
//! the expressible fix is rooting the conventional claimed entries it (or `node` directly)
//! launches. `views/**` templates are unclaimed too and stay out of scope (spec §1).

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, GraphView, Plugin, PluginDescriptor, PluginTarget, RootSink,
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
            requested_file_access: vec![],
            // Single rule (spec §2): express is always a declared runtime dependency, never
            // an implicit peer; wrapper frameworks reach this plugin through RFC 0015 §3
            // `dependencies` implication.
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("express"))],
            dependencies: vec![],
        }
    }

    fn contribute_roots(&self, graph: &GraphView<'_>, out: &mut RootSink) {
        for file in entry_files(graph) {
            // Probable, not Certain (spec §3): a file named `app.ts` in an Express-using
            // project is *probably* its entry — a convention, unlike `pages/**` which is
            // definitionally routed. Probable still suppresses `unused` at every tier
            // (RFC 0005 §1 rule 4) while keeping the evidence honestly labeled.
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

    fn annotate_symbols(&self, graph: &GraphView<'_>, out: &mut AnnotationSink) {
        // The generator layout's `app.js` exports the app object solely for the unclaimed
        // `bin/www` to require — an external consumer the graph cannot see (spec §3).
        for file in entry_files(graph) {
            for symbol in exported_top_level(graph, file) {
                out.mark_externally_consumed(file.path.clone(), symbol.name.clone());
            }
        }
    }
}

/// Every claimed production `js-ts` file matching a conventional entry name at an app root.
fn entry_files<'a>(graph: &'a GraphView<'_>) -> Vec<&'a kndo_core::graph::FileNode> {
    let app_roots = conventions::app_roots(graph.files().map(|f| f.path.0.as_str()));
    graph
        .files()
        .filter(|f| is_production_js(f))
        .filter(|f| conventions::is_entry(f.path.0.as_str(), &app_roots))
        .collect()
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

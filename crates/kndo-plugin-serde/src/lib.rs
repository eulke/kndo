//! `kndo:serde` — the built-in serde conventions plugin.
//!
//! serde's traits are third-party, so the Rust adapter's machinery-trait list deliberately
//! excludes them and the core's implement-dispatch fan-out never
//! fires (the `Implement` reference to an out-of-repo trait resolves to nothing). Yet a
//! hand-written `impl Serialize for T` is invoked exactly like a language hook: serde's
//! machinery calls `serialize` whenever `T` is serialized — never by name from user code.
//! This plugin closes that gap with the framework counterpart of the contract's
//! `implicitly_invoked` flag — `mark_implicitly_invoked`, the machinery-dispatch
//! rule — so a `Serialize` impl on a test-covered type doesn't read as a test blind spot.

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor,
};
use kndo_core::vocab::FileRole;
use smol_str::SmolStr;

mod conventions;

pub struct SerdePlugin;

impl Plugin for SerdePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:serde"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new(
                "a Cargo.toml under the project root depends on serde",
            )],
            // Only files that already declare serde-shaped members are ever read —
            // the glob grants access, the symbol table narrows the actual reads.
            requested_file_access: vec![SmolStr::new("**/*.rs")],
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("serde"))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    fn annotate_symbols(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut AnnotationSink,
    ) {
        for file in graph.files() {
            if file.language.as_deref() != Some("rust") || !has_serde_shaped_members(graph, file) {
                continue;
            }
            let Some(bytes) = content.read(&file.path) else {
                continue;
            };
            let Ok(source) = std::str::from_utf8(&bytes) else {
                continue;
            };
            mark_file(graph, file, source, out);
        }
    }
}

/// One file's marks: the graph's `(owner, member)` pairs filtered through the source's
/// serde impl headers, emitted as qualified member selectors.
fn mark_file(
    graph: &GraphView<'_>,
    file: &kndo_core::graph::FileNode,
    source: &str,
    out: &mut AnnotationSink,
) {
    let members: Vec<(&str, &str)> = graph
        .symbols_in(&file.path)
        .filter_map(|s| s.member_of.as_deref().map(|o| (o, s.name.as_str())))
        .collect();
    for (owner, member) in conventions::machinery_marks(source, &members) {
        out.mark_implicitly_invoked(file.path.clone(), format!("{owner}.{member}"));
    }
}

/// The cheap pre-gate: a file is only ever READ when its symbol table already
/// declares a member serde's machinery could invoke — `serialize`, `deserialize`,
/// `expecting`, `visit_*`. Everything else never touches the content channel.
fn has_serde_shaped_members(graph: &GraphView<'_>, file: &kndo_core::graph::FileNode) -> bool {
    if file
        .class
        .as_ref()
        .is_some_and(|c| c.role != FileRole::Production)
    {
        return false;
    }
    graph
        .symbols_in(&file.path)
        .any(|s| s.member_of.is_some() && serde_shaped(&s.name))
}

/// The member names serde's machinery could invoke — the read gate's whole vocabulary.
fn serde_shaped(name: &str) -> bool {
    matches!(
        name,
        "serialize" | "deserialize" | "deserialize_in_place" | "expecting"
    ) || name.starts_with("visit_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_serde() {
        let d = SerdePlugin.descriptor();
        assert_eq!(d.id, "kndo:serde");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new("serde"))]
        );
    }

    #[test]
    fn the_plugin_mutates_the_graph() {
        assert!(SerdePlugin.mutates_graph());
    }
}

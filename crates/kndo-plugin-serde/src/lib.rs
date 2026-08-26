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
//!
//! Everything below the table is one loop over `SymbolNode::implements`, the fact the adapter
//! already extracted. The plugin holds serde's knowledge and nothing else: it reads no source,
//! parses no grammar, and would be the same shape in any language whose adapter fills that
//! field.

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor,
};
use smol_str::SmolStr;

pub struct SerdePlugin;

/// serde's dispatch surface, curated: each trait, and which of an implementor's members that
/// trait's machinery drives. `DeserializeSeed` drives the same member as `Deserialize` and
/// gets its own row rather than sharing one — a row is one trait, and reading the table should
/// not require knowing which traits happen to agree.
///
/// A same-named local trait over-matches and marks a member serde does not actually drive.
/// That is the safe direction and deliberately so: marking only ever keeps a member alive
/// alongside its owner, never accuses it — and the plugin is gated on the manifest depending
/// on serde in the first place.
/// One row: a trait name, and which of an implementor's members that trait's machinery
/// drives.
type MachineryTrait = (&'static str, fn(&str) -> bool);

const TRAITS: &[MachineryTrait] = &[
    ("Serialize", |m| m == "serialize"),
    ("Deserialize", |m| {
        m == "deserialize" || m == "deserialize_in_place"
    }),
    ("DeserializeSeed", |m| m == "deserialize"),
    ("Visitor", |m| m == "expecting" || m.starts_with("visit_")),
];

/// Does serde's machinery invoke `member` on a type by virtue of it being declared in an
/// `impl` of `trait_name`?
fn machinery_drives(trait_name: &str, member: &str) -> bool {
    TRAITS
        .iter()
        .any(|(t, drives)| *t == trait_name && drives(member))
}

impl Plugin for SerdePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:serde"),
            version: SmolStr::new("1"),
            detection: vec![SmolStr::new(
                "a Cargo.toml under the project root depends on serde",
            )],
            // Nothing to read: the answer is entirely in the symbol table.
            requested_file_access: vec![],
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
        _content: &ContentView<'_>,
        out: &mut AnnotationSink,
    ) {
        out.mark_machinery_impls(graph, machinery_drives);
    }
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
        assert!(d.requested_file_access.is_empty());
    }

    #[test]
    fn the_plugin_mutates_the_graph() {
        assert!(SerdePlugin.mutates_graph());
    }

    #[test]
    fn each_trait_drives_only_its_own_members() {
        assert!(machinery_drives("Serialize", "serialize"));
        assert!(!machinery_drives("Serialize", "helper"));
        assert!(machinery_drives("Deserialize", "deserialize_in_place"));
        assert!(machinery_drives("Visitor", "expecting"));
        assert!(machinery_drives("Visitor", "visit_str"));
        // The reason the fact is per-impl-block rather than per-type: a type implementing
        // both `Serialize` and `Display` files both methods under the same owner, and only
        // the trait each was declared under tells them apart.
        assert!(!machinery_drives("Display", "fmt"));
        assert!(!machinery_drives("Visitor", "serialize"));
    }
}

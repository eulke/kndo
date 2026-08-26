//! `kndo:rkyv` — the built-in rkyv conventions plugin.
//!
//! rkyv's traits are third-party, so the Rust adapter's machinery-trait list excludes them and
//! the core's implement-dispatch fan-out never fires — an `Implement` reference to an
//! out-of-repo trait resolves to nothing. Yet a hand-written `impl ArchiveWith<T> for W` is
//! invoked exactly like a language hook: the code that calls `resolve_with` is what
//! `#[rkyv(with = W)]` expands to, and no reference edge can see generated code
//! (`internal/detection-gaps.md` §2).
//!
//! The `with` adapters are the whole reason this exists. A `#[derive(rkyv::Archive)]` needs no
//! plugin — the derive writes the impl and there are no hand-written members to lose — but a
//! bridge for a type rkyv does not natively support (kndo's own `SmolStrAsString`,
//! `TypeExprAsFlat`) is hand-written and reachable only through the expansion.

use kndo_core::plugin::{
    ActivationRule, AnnotationSink, ContentView, GraphView, Plugin, PluginDescriptor,
};
use smol_str::SmolStr;

pub struct RkyvPlugin;

/// rkyv's dispatch surface, curated. The `*With` trio is the load-bearing part: those are the
/// impls a project hand-writes. The bare trio is here because a hand-written
/// `impl Archive for T` is legitimate rkyv even where the derive is the common path — and
/// each trait drives exactly one member, which is why every row reads the same way.
/// One row: a trait name, and which of an implementor's members that trait's machinery
/// drives.
type MachineryTrait = (&'static str, fn(&str) -> bool);

const TRAITS: &[MachineryTrait] = &[
    ("Archive", |m| m == "resolve"),
    ("Serialize", |m| m == "serialize"),
    ("Deserialize", |m| m == "deserialize"),
    ("ArchiveWith", |m| m == "resolve_with"),
    ("SerializeWith", |m| m == "serialize_with"),
    ("DeserializeWith", |m| m == "deserialize_with"),
];

/// Does rkyv's machinery invoke `member` on a type by virtue of it being declared in an
/// `impl` of `trait_name`?
fn machinery_drives(trait_name: &str, member: &str) -> bool {
    TRAITS
        .iter()
        .any(|(t, drives)| *t == trait_name && drives(member))
}

impl Plugin for RkyvPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:rkyv"),
            version: SmolStr::new("1"),
            // Empty: the gate below IS a rule, so prose beside it would be the same fact twice.
            detection: vec![],
            // Nothing to read: the answer is entirely in the symbol table.
            requested_file_access: vec![],
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("rkyv"))],
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
    fn descriptor_claims_the_reserved_namespace_and_gates_on_rkyv() {
        let d = RkyvPlugin.descriptor();
        assert_eq!(d.id, "kndo:rkyv");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new("rkyv"))]
        );
        assert!(RkyvPlugin.mutates_graph());
        assert!(d.requested_file_access.is_empty());
    }

    #[test]
    fn the_with_adapter_trio_is_what_a_project_hand_writes() {
        // kndo's own `rkyv_support.rs`, which is why this plugin exists.
        assert!(machinery_drives("ArchiveWith", "resolve_with"));
        assert!(machinery_drives("SerializeWith", "serialize_with"));
        assert!(machinery_drives("DeserializeWith", "deserialize_with"));
        // `fmt` belongs to Debug — the Rust adapter's business, not rkyv's — and a member of
        // a `Serialize` impl is serde's or rkyv's depending on nothing this plugin can see,
        // which is why over-matching is bounded to keep-alive.
        assert!(!machinery_drives("Debug", "fmt"));
        assert!(!machinery_drives("ArchiveWith", "serialize_with"));
    }
}

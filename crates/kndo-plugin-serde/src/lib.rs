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
    ActivationRule, AnnotationSink, ContentView, EdgeSink, GraphView, Plugin, PluginDescriptor,
    PluginTarget,
};
use kndo_core::vocab::{Confidence, RefKind};
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

/// The `#[serde(...)]` keys whose value **names an item**, and the whole of what this plugin
/// knows about attribute strings.
///
/// Everything else serde writes between quotes is data — `rename`, `rename_all`, `tag`,
/// `content`, `crate`, `expecting`, `variant_identifier` — and this plugin ignores it, which
/// is precisely the decision no adapter can make. Measured over the attributes the Rust
/// adapter scans: serde alone writes 482 such key-value pairs, 248 of whose values collide
/// with a real declaration in the crate, and only 176 of those sit under a key from this
/// table. An adapter treating every collision as a reference would contribute 72 keep-alive
/// edges in serde to close one real case, and every one of them silences a true finding.
///
/// The list is serde's field- and container-attribute reference, restricted to the keys whose
/// documented value is a path: <https://serde.rs/field-attrs.html>. `remote` is here because
/// its value names a type — it looked like noise in the first measurement and is not, which is
/// exactly the kind of detail only serde's own plugin can be right about.
const PATH_KEYS: &[&str] = &[
    "skip_serializing_if",
    "serialize_with",
    "deserialize_with",
    "with",
    "default",
    "getter",
    "bound",
    "remote",
    "try_from",
    "into",
    "from",
];

impl Plugin for SerdePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:serde"),
            version: SmolStr::new("1"),
            // Empty: the gate below IS a rule, so prose beside it would be the same fact twice.
            detection: vec![],
            // Nothing to read: the answer is entirely in the symbol table.
            requested_file_access: vec![],
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new("serde"))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    /// A serde attribute naming a function is a call site the language cannot see: the
    /// function's only caller is code serde's derive macro generates, which exists in no
    /// source file. `#[serde(skip_serializing_if = "usize_is_zero")]` reads, to plain
    /// reachability, as a struct field with a string on it — and `usize_is_zero` as dead code.
    ///
    /// The edge runs from the **decorated declaration** to the named item, which is the true
    /// direction: the generated impl belongs to the type, so the function runs exactly when
    /// that type is serialized. `Probable`, not `Certain`: the value may name a method rather
    /// than a free function, or a same-named item that is not the one serde resolves.
    ///
    /// Scoped to the declaring file, because a `plugin-target` is `{path, symbol}` and the
    /// attribute knows only a name. A value naming another module or crate
    /// (`crate::util::is_zero`, `chrono::serde::ts_seconds`) therefore resolves to nothing and
    /// is dropped — visible in `kndo doctor`'s dropped count, and the right outcome for a
    /// cross-crate path either way. Reaching the cross-module case needs a project-wide target
    /// form, which is a contract change, not something to approximate here.
    fn contribute_edges(
        &self,
        graph: &GraphView<'_>,
        _content: &ContentView<'_>,
        out: &mut EdgeSink,
    ) {
        for file in graph.files() {
            for attr in graph.attr_strings_in(&file.path) {
                if attr.attribute != "serde" || !PATH_KEYS.contains(&attr.key.as_str()) {
                    continue;
                }
                let Some(owner) = &attr.owner else {
                    continue; // nothing to hang the edge on; contribute nothing
                };
                // The last segment: serde accepts a full path (`crate::util::is_zero`), and
                // the target vocabulary is a declared name. A path into another crate simply
                // resolves to nothing, which is the correct outcome for it.
                let named = attr.literal.rsplit("::").next().unwrap_or_default();
                if named.is_empty() {
                    continue;
                }
                out.add(
                    PluginTarget::symbol(file.path.clone(), owner.clone()),
                    PluginTarget::symbol(file.path.clone(), SmolStr::new(named)),
                    RefKind::Call,
                    Confidence::Probable,
                );
            }
        }
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

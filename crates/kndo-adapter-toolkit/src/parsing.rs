//! Shared tree-sitter machinery — the paved road for first-party adapters: the node-to-span
//! mapping every adapter shares, written once. Grammar-specific parser setup (TS/TSX, JVM XML
//! manifests, …) belongs to the one adapter that needs that grammar, not here — this module
//! stays usable by every language, tree-sitter grammar or not.

use kndo_core::adapter::Span;
use tree_sitter::Node;

/// A tree-sitter node's extent as kndo's 1-based (line, column) `Span` — the one mapping
/// every adapter shares, written once.
pub fn span(node: Node) -> Span {
    Span {
        start: (
            node.start_position().row as u32 + 1,
            node.start_position().column as u32 + 1,
        ),
        end: (
            node.end_position().row as u32 + 1,
            node.end_position().column as u32 + 1,
        ),
    }
}

/// A node's source text, grammar-independent (any tree-sitter tree, any node) — every
/// adapter calls this rather than keeping its own copy.
pub fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

/// The first direct child of `node` with the given grammar `kind` — grammar-independent (any
/// tree-sitter tree, any node kind name), unlike parsing itself.
pub fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    node.children(&mut node.walk()).find(|n| n.kind() == kind)
}

/// The `modifiers` container child a declaration carries, if any — the node every
/// modifier-bearing grammar in this workspace hangs `visibility_modifier`, `member_modifier`
/// and friends off, under that exact name.
pub fn modifiers_node(item: Node) -> Option<Node> {
    find_child(item, "modifiers")
}

/// `item`'s rung on the adapter's own visibility ladder: the level of whichever keyword in
/// `levels` it is written with, or `default` when it writes none.
///
/// `default` is a parameter and not a table entry because writing nothing means something
/// different in every language — Kotlin `public` (3), Swift `internal` (2), Java
/// package-private — and that difference is load-bearing. Silence is not a rung; only the
/// adapter knows what its language reads into it, so only the adapter may state it.
pub fn visibility_level(item: Node, levels: &[(&str, u8)], default: u8) -> u8 {
    let Some(modifiers) = modifiers_node(item) else {
        return default;
    };
    let Some(vis) = find_child(modifiers, "visibility_modifier") else {
        return default;
    };
    vis.children(&mut vis.walk())
        .find_map(|c| levels.iter().find(|(k, _)| *k == c.kind()))
        .map_or(default, |(_, level)| *level)
}

/// Whether `item`'s modifiers contain `keyword` inside a `wrapper` node — the shape a grammar
/// uses when it groups modifiers by family (`member_modifier > override`,
/// `inheritance_modifier > open`) instead of listing them flat.
pub fn has_modifier_wrapper(item: Node, wrapper: &str, keyword: &str) -> bool {
    let Some(modifiers) = modifiers_node(item) else {
        return false;
    };
    let Some(found) = find_child(modifiers, wrapper) else {
        return false;
    };
    found
        .children(&mut found.walk())
        .any(|c| c.kind() == keyword)
}

/// The LAST identifier in a (possibly qualified, possibly generic) type expression:
/// `com.foo.Bar<Baz>` yields `Bar`. Recurses through the grammar's own nested type node
/// (`nested_kind`, `user_type` in every grammar here) so a qualified name resolves to its
/// final segment rather than its package root.
///
/// `identifier_kind` is the leaf a type name is spelled with — `identifier` in Kotlin,
/// `type_identifier` in Swift. Naming it is the whole grammar-specific part.
pub fn last_identifier_text<'a>(
    node: Node,
    src: &'a [u8],
    identifier_kind: &str,
    nested_kind: &str,
) -> Option<&'a str> {
    let mut result = None;
    for child in node.children(&mut node.walk()) {
        let kind = child.kind();
        if kind == identifier_kind {
            result = Some(text(child, src));
        } else if kind == nested_kind {
            result = last_identifier_text(child, src, identifier_kind, nested_kind).or(result);
        }
    }
    result
}

/// The handler a `(kind, handler)` dispatch table assigns to `kind`, or `None`. Generic over
/// the handler type on purpose: the declaration walk and the body walk carry different handler
/// signatures (one takes the adapter's own extraction context), and the LOOKUP is the only
/// thing they share.
pub fn handler_for<'t, H>(kind: &str, table: &'t [(&'static str, H)]) -> Option<&'t H> {
    table
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, handler)| handler)
}

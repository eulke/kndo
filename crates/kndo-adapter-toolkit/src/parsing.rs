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

/// A node's source text, grammar-independent (any tree-sitter tree, any node) — was
/// copy-pasted identically into eight adapters before this moved here.
pub fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

/// The first direct child of `node` with the given grammar `kind` — grammar-independent (any
/// tree-sitter tree, any node kind name), unlike parsing itself.
pub fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    node.children(&mut node.walk()).find(|n| n.kind() == kind)
}

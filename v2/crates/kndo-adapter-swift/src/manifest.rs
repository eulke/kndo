//! `Package.swift` dependency names, parsed with the same grammar extraction
//! uses — SwiftPM requires every argument labeled, so reading `.package(url:)`
//! and `.package(path:)` by label is exact, never a heuristic. The NAME a rule
//! activates on is the identity the ecosystem imports the package by: the last
//! path segment of the url (with any `.git` suffix dropped) or of the local
//! path.

use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

pub fn dependencies(content: &[u8]) -> Vec<SmolStr> {
    let language = tree_sitter_swift::LANGUAGE.into();
    let Some(tree) = tk::parse(&language, content) else {
        return Vec::new();
    };
    let mut out: Vec<SmolStr> = Vec::new();
    collect(tree.root_node(), content, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect(node: Node<'_>, src: &[u8], out: &mut Vec<SmolStr>) {
    if node.kind() == "call_expression" && callee_is_dot_package(node, src) {
        for label in ["url", "path"] {
            if let Some(value) = labeled_argument(node, label, src)
                && let Some(text) = string_text(value, src)
                && let Some(name) = package_name(&text)
            {
                out.push(SmolStr::new(name));
            }
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect(child, src, out);
    }
}

/// `.package(...)` — a prefix-dot implicit-member call, SwiftPM's factory style.
fn callee_is_dot_package(call: Node<'_>, src: &[u8]) -> bool {
    let Some(callee) = call.child(0) else {
        return false;
    };
    callee.kind() == "prefix_expression" && tk::text(callee, src) == ".package"
}

fn labeled_argument<'t>(call: Node<'t>, label: &str, src: &[u8]) -> Option<Node<'t>> {
    let suffix = tk::child_of_kind(call, "call_suffix")?;
    let args = tk::child_of_kind(suffix, "value_arguments")?;
    let mut c = args.walk();
    args.named_children(&mut c)
        .filter(|n| n.kind() == "value_argument")
        .find(|arg| {
            arg.child_by_field_name("name")
                .is_some_and(|n| tk::text(n, src) == label)
        })
        .and_then(|arg| arg.child_by_field_name("value"))
}

fn string_text(value: Node<'_>, src: &[u8]) -> Option<String> {
    let mut found = None;
    tk::walk(value, &mut |n| {
        if n.kind() == "line_str_text" && found.is_none() {
            found = Some(tk::text(n, src).to_string());
        }
    });
    found
}

fn package_name(url_or_path: &str) -> Option<&str> {
    let last = url_or_path.trim_end_matches('/').rsplit('/').next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then_some(name)
}

//! The at-rule walk: `@import`, `@use` and `@forward` reduce to one shape — the
//! string or `url()` argument the statement names — and each becomes one
//! side-effect import. Comments feed suppression; a generated banner is
//! reported as a marker, and what it means is the engine's.

use kndo_contract::adapter::SourceFile;
use kndo_contract::evidence::{EvidenceSink, ImportShape, ImportTarget};
use kndo_contract::vocab::Confidence;
use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

/// The grammar's comment markers: CSS knows the block form, SCSS adds `//`.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"",
    block_doc: b"*",
};

pub(crate) fn extract(file: &SourceFile<'_>, out: &mut EvidenceSink) {
    let language: tree_sitter::Language = if file.path.as_str().ends_with(".scss") {
        tree_sitter_scss::language()
    } else {
        tree_sitter_css::LANGUAGE.into()
    };
    let Some(tree) = tk::parse_reporting(&language, file.content, out) else {
        return;
    };
    // A generated sheet carries its tool's `DO NOT EDIT` banner; the marker
    // is the report, and `Effect::Generated` is what it means.
    tk::mark_generated(file.content, tk::GENERATED_NEEDLES, &["/*", "*", "//"], out);
    let source = file.content;
    tk::walk(tree.root_node(), &mut |n| match n.kind() {
        kind if kind.ends_with("comment") => tk::comment_evidence(n, source, &COMMENT_MARKERS, out),
        "import_statement" | "use_statement" | "forward_statement" => {
            for specifier in specifiers(n, source) {
                if let Some(target) = classify(specifier) {
                    out.import(
                        target,
                        ImportShape::SideEffect,
                        tk::span(n),
                        Confidence::Certain,
                    );
                }
            }
        }
        _ => {}
    });
}

/// The file names one statement carries: its string arguments, and the argument
/// of a `url()` call, quoted or bare. The grammar recovers from the Sass clauses
/// it does not know (`as`, `with`, `show`) by wrapping what follows in error
/// nodes, and a specifier inside one is still the specifier — so the statement's
/// subtree is read, with a `with (…)` configuration map pruned: its strings
/// configure, they do not import.
fn specifiers<'a>(statement: Node<'a>, source: &'a [u8]) -> Vec<&'a str> {
    let mut out = Vec::new();
    collect(statement, source, &mut out);
    out
}

fn collect<'a>(node: Node<'a>, source: &'a [u8], out: &mut Vec<&'a str>) {
    match node.kind() {
        "string_value" => out.push(quoted_content(node, source)),
        "parenthesized_value" => {}
        "call_expression" => {
            if call_name(node, source) == Some("url")
                && let Some(argument) = tk::child_of_kind(node, "arguments").and_then(|args| {
                    let mut c = args.walk();
                    args.named_children(&mut c)
                        .find(|a| matches!(a.kind(), "string_value" | "plain_value"))
                })
            {
                out.push(quoted_content(argument, source));
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect(child, source, out);
            }
        }
    }
}

/// What a quoted value HOLDS, read off the parse. The grammar spells the
/// delimiters as this node's own first and last children, so the content is the
/// span between them and nothing has to decide which characters were quotes: a
/// `plain_value` — `url(x)`, unquoted — has no such children and is its own
/// text, which is the same rule arriving at the same answer.
fn quoted_content<'a>(node: Node<'a>, source: &'a [u8]) -> &'a str {
    let quote = |n: Option<Node<'a>>| n.filter(|n| matches!(tk::text(*n, source), "\"" | "'"));
    match (
        quote(node.child(0)),
        quote(node.child(node.child_count().saturating_sub(1))),
    ) {
        (Some(open), Some(close)) if close.start_byte() > open.end_byte() => {
            std::str::from_utf8(&source[open.end_byte()..close.start_byte()]).unwrap_or_default()
        }
        _ => tk::text(node, source),
    }
}

fn call_name<'a>(call: Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    tk::child_of_kind(call, "function_name").map(|n| tk::text(n, source))
}

/// What a specifier names: a file by path, or a package by name. A scheme or
/// protocol-relative URL leaves the project and names nothing here; a leading
/// `~` is the bundler spelling of a package and drops.
fn classify(specifier: &str) -> Option<ImportTarget> {
    let specifier = specifier.trim();
    if specifier.is_empty()
        || specifier.contains("://")
        || specifier.starts_with("//")
        || specifier.starts_with("data:")
    {
        return None;
    }
    if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
        return Some(ImportTarget::Relative(SmolStr::new(specifier)));
    }
    Some(ImportTarget::Package(SmolStr::new(
        specifier.trim_start_matches('~'),
    )))
}

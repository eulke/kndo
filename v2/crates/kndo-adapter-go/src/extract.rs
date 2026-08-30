//! Extraction: one pass over top-level declarations, the import block, and a pruned
//! full-tree walk for references and comments. Go-specific facts spelled here:
//! capitalization is reach, `package main` + `func main` is the binary entry, and
//! `_test.go` is the test runner's file. The package-as-unit fact lives in
//! [`crate::resolve::unit_mates`], not in evidence — what a file sees without an
//! import depends on the file set, never on this file's bytes. Deliberately
//! undeclared: struct fields, interface methods, and ALL methods — Go's interfaces
//! are structural, so any method may satisfy one and be dispatched without its
//! name ever appearing (`MarshalYAML`, `IsEmpty`); the grammar cannot prove a
//! method dead, and never accuses what it cannot prove.

use kndo_contract::evidence::{
    EvidenceSink, ImportShape, ImportTarget, Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::vocab::{Confidence, ProjectPath, Span};
use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

pub fn extract(
    path: &ProjectPath,
    source: &[u8],
    tree: &tree_sitter::Tree,
    out: &mut EvidenceSink,
) {
    let is_test_file = path.as_str().ends_with("_test.go");
    // The test runner's own convention, visible in the path before any parse.
    if is_test_file {
        out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Certain);
    }

    let root = tree.root_node();
    let package_main = package_name(root, source) == Some("main".to_string());
    // Library mode: every non-internal, non-main, non-test package in a module is
    // importable by other modules — published surface, whether or not anything in
    // this repository imports it. `internal/` is the language's own "not importable
    // from outside" fence, so it earns no root. Probable — convention, not this
    // file's statement.
    if !package_main && !is_test_file && !is_internal(path.as_str()) {
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Probable,
        );
    }
    // The `// Code generated … DO NOT EDIT.` convention: generated code is the
    // generator's business — it declares nothing accusable here, while its imports
    // and references stay real evidence about YOUR code.
    let generated = is_generated(source);

    let mut cursor = root.walk();
    let children: Vec<Node<'_>> = root.named_children(&mut cursor).collect();
    for item in children {
        if generated && item.kind() != "import_declaration" {
            continue;
        }
        match item.kind() {
            "function_declaration" => {
                if let Some(n) = item.child_by_field_name("name") {
                    let name = tk::text(n, source);
                    let id =
                        out.declaration(name, SymbolKind::Function, tk::span(item), reach_of(name));
                    out.metrics(id, function_metrics(item));
                    if package_main && name == "main" {
                        out.root(
                            RootTarget::Declaration(id),
                            RootKind::Production,
                            Confidence::Certain,
                        );
                    }
                    if name == "init" {
                        // The runtime calls every init on package load.
                        out.root(
                            RootTarget::Declaration(id),
                            RootKind::Production,
                            Confidence::Certain,
                        );
                    }
                }
            }
            // Methods are deliberately not declared (see the module doc). No
            // declaration also means no metrics for them: the duplicate analysis
            // reads declared functions only.
            "method_declaration" => {}
            "type_declaration" => {
                let mut c = item.walk();
                for spec in item.named_children(&mut c) {
                    if !matches!(spec.kind(), "type_spec" | "type_alias") {
                        continue;
                    }
                    if let Some(n) = spec.child_by_field_name("name") {
                        let name = tk::text(n, source);
                        out.declaration(name, SymbolKind::Type, tk::span(spec), reach_of(name));
                    }
                }
            }
            "const_declaration" | "var_declaration" => {
                let kind = if item.kind() == "const_declaration" {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Variable
                };
                let mut c = item.walk();
                for spec in item.named_children(&mut c) {
                    if !matches!(spec.kind(), "const_spec" | "var_spec") {
                        continue;
                    }
                    let mut sc = spec.walk();
                    for n in spec.children_by_field_name("name", &mut sc) {
                        let name = tk::text(n, source);
                        // The blank identifier binds nothing nameable.
                        if name == "_" {
                            continue;
                        }
                        out.declaration(name, kind.clone(), tk::span(spec), reach_of(name));
                    }
                }
            }
            "import_declaration" => imports(item, source, out),
            _ => {}
        }
    }

    references_and_comments(root, source, out);
}

fn package_name(root: Node<'_>, source: &[u8]) -> Option<String> {
    let clause = tk::child_of_kind(root, "package_clause")?;
    let ident = tk::child_of_kind(clause, "package_identifier")?;
    Some(tk::text(ident, source).to_string())
}

fn is_internal(path: &str) -> bool {
    path.starts_with("internal/") || path.contains("/internal/")
}

/// golang.org/s/generatedcode: a line `// Code generated … DO NOT EDIT.` before
/// the package clause marks the whole file.
fn is_generated(source: &[u8]) -> bool {
    let head = &source[..source.len().min(2048)];
    std::str::from_utf8(head).is_ok_and(|s| {
        s.lines().take(20).any(|l| {
            let l = l.trim();
            l.starts_with("// Code generated") && l.ends_with("DO NOT EDIT.")
        })
    })
}

/// Capitalization IS the visibility ladder's shared half in Go.
fn reach_of(name: &str) -> Reach {
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Reach::Exported
    } else {
        Reach::Private
    }
}

/// One record per import spec: plain → the package's whole surface under its local
/// name; `_` → side effect; `.` → glob into this file's scope.
fn imports(decl: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    tk::walk(decl, &mut |n| {
        if n.kind() != "import_spec" {
            return;
        }
        let Some(path_node) = n.child_by_field_name("path") else {
            return;
        };
        let path = string_content(path_node, source);
        if path.is_empty() {
            return;
        }
        let shape = match n.child_by_field_name("name") {
            Some(name) if name.kind() == "blank_identifier" => ImportShape::SideEffect,
            Some(name) if name.kind() == "dot" => ImportShape::Glob,
            Some(name) => ImportShape::Namespace {
                local: SmolStr::new(tk::text(name, source)),
            },
            None => ImportShape::Namespace {
                local: SmolStr::new(path.rsplit('/').next().unwrap_or(&path)),
            },
        };
        out.import(
            ImportTarget::Package(SmolStr::new(path)),
            shape,
            tk::span(n),
            Confidence::Certain,
        );
    });
}

fn string_content(node: Node<'_>, source: &[u8]) -> String {
    tk::text(node, source).trim_matches(['"', '`']).to_string()
}

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    walk_pruned(root, &mut |n| {
        if n.kind() == "comment" {
            comment(n, source, out);
            return;
        }
        if !matches!(
            n.kind(),
            "identifier" | "type_identifier" | "field_identifier" | "package_identifier"
        ) {
            return;
        }
        let Some(parent) = n.parent() else {
            return;
        };
        if !is_use(n, parent) {
            return;
        }
        out.reference(tk::text(n, source), classify(n, parent), tk::span(n));
    });
}

/// Depth-first walk that never enters import declarations — those bind and rename,
/// and their paths already became import evidence.
fn walk_pruned(node: Node<'_>, f: &mut dyn FnMut(Node<'_>)) {
    if node.kind() == "import_declaration" {
        return;
    }
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_pruned(child, f);
    }
}

/// Binding and naming positions are not uses; the bias stays keep-alive — only
/// unambiguous declarations and bindings are excluded.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        "function_declaration"
        | "method_declaration"
        | "type_spec"
        | "type_alias"
        | "const_spec"
        | "var_spec"
        | "field_declaration"
        | "method_elem"
        | "parameter_declaration"
        | "variadic_parameter_declaration"
        | "label_name"
        | "package_clause" => parent.child_by_field_name("name") != Some(n),
        _ => true,
    }
}

fn classify(n: Node<'_>, parent: Node<'_>) -> RefKind {
    if parent.kind() == "call_expression" && parent.child_by_field_name("function") == Some(n) {
        return RefKind::Call;
    }
    if parent.kind() == "selector_expression" && parent.child_by_field_name("field") == Some(n) {
        let called = parent.parent().is_some_and(|gp| {
            gp.kind() == "call_expression" && gp.child_by_field_name("function") == Some(parent)
        });
        return if called { RefKind::Call } else { RefKind::Read };
    }
    if n.kind() == "type_identifier" {
        return RefKind::TypeUse;
    }
    RefKind::Read
}

/// Metrics over one function-shaped node, normalized as the other adapters
/// normalize: identifiers, strings and numbers collapse to their kind so Type-2
/// clones fingerprint identically; comments never count.
fn function_metrics(node: Node<'_>) -> kndo_contract::evidence::FunctionMetrics {
    let mut token_hashes: Vec<u64> = Vec::new();
    let mut cyclomatic = 1u32;
    tk::walk(node, &mut |n| {
        match n.kind() {
            "if_statement" | "for_statement" | "expression_case" | "type_case"
            | "communication_case" | "default_case" => {
                cyclomatic += 1;
            }
            "binary_expression" => {
                let mut c = n.walk();
                if n.children(&mut c)
                    .any(|ch| matches!(ch.kind(), "&&" | "||"))
                {
                    cyclomatic += 1;
                }
            }
            _ => {}
        }
        if n.child_count() == 0 {
            let class = match n.kind() {
                "identifier" | "field_identifier" | "type_identifier" | "package_identifier" => {
                    "id"
                }
                "interpreted_string_literal_content" | "raw_string_literal_content" => "str",
                "int_literal" | "float_literal" | "imaginary_literal" => "num",
                "comment" => return,
                other => other,
            };
            token_hashes.push(tk::fnv1a(class.as_bytes()));
        }
    });
    let loc = (node.end_position().row - node.start_position().row + 1) as u32;
    kndo_contract::evidence::FunctionMetrics {
        cyclomatic,
        loc,
        token_count: token_hashes.len() as u32,
        fingerprints: tk::winnow(&token_hashes, 5, 4),
    }
}

fn comment(n: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut span = tk::span(n);
    while span.end > span.start
        && matches!(source.get(span.end as usize - 1), Some(b'\n') | Some(b'\r'))
    {
        span = Span::new(span.start, span.end - 1);
    }
    let bytes = &source[span.start as usize..(span.end as usize).min(source.len())];
    let text = if bytes.starts_with(b"//") {
        Span::new(span.start + 2, span.end)
    } else if bytes.starts_with(b"/*") && bytes.ends_with(b"*/") && bytes.len() >= 4 {
        Span::new(span.start + 2, span.end - 2)
    } else {
        span
    };
    out.comment(span, text);
}

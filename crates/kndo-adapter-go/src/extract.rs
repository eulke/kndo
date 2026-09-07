//! Extraction: one pass over top-level declarations, the import block, and a pruned
//! full-tree walk for references and comments. Go-specific facts spelled here:
//! the package clause NAMES the namespace, capitalization is reach,
//! `package main` + `func main` is the binary entry, and `_test.go` is the test
//! runner's file. Which files co-compile stays in [`crate::resolve::sees`] —
//! that depends on the file set, never on this file's bytes — while the
//! namespace the file declares itself into is its own statement and belongs
//! here. Deliberately undeclared: struct fields, interface methods, and ALL
//! methods — Go's interfaces are structural, so any method may satisfy one and
//! be dispatched without its name ever appearing (`MarshalYAML`, `IsEmpty`);
//! the grammar cannot prove a method dead, and never accuses what it cannot
//! prove.

use kndo_contract::evidence::{
    EvidenceSink, ImportShape, ImportTarget, Reach, RefKind, RootKind, RootTarget, SymbolKind,
};
use kndo_contract::vocab::{Confidence, ProjectPath};
use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

pub fn extract(
    path: &ProjectPath,
    source: &[u8],
    tree: &tree_sitter::Tree,
    out: &mut EvidenceSink,
) {
    // WHAT a `_test.go` file is, the spec declares as a file role and the
    // engine anchors; what this pass needs it for is narrower — which of two
    // binaries a declaration is compiled into, which changes what a root on it
    // means.
    let is_test_file = path.as_str().ends_with("_test.go");

    let root = tree.root_node();
    let package = package_name(root, source);
    let package_main = package.as_deref() == Some("main");
    // The directory ADDRESSES the package — Go's import path is the directory
    // — and the clause NAMES it, so the segments are both: two `util` packages
    // in two directories are two namespaces, and a directory's `foo` and its
    // external `foo_test` are two more. Appending the clause unconditionally
    // is what makes the pair unique: dropping it where it repeats the
    // directory's last segment would merge `a/b` + `package b` with `a` +
    // `package b`. A file whose package clause the parse never produced
    // declares none and stands alone — the keep-alive reading of a broken file.
    if let Some(package) = &package {
        let dir = path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        out.namespace(
            dir.split('/')
                .filter(|c| !c.is_empty())
                .chain(std::iter::once(package.as_str()))
                .map(SmolStr::new),
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
                    let id = out.declaration(
                        name,
                        SymbolKind::Function,
                        tk::span(item),
                        reach_of(name, path.as_str()),
                    );
                    out.metrics(id, function_metrics(item, source));
                    // The runner's own rule: in a `_test.go` file it runs every
                    // `TestXxx`, `BenchmarkXxx`, `ExampleXxx` and `FuzzXxx` by
                    // name — a root on the function, whatever its reach.
                    if is_test_file && runner_entry(name) {
                        out.root(
                            RootTarget::Declaration(id),
                            RootKind::Test,
                            Confidence::Certain,
                        );
                    }
                    if package_main && name == "main" {
                        out.root(
                            RootTarget::Declaration(id),
                            RootKind::Production,
                            Confidence::Certain,
                        );
                    }
                    if name == "init" {
                        // The runtime calls every init on package load — the
                        // load of the binary this file is compiled into, which
                        // for a `_test.go` file is the test binary alone.
                        out.root(
                            RootTarget::Declaration(id),
                            if is_test_file {
                                RootKind::Test
                            } else {
                                RootKind::Production
                            },
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
                        out.declaration(
                            name,
                            SymbolKind::Type,
                            tk::span(spec),
                            reach_of(name, path.as_str()),
                        );
                    }
                }
            }
            "const_declaration" | "var_declaration" => {
                let kind = if item.kind() == "const_declaration" {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Variable
                };
                // `var ( … )` wraps its specs in a `var_spec_list` and
                // `const ( … )` does not — a grammar asymmetry, not a
                // language one. One level of descent through the wrapper and
                // no further: a `var` inside a function literal on the right
                // of this one declares a local, not a package name.
                let mut c = item.walk();
                let specs: Vec<Node<'_>> = item
                    .named_children(&mut c)
                    .flat_map(|child| match child.kind() {
                        "var_spec_list" | "const_spec_list" => {
                            let mut lc = child.walk();
                            child.named_children(&mut lc).collect::<Vec<_>>()
                        }
                        _ => vec![child],
                    })
                    .collect();
                for spec in specs {
                    if !matches!(spec.kind(), "const_spec" | "var_spec") {
                        continue;
                    }
                    let mut sc = spec.walk();
                    for n in spec.children_by_field_name("name", &mut sc) {
                        // `const a, b = 1, 2` labels its separating commas
                        // with the `name` field too; only a named node is a
                        // name.
                        if !n.is_named() {
                            continue;
                        }
                        let name = tk::text(n, source);
                        // The blank identifier binds nothing nameable.
                        if name == "_" {
                            continue;
                        }
                        out.declaration(
                            name,
                            kind.clone(),
                            tk::span(spec),
                            reach_of(name, path.as_str()),
                        );
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

/// `go test` runs a top-level function named `Test`, `Benchmark`, `Example`
/// or `Fuzz` followed by nothing or by a character that is not a lowercase
/// letter — `TestFoo` and `Test_foo` are entries, `Testing` is a function.
fn runner_entry(name: &str) -> bool {
    ["Test", "Benchmark", "Example", "Fuzz"]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| !rest.chars().next().is_some_and(|c| c.is_lowercase()))
        })
}

/// golang.org/s/generatedcode: the toolchain anchors this one at both ends —
/// a line that IS `// Code generated … DO NOT EDIT.`, not a comment that
/// merely mentions the words — and places it before the first non-comment,
/// non-blank text, which is the header the toolkit bounds.
fn is_generated(source: &[u8]) -> bool {
    tk::header_lines(source, &["//", "/*", "*"])
        .any(|l| l.starts_with("// Code generated") && l.ends_with("DO NOT EDIT."))
}

/// Capitalization IS Go's visibility story: uppercase exports, lowercase
/// reaches exactly the package — the namespace this file declared itself into,
/// which has nothing nested under it. The one fence beyond it is the path's:
/// an exported name in a package under an `internal` directory is importable
/// only from the tree rooted at that directory's parent, which is the
/// directory `up` levels above the file's.
fn reach_of(name: &str, path: &str) -> Reach {
    if !name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return Reach::Namespace { up: 0 };
    }
    match internal_fence(path) {
        Some(up) => Reach::Directory { up },
        None => Reach::Exported,
    }
}

/// How many directories above the file's own the `internal` fence sits: the
/// parent of the innermost `internal` element, the most restrictive one. A
/// file at `a/b/internal/c/x.go` is fenced at `a/b`, two above `a/b/internal/c`.
fn internal_fence(path: &str) -> Option<u32> {
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    let components: Vec<&str> = dir.split('/').filter(|c| !c.is_empty()).collect();
    let innermost = components.iter().rposition(|c| *c == "internal")?;
    Some((components.len() - innermost) as u32)
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

/// Go's comment markers, declared where its grammar knowledge lives.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"",
    block_doc: b"*",
};

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    // Import declarations bind and rename; their paths already became import
    // evidence.
    tk::walk_pruned(root, &["import_declaration"], &mut |n| {
        if n.kind() == "comment" {
            tk::comment_evidence(n, source, &COMMENT_MARKERS, out);
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

/// Binding and naming positions are not uses; the bias stays keep-alive — only
/// unambiguous declarations and bindings are excluded. Four shapes:
///
/// 1. a `name` field of its parent — EVERY one, since `var a, b int` and
///    `[T any]` each label more than one;
/// 2. the package clause's own identifier, which the grammar gives no field
///    at all, so a package named like one of its declarations was reading as
///    a use of it;
/// 3. the left side of a `:=`, which binds a local rather than naming
///    anything the package declares (`=` is an assignment: those names must
///    already exist, so writing one IS a use);
/// 4. the receiver's type in a method declaration — Go requires the base type
///    to be declared in the same package, so the receiver is part of the
///    type's own definition, and counting it made any type with a method
///    unaccusable.
///
/// Deliberately still uses: a composite literal's key (`T{Field: v}`), which
/// names a struct field in one reading and a constant in another — the
/// grammar gives one node for both, and the reading that could accuse is the
/// one to avoid; and a selector's operand (`fmt` in `fmt.Println`), which the
/// qualified-reference work resolves rather than drops.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    if parent.kind() == "package_clause" {
        return false;
    }
    let mut c = parent.walk();
    if parent
        .children_by_field_name("name", &mut c)
        .any(|f| f == n)
    {
        return false;
    }
    if binds_a_local(parent) {
        return false;
    }
    !in_receiver_type(n)
}

/// Is this name's list the left side of a `:=` — a short declaration, a range
/// clause or a receive? An `=` in the same place is an assignment, whose names
/// must already exist, so writing one there IS a use.
fn binds_a_local(parent: Node<'_>) -> bool {
    if parent.kind() != "expression_list" {
        return false;
    }
    let Some(clause) = parent.parent() else {
        return false;
    };
    if !matches!(
        clause.kind(),
        "short_var_declaration" | "range_clause" | "receive_statement"
    ) || clause.child_by_field_name("left") != Some(parent)
    {
        return false;
    }
    let mut c = clause.walk();
    clause.children(&mut c).any(|ch| ch.kind() == ":=")
}

/// Is this name the type a method is declared ON? The receiver's type sits
/// under the `type` field of the one `parameter_declaration` of the
/// `parameter_list` a `method_declaration` holds as its `receiver`, possibly
/// behind a pointer or a generic instantiation.
fn in_receiver_type(n: Node<'_>) -> bool {
    let mut cursor = n;
    while let Some(parent) = cursor.parent() {
        match parent.kind() {
            "pointer_type" | "generic_type" | "type_arguments" => cursor = parent,
            "parameter_declaration" => {
                return parent.child_by_field_name("type") == Some(cursor)
                    && parent.parent().is_some_and(|list| {
                        list.parent().is_some_and(|method| {
                            method.kind() == "method_declaration"
                                && method.child_by_field_name("receiver") == Some(list)
                        })
                    });
            }
            _ => return false,
        }
    }
    false
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
const METRICS: tk::MetricsSpec = tk::MetricsSpec {
    // `default_case` is deliberately absent: the catch-the-rest arm is not a
    // new predicate — the shared rule in the spec's contract.
    is_branch: |n, _| match n.kind() {
        "if_statement" | "for_statement" | "expression_case" | "type_case"
        | "communication_case" => true,
        "binary_expression" => {
            let mut c = n.walk();
            n.children(&mut c)
                .any(|ch| matches!(ch.kind(), "&&" | "||"))
        }
        _ => false,
    },
    token_class: |n| match n.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "package_identifier" => Some("id"),
        "interpreted_string_literal_content" | "raw_string_literal_content" => Some("str"),
        "int_literal" | "float_literal" | "imaginary_literal" => Some("num"),
        "comment" => None,
        other => Some(other),
    },
};

fn function_metrics(node: Node<'_>, source: &[u8]) -> kndo_contract::evidence::FunctionMetrics {
    tk::function_metrics(node, &METRICS, source)
}

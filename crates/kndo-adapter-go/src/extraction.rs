//! Go extraction — declarations, references, imports, roots (docs/adapters/go.md §2). A manual
//! tree-sitter-go walk in the same style as `kndo-adapter-js`'s extraction (node/field names
//! verified via `parsing::introspect`, never guessed — RFC 0002 §8's spirit applied before any
//! extraction code was written, not after a bug report).
//!
//! Shape, contrasted with JS: Go's grammar needs no "which of several export forms is this"
//! disambiguation (there is exactly one: capitalize the identifier) and no relative-import
//! resolution pass — so this file is one flat walk plus one reference pass, not JS's several
//! sequential passes over hoisting-sensitive export/import surfaces.

use std::collections::{HashMap, HashSet};

use kndo_core::adapter::{
    Declaration, Diagnostic, DiagnosticLevel, FileFacts, ImportBinding, ImportKind, RawImport,
    RawReference, RawRoot, RawRootTarget, RawSuppression, Span, VisibilityLevel,
};
use kndo_core::vocab::{Confidence, RootKind, SymbolKind};
use smol_str::SmolStr;
use tree_sitter::Node;

pub fn extract(path: &str, content: &[u8]) -> FileFacts {
    let mut out = FileFacts {
        unit: Some(SmolStr::new(kndo_adapter_toolkit::paths::dirname(path))),
        ..FileFacts::default()
    };

    let Some(tree) = crate::parsing::parse(content) else {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "failed to initialize the Go parser".to_string(),
            span: None,
        });
        return out;
    };
    let root = tree.root_node();
    if root.has_error() {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "syntax errors in this file — extraction is best-effort".to_string(),
            span: None,
        });
    }

    let is_main_package = package_name(root, content).as_deref() == Some("main");
    // Root-worthiness by path, computed once (docs/adapters/go.md §0, §4): `internal/` is
    // compiler-enforced, not externally consumed by definition, so its exports aren't
    // auto-promoted; a test file's declarations are never library-mode public API either.
    let is_internal = path.split('/').any(|seg| seg == "internal");
    let is_test_file = path.ends_with("_test.go");
    let promote_exports = !is_internal && !is_test_file;

    let mut aliases: HashMap<SmolStr, usize> = HashMap::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_declaration" => {
                handle_import_declaration(child, content, &mut out, &mut aliases)
            }
            "function_declaration" => {
                handle_function(child, content, is_main_package, promote_exports, &mut out)
            }
            "method_declaration" => handle_method(child, content, promote_exports, &mut out),
            "type_declaration" => {
                handle_type_declaration(child, content, promote_exports, &mut out)
            }
            "const_declaration" => handle_value_declaration(
                child,
                content,
                "const_spec",
                promote_exports,
                SymbolKind::Const,
                &mut out,
            ),
            "var_declaration" => handle_value_declaration(
                child,
                content,
                "var_spec",
                promote_exports,
                SymbolKind::Variable,
                &mut out,
            ),
            _ => {}
        }
    }

    let mut seen_bindings: HashSet<(usize, SmolStr)> = HashSet::new();
    collect_references(
        root,
        content,
        &aliases,
        &mut out.imports,
        &mut seen_bindings,
        &mut out.references,
    );
    collect_suppressions(root, content, &mut out.suppressions);

    out
}

// ---------------------------------------------------------------- shared helpers

fn span(node: Node) -> Span {
    let start = node.start_position();
    let end = node.end_position();
    Span {
        start: (start.row as u32 + 1, start.column as u32 + 1),
        end: (end.row as u32 + 1, end.column as u32 + 1),
    }
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

/// Exported iff the first rune is uppercase — Go's entire visibility rule, no keyword involved
/// (docs/adapters/go.md §1).
fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn visibility(exported: bool) -> VisibilityLevel {
    VisibilityLevel(if exported { 1 } else { 0 })
}

fn package_name(root: Node, src: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_clause" {
            let mut inner = child.walk();
            for grandchild in child.children(&mut inner) {
                if grandchild.kind() == "package_identifier" {
                    return Some(text(grandchild, src).to_string());
                }
            }
        }
    }
    None
}

fn push_declaration(
    out: &mut FileFacts,
    name: &str,
    kind: SymbolKind,
    node_span: Span,
    promote_exports: bool,
) {
    let exported = is_exported(name);
    out.declarations.push(Declaration {
        name: SmolStr::new(name),
        kind,
        span: node_span,
        exported,
        visibility: visibility(exported),
        member_of: None,
    });
    if exported && promote_exports {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(name)),
            confidence: Confidence::Certain,
        });
    }
}

// ---------------------------------------------------------------- declarations

/// `func Name(...) ...` or `func init() {}` / `func main() {}` (roots, docs/adapters/go.md §2 —
/// unconditional regardless of the capitalization rule).
fn handle_function(
    node: Node,
    src: &[u8],
    is_main_package: bool,
    promote_exports: bool,
    out: &mut FileFacts,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    push_declaration(out, name, SymbolKind::Function, span(node), promote_exports);
    if name == "init" || (name == "main" && is_main_package) {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(name)),
            confidence: Confidence::Certain,
        });
    }
}

/// `func (t T) Name(...)` / `func (t *T) Name(...)` — a *member* declaration (RFC 0012 §3):
/// bare name `Name` with `member_of: Some("Type")`, never a `"Type.Name"` string. Ownership as
/// a structured fact is what lets the core resolve a bare method-call reference through the
/// duck-typed fallback instead of missing entirely — before this, an unexported method used
/// only in-package false-positived as `unused:method`.
fn handle_method(node: Node, src: &[u8], promote_exports: bool, out: &mut FileFacts) {
    let (Some(receiver), Some(name_node)) = (
        node.child_by_field_name("receiver"),
        node.child_by_field_name("name"),
    ) else {
        return;
    };
    let Some(receiver_type) = receiver_type_name(receiver, src) else {
        return;
    };
    let method_name = text(name_node, src);
    // Root-worthiness (and suppression addressing) go by the method's *own* exportedness —
    // `is_exported` on a dotted string would key off the receiver type's capitalization
    // instead, which is a different, unrelated fact.
    let exported = is_exported(method_name);
    out.declarations.push(Declaration {
        name: SmolStr::new(method_name),
        kind: SymbolKind::Method,
        span: span(node),
        exported,
        visibility: visibility(exported),
        member_of: Some(SmolStr::new(&receiver_type)),
    });
    if exported && promote_exports {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            // Member root targets use the qualified form (contracts §2, RFC 0012 §3) — the
            // core's bare-name table deliberately never contains members.
            target: RawRootTarget::Declaration(SmolStr::new(format!(
                "{receiver_type}.{method_name}"
            ))),
            confidence: Confidence::Certain,
        });
    }
}

fn receiver_type_name(receiver: Node, src: &[u8]) -> Option<String> {
    // `receiver` is a `parameter_list` with exactly one `parameter_declaration`.
    let mut cursor = receiver.walk();
    for param in receiver.children(&mut cursor) {
        if param.kind() != "parameter_declaration" {
            continue;
        }
        let ty = param.child_by_field_name("type")?;
        let ident = if ty.kind() == "pointer_type" {
            let mut c = ty.walk();
            let found = ty.children(&mut c).find(|n| n.kind() == "type_identifier");
            found?
        } else {
            ty
        };
        if ident.kind() == "type_identifier" {
            return Some(text(ident, src).to_string());
        }
    }
    None
}

fn handle_type_declaration(node: Node, src: &[u8], promote_exports: bool, out: &mut FileFacts) {
    let mut cursor = node.walk();
    for spec in node.children(&mut cursor) {
        let Some(name_node) = spec.child_by_field_name("name") else {
            continue;
        };
        let kind = match spec.kind() {
            "type_alias" => SymbolKind::TypeAlias, // `type X = Y`
            "type_spec" => match spec.child_by_field_name("type").map(|t| t.kind()) {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                // A defined (not aliased) type over another type, e.g. `type Celsius float64` —
                // no existing facet fits (it's neither struct, interface, nor an alias); Go's
                // own vocabulary for this is simply "type", so that's what it stays.
                _ => SymbolKind::Other(SmolStr::new("type")),
            },
            _ => continue,
        };
        push_declaration(out, text(name_node, src), kind, span(spec), promote_exports);
    }
}

fn handle_value_declaration(
    node: Node,
    src: &[u8],
    spec_kind: &str,
    promote_exports: bool,
    symbol_kind: SymbolKind,
    out: &mut FileFacts,
) {
    let mut cursor = node.walk();
    for spec in node.children(&mut cursor) {
        if spec.kind() != spec_kind {
            continue;
        }
        // `const A, B = 1, 2` / `var X, Y int` — a spec can name more than one identifier.
        let mut name_cursor = spec.walk();
        for name_node in spec.children_by_field_name("name", &mut name_cursor) {
            push_declaration(
                out,
                text(name_node, src),
                symbol_kind.clone(),
                span(name_node),
                promote_exports,
            );
        }
    }
}

// ---------------------------------------------------------------- imports

fn handle_import_declaration(
    node: Node,
    src: &[u8],
    out: &mut FileFacts,
    aliases: &mut HashMap<SmolStr, usize>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => handle_import_spec(child, src, out, aliases),
            "import_spec_list" => {
                let mut inner = child.walk();
                for spec in child.children(&mut inner) {
                    if spec.kind() == "import_spec" {
                        handle_import_spec(spec, src, out, aliases);
                    }
                }
            }
            _ => {}
        }
    }
}

fn handle_import_spec(
    node: Node,
    src: &[u8],
    out: &mut FileFacts,
    aliases: &mut HashMap<SmolStr, usize>,
) {
    let Some(path_node) = node.child_by_field_name("path") else {
        return;
    };
    // `interpreted_string_literal`'s own text still carries the surrounding quotes.
    let specifier = text(path_node, src).trim_matches('"').to_string();
    if specifier.is_empty() {
        return;
    }

    let name_node = node.child_by_field_name("name");
    let (side_effect_only, opaque_namespace_use, alias) = match name_node.map(|n| n.kind()) {
        Some("blank_identifier") => (true, false, None),
        Some("dot") => (false, true, None),
        Some("package_identifier") => (false, false, name_node.map(|n| SmolStr::new(text(n, src)))),
        _ => (
            false,
            false,
            Some(SmolStr::new(default_import_alias(&specifier))),
        ),
    };

    let index = out.imports.len();
    out.imports.push(RawImport {
        specifier: SmolStr::new(&specifier),
        kind: ImportKind::Package, // Go has no relative imports (docs/adapters/go.md §0)
        span: span(node),
        side_effect_only,
        type_only: false,
        confidence: Confidence::Certain, // no dynamic import surface in Go
        bindings: Vec::new(),            // filled in by collect_references (dotted local access)
        reexported: false,
        opaque_namespace_use,
    });
    if let Some(alias) = alias {
        aliases.insert(alias, index);
    }
}

/// The package name Go code refers to when an import has no explicit alias: the specifier's
/// last path segment. A real, documented imprecision (docs/adapters/go.md §3) — the target
/// package's *declared* name can differ (`gopkg.in/yaml.v3` imports as `yaml`), which a
/// single-file extraction pass has no way to know without reading the target.
fn default_import_alias(specifier: &str) -> String {
    specifier
        .rsplit('/')
        .next()
        .unwrap_or(specifier)
        .to_string()
}

// ---------------------------------------------------------------- references

/// Declaration-site name fields to exclude from the reference walk, mirroring
/// `kndo-adapter-js`'s `skip_field` trick — a name being *declared* is not a use of anything.
fn skip_field_for(kind: &str) -> Option<&'static str> {
    match kind {
        "function_declaration"
        | "method_declaration"
        | "type_spec"
        | "type_alias"
        | "parameter_declaration"
        | "variadic_parameter_declaration"
        | "field_declaration" => Some("name"),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_references(
    node: Node,
    src: &[u8],
    aliases: &HashMap<SmolStr, usize>,
    imports: &mut [RawImport],
    seen_bindings: &mut HashSet<(usize, SmolStr)>,
    out: &mut Vec<RawReference>,
) {
    // Import specifiers are pure name-binding syntax — nothing inside is a reference (mirrors
    // js-ts's identical stance on `import_statement`).
    if matches!(node.kind(), "import_declaration" | "package_clause") {
        return;
    }

    // `pkg.Name` where `pkg` is a known import alias: the qualified access JS's `ns.foo`
    // handling already established a convention for (contracts §2's `ImportBinding` shape) —
    // one dotted binding + a same-named reference, resolved precisely instead of guessed.
    if node.kind() == "selector_expression" {
        if let (Some(operand), Some(field)) = (
            node.child_by_field_name("operand"),
            node.child_by_field_name("field"),
        ) {
            if operand.kind() == "identifier" {
                if let Some(&import_idx) = aliases.get(text(operand, src)) {
                    let dotted = format!("{}.{}", text(operand, src), text(field, src));
                    let dotted = SmolStr::new(&dotted);
                    if seen_bindings.insert((import_idx, dotted.clone())) {
                        imports[import_idx].bindings.push(ImportBinding {
                            local: dotted.clone(),
                            imported: Some(SmolStr::new(text(field, src))),
                        });
                    }
                    out.push(RawReference {
                        name: dotted,
                        scope_context: None,
                        span: span(node),
                    });
                    return; // operand/field fully handled — don't also walk them generically
                }
            }
        }
    }

    // `const`/`var` specs can name more than one identifier (`const A, B = 1, 2`) — skip every
    // `name`-field child, not just the first, before recursing into the rest (the values).
    if matches!(node.kind(), "const_spec" | "var_spec") {
        let mut skip_ids = HashSet::new();
        let mut name_cursor = node.walk();
        for n in node.children_by_field_name("name", &mut name_cursor) {
            skip_ids.insert(n.id());
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if skip_ids.contains(&child.id()) {
                continue;
            }
            collect_references(child, src, aliases, imports, seen_bindings, out);
        }
        return;
    }

    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier"
    ) {
        out.push(RawReference {
            name: SmolStr::new(text(node, src)),
            scope_context: None,
            span: span(node),
        });
    }

    // Declaration-site name fields are skipped by *not recursing into them* — a name being
    // declared is a leaf identifier itself, so excluding it from the push above (rather than
    // from the descent below) would never fire: the check has to gate whichever child the
    // parent names as its "name" field, evaluated here at the parent, not re-derived once the
    // walk has already arrived at that child with no memory of which field it came from.
    let skip_id = skip_field_for(node.kind())
        .and_then(|f| node.child_by_field_name(f))
        .map(|n| n.id());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if Some(child.id()) == skip_id {
            continue;
        }
        collect_references(child, src, aliases, imports, seen_bindings, out);
    }
}

// ---------------------------------------------------------------- suppressions

fn collect_suppressions(node: Node, src: &[u8], out: &mut Vec<RawSuppression>) {
    if node.kind() == "comment" {
        if let Some(pragma) =
            kndo_adapter_toolkit::suppression::parse_suppression_pragma(text(node, src))
        {
            out.push(RawSuppression {
                span: span(node),
                category: pragma.category,
                subject: pragma.subject,
                reason: pragma.reason,
                scope: pragma.scope,
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_suppressions(child, src, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl<'a>(facts: &'a FileFacts, name: &str) -> &'a Declaration {
        facts
            .declarations
            .iter()
            .find(|d| d.name.as_str() == name)
            .unwrap_or_else(|| panic!("no declaration named {name:?} in {:?}", facts.declarations))
    }

    #[test]
    fn unit_is_the_file_s_own_directory() {
        let facts = extract("pkg/sub/a.go", b"package sub\n");
        assert_eq!(facts.unit.as_deref(), Some("pkg/sub"));
        let facts = extract("a.go", b"package main\n");
        assert_eq!(facts.unit.as_deref(), Some(""));
    }

    #[test]
    fn plain_function_is_a_declaration() {
        let facts = extract("a.go", b"package p\n\nfunc Helper() int { return 1 }\n");
        let d = decl(&facts, "Helper");
        assert_eq!(d.kind, SymbolKind::Function);
        assert!(d.exported);
        assert_eq!(d.visibility, VisibilityLevel(1));
    }

    #[test]
    fn unexported_function_has_visibility_zero() {
        let facts = extract("a.go", b"package p\n\nfunc helper() {}\n");
        let d = decl(&facts, "helper");
        assert!(!d.exported);
        assert_eq!(d.visibility, VisibilityLevel(0));
    }

    #[test]
    fn methods_are_members_of_their_receiver_type() {
        // Bare name + member_of (RFC 0012 §3) — never a dotted string. Value- and
        // pointer-receiver methods share the owner exactly as Go's method-set rules do.
        let src =
            b"package p\n\ntype T struct{}\nfunc (t T) Value() {}\nfunc (t *T) Pointer() {}\n";
        let facts = extract("a.go", src);
        let value = facts
            .declarations
            .iter()
            .find(|d| d.name.as_str() == "Value")
            .unwrap();
        assert_eq!(value.member_of.as_deref(), Some("T"));
        let pointer = facts
            .declarations
            .iter()
            .find(|d| d.name.as_str() == "Pointer")
            .unwrap();
        assert_eq!(pointer.member_of.as_deref(), Some("T"));
        // Exported methods root themselves by the qualified form (member root targets,
        // contracts §2) — the core's bare-name table deliberately never holds members.
        assert!(facts.roots.iter().any(|r| matches!(
            &r.target, RawRootTarget::Declaration(n) if n.as_str() == "T.Value"
        )));
    }

    #[test]
    fn struct_interface_alias_and_defined_type_get_the_right_kind() {
        let src = br#"
package p

type S struct { F int }
type I interface { M() }
type A = string
type D int
"#;
        let facts = extract("a.go", src);
        assert_eq!(decl(&facts, "S").kind, SymbolKind::Struct);
        assert_eq!(decl(&facts, "I").kind, SymbolKind::Interface);
        assert_eq!(decl(&facts, "A").kind, SymbolKind::TypeAlias);
        assert_eq!(
            decl(&facts, "D").kind,
            SymbolKind::Other(SmolStr::new("type"))
        );
    }

    #[test]
    fn multi_name_const_and_var_specs_each_become_a_declaration() {
        let facts = extract("a.go", b"package p\n\nconst A, B = 1, 2\nvar X, Y int\n");
        assert_eq!(decl(&facts, "A").kind, SymbolKind::Const);
        assert_eq!(decl(&facts, "B").kind, SymbolKind::Const);
        assert_eq!(decl(&facts, "X").kind, SymbolKind::Variable);
        assert_eq!(decl(&facts, "Y").kind, SymbolKind::Variable);
    }

    #[test]
    fn main_func_in_package_main_is_a_root() {
        let facts = extract("main.go", b"package main\n\nfunc main() {}\n");
        assert!(facts.roots.iter().any(|r| matches!(
            &r.target, RawRootTarget::Declaration(n) if n.as_str() == "main"
        ) && r.kind == RootKind::Production));
    }

    #[test]
    fn main_func_outside_package_main_is_not_a_root() {
        let facts = extract("a.go", b"package p\n\nfunc main() {}\n");
        assert!(!facts
            .roots
            .iter()
            .any(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "main")));
    }

    #[test]
    fn init_is_always_a_root_regardless_of_package() {
        let facts = extract("a.go", b"package p\n\nfunc init() {}\n");
        assert!(facts
            .roots
            .iter()
            .any(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "init")));
    }

    #[test]
    fn exported_top_level_declaration_is_promoted_to_a_root() {
        let facts = extract(
            "a.go",
            b"package p\n\nfunc Exported() {}\nfunc unexported() {}\n",
        );
        assert!(facts.roots.iter().any(
            |r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "Exported")
        ));
        assert!(!facts.roots.iter().any(
            |r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "unexported")
        ));
    }

    #[test]
    fn internal_package_exports_are_not_promoted_to_roots() {
        let facts = extract(
            "pkg/internal/util/a.go",
            b"package util\n\nfunc Helper() {}\n",
        );
        assert!(!facts
            .roots
            .iter()
            .any(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "Helper")));
        // Still a declaration, still correctly "exported" — just not auto-promoted.
        assert!(decl(&facts, "Helper").exported);
    }

    #[test]
    fn test_file_exports_are_not_promoted_to_roots() {
        let facts = extract("a_test.go", b"package p\n\nfunc ExportedHelper() {}\n");
        assert!(!facts.roots.iter().any(
            |r| matches!(&r.target, RawRootTarget::Declaration(n) if n.as_str() == "ExportedHelper")
        ));
    }

    #[test]
    fn plain_import_binds_the_last_path_segment_as_the_default_alias() {
        let src = b"package p\n\nimport \"encoding/json\"\n\nfunc F() { json.Marshal(nil) }\n";
        let facts = extract("a.go", src);
        assert_eq!(facts.imports.len(), 1);
        assert_eq!(facts.imports[0].specifier.as_str(), "encoding/json");
        assert!(!facts.imports[0].side_effect_only);
        let binding = &facts.imports[0].bindings[0];
        assert_eq!(binding.local.as_str(), "json.Marshal");
        assert_eq!(binding.imported.as_deref(), Some("Marshal"));
        assert!(facts
            .references
            .iter()
            .any(|r| r.name.as_str() == "json.Marshal"));
    }

    #[test]
    fn aliased_import_binds_the_explicit_alias() {
        let src = b"package p\n\nimport j \"encoding/json\"\n\nfunc F() { j.Marshal(nil) }\n";
        let facts = extract("a.go", src);
        assert_eq!(facts.imports[0].bindings[0].local.as_str(), "j.Marshal");
    }

    #[test]
    fn blank_import_is_side_effect_only() {
        let facts = extract("a.go", b"package p\n\nimport _ \"embed\"\n");
        assert!(facts.imports[0].side_effect_only);
        assert!(!facts.imports[0].opaque_namespace_use);
    }

    #[test]
    fn dot_import_is_opaque_namespace_use() {
        let facts = extract("a.go", b"package p\n\nimport . \"strings\"\n");
        assert!(facts.imports[0].opaque_namespace_use);
    }

    #[test]
    fn grouped_imports_are_all_extracted() {
        let src = b"package p\n\nimport (\n\t\"fmt\"\n\t\"encoding/json\"\n)\n";
        let facts = extract("a.go", src);
        let specs: Vec<&str> = facts.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specs, vec!["fmt", "encoding/json"]);
    }

    #[test]
    fn plain_identifier_call_is_a_reference() {
        let facts = extract("a.go", b"package p\n\nfunc a() {}\nfunc b() { a() }\n");
        assert!(facts.references.iter().any(|r| r.name.as_str() == "a"));
    }

    #[test]
    fn declaration_name_sites_are_never_references() {
        let facts = extract("a.go", b"package p\n\nfunc lonelyName() {}\n");
        assert!(!facts
            .references
            .iter()
            .any(|r| r.name.as_str() == "lonelyName"));
    }

    #[test]
    fn kndo_allow_pragma_is_extracted_as_a_suppression() {
        let facts = extract(
            "a.go",
            b"package p\n\n// kndo:allow unused stale helper\nfunc helper() {}\n",
        );
        assert_eq!(facts.suppressions.len(), 1);
        assert_eq!(facts.suppressions[0].category.as_str(), "unused");
        assert_eq!(
            facts.suppressions[0].reason.as_deref(),
            Some("stale helper")
        );
    }

    #[test]
    fn syntax_errors_degrade_to_a_diagnostic_not_a_panic() {
        let facts = extract("a.go", b"package p\n\nfunc broken( {{{ garbage\n");
        assert!(!facts.diagnostics.is_empty());
    }
}

#[cfg(test)]
mod debug_probe {
    use super::*;
    #[test]
    fn debug_real_main_go() {
        let src = b"package main\n\nimport (\n\t\"fmt\"\n\n\t\"example.com/demo/sub\"\n)\n\nfunc main() {\n\tfmt.Println(sub.Greeting())\n}\n";
        let facts = extract("main.go", src);
        eprintln!("roots: {:?}", facts.roots);
        eprintln!("declarations: {:?}", facts.declarations);
        eprintln!("imports: {:?}", facts.imports);
    }
}

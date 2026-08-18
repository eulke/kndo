//! Declaration & import extraction — spec docs/adapters/js-ts.md §2–3, first slice.
//!
//! Field names below are verified against the real tree-sitter-typescript grammar (not
//! assumed) — see `kndo_adapter_toolkit::parsing::introspect` for the probe this was built
//! against. Scope of this slice: top-level declarations (functions, classes, interfaces,
//! type aliases, enums + members, const/let) and ESM static imports. Deferred to later
//! commits (each already flagged in the spec, not silently missing): class/interface
//! members, CJS export patterns, JSX references, dynamic constructs, cyclomatic complexity,
//! fingerprints, suppressions, `export { a as b }` / `export * from` surface nuances.

use kndo_core::adapter::{
    Declaration, Diagnostic, DiagnosticLevel, FileFacts, ImportKind, RawImport, Span,
    VisibilityLevel,
};
use kndo_core::vocab::{Confidence, SymbolKind};
use smol_str::SmolStr;
use tree_sitter::Node;

pub fn extract(path: &str, content: &[u8]) -> FileFacts {
    let tsx = path.ends_with(".tsx") || path.ends_with(".jsx");
    let mut out = FileFacts::default();

    let Some(tree) = kndo_adapter_toolkit::parsing::parse(content, tsx) else {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            message: "failed to initialize the tree-sitter parser".into(),
            span: None,
        });
        return out;
    };

    let root = tree.root_node();
    if root.has_error() {
        // Contract: adapters must not fail on broken code (contracts §2) — tree-sitter still
        // produces a usable partial tree, so we keep walking and just flag it.
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            message: "syntax errors in file — extraction is best-effort".into(),
            span: None,
        });
    }

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        handle_statement(child, content, false, &mut out);
    }
    out
}

fn span(node: Node) -> Span {
    let s = node.start_position();
    let e = node.end_position();
    Span {
        start: (s.row as u32 + 1, s.column as u32 + 1),
        end: (e.row as u32 + 1, e.column as u32 + 1),
    }
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

fn visibility(exported: bool) -> VisibilityLevel {
    // Ladder per spec §4: module-local(0) < exported(1) < package-surface(2). The package
    // surface level is a graph-assembly-time promotion (cross-references the owning
    // package's `exports` map) — out of an adapter's per-file reach, so extraction only ever
    // emits 0 or 1.
    VisibilityLevel(if exported { 1 } else { 0 })
}

fn string_literal_value(node: Node, src: &[u8]) -> Option<SmolStr> {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .find(|c| c.kind() == "string_fragment");
    found.map(|c| SmolStr::new(text(c, src)))
}

/// Dispatches one top-level (or export-unwrapped) statement. `exported` is threaded in by
/// the caller when unwrapping an `export_statement`.
fn handle_statement(node: Node, src: &[u8], exported: bool, out: &mut FileFacts) {
    match node.kind() {
        "export_statement" => handle_export_statement(node, src, out),
        "import_statement" => handle_import_statement(node, src, out),
        "function_declaration" | "generator_function_declaration" => {
            handle_named(node, src, exported, out, SymbolKind::Function)
        }
        "class_declaration" => handle_named(node, src, exported, out, SymbolKind::Class),
        "interface_declaration" => handle_named(node, src, exported, out, SymbolKind::Interface),
        "type_alias_declaration" => handle_named(node, src, exported, out, SymbolKind::TypeAlias),
        "enum_declaration" => handle_enum(node, src, exported, out),
        "lexical_declaration" | "variable_declaration" => handle_lexical(node, src, exported, out),
        _ => {}
    }
}

/// Handles a declaration whose only shape variance is its `name` field falling back to
/// `default` (covers `export default function foo(){}`-style named-but-default exports).
fn handle_named(node: Node, src: &[u8], exported: bool, out: &mut FileFacts, kind: SymbolKind) {
    let name = node
        .child_by_field_name("name")
        .map(|n| SmolStr::new(text(n, src)))
        .unwrap_or_else(|| SmolStr::new("default"));
    out.declarations.push(Declaration {
        name,
        kind,
        span: span(node),
        exported,
        visibility: visibility(exported),
    });
}

fn handle_export_statement(node: Node, src: &[u8], out: &mut FileFacts) {
    if let Some(decl) = node.child_by_field_name("declaration") {
        handle_statement(decl, src, true, out);
        return;
    }
    if let Some(value) = node.child_by_field_name("value") {
        // `export default <anonymous class|function|expression>` — spec §2: synthetic
        // symbol named `default`.
        let kind = match value.kind() {
            "class" => SymbolKind::Class,
            "function" | "arrow_function" | "generator_function" => SymbolKind::Function,
            _ => SymbolKind::Const,
        };
        out.declarations.push(Declaration {
            name: SmolStr::new("default"),
            kind,
            span: span(node),
            exported: true,
            visibility: visibility(true),
        });
    }
    // `export { a as c }`, `export * from "..."`, `export * as ns from "..."` — export-
    // surface binding nuances (RFC 0005 §13 redundant-export-binding territory); no new
    // declarations to extract here, deliberately not attempted in this slice.
}

fn handle_enum(node: Node, src: &[u8], exported: bool, out: &mut FileFacts) {
    let name = node
        .child_by_field_name("name")
        .map(|n| SmolStr::new(text(n, src)))
        .unwrap_or_else(|| SmolStr::new("default"));
    out.declarations.push(Declaration {
        name,
        kind: SymbolKind::Enum,
        span: span(node),
        exported,
        visibility: visibility(exported),
    });

    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        let member_name = match member.kind() {
            "property_identifier" => Some(SmolStr::new(text(member, src))),
            "enum_assignment" => member
                .child_by_field_name("name")
                .map(|n| SmolStr::new(text(n, src))),
            _ => None,
        };
        if let Some(member_name) = member_name {
            out.declarations.push(Declaration {
                name: member_name,
                kind: SymbolKind::EnumMember,
                span: span(member),
                exported,
                visibility: visibility(exported),
            });
        }
    }
}

fn handle_lexical(node: Node, src: &[u8], exported: bool, out: &mut FileFacts) {
    let declared_kind = match node.child_by_field_name("kind").map(|n| text(n, src)) {
        Some("const") => SymbolKind::Const,
        _ => SymbolKind::Variable, // `let` and plain `var` (no `kind` field) both land here.
    };
    let mut cursor = node.walk();
    for declarator in node.children(&mut cursor) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_node) = declarator.child_by_field_name("name") else {
            continue;
        };
        // Destructuring patterns (`const { a, b } = x`) deferred — not silently dropped from
        // the spec, just not in this slice's scope.
        if name_node.kind() != "identifier" {
            continue;
        }
        out.declarations.push(Declaration {
            name: SmolStr::new(text(name_node, src)),
            kind: declared_kind.clone(),
            span: span(declarator),
            exported,
            visibility: visibility(exported),
        });
    }
}

fn handle_import_statement(node: Node, src: &[u8], out: &mut FileFacts) {
    let Some(source_node) = node.child_by_field_name("source") else {
        return;
    };
    let Some(specifier) = string_literal_value(source_node, src) else {
        return;
    };

    let kind =
        if specifier.starts_with('.') || specifier.starts_with('/') || specifier.starts_with('#') {
            ImportKind::Relative
        } else {
            ImportKind::Package
        };

    // `import_clause` has no field name of its own on `import_statement` (verified against
    // the grammar — it's an unnamed child), so presence is checked by kind, not by field.
    let mut cursor = node.walk();
    let mut has_type_token = false;
    let mut has_clause = false;
    for c in node.children(&mut cursor) {
        match c.kind() {
            "type" => has_type_token = true,
            "import_clause" => has_clause = true,
            _ => {}
        }
    }
    let type_only = has_type_token;
    let side_effect_only = !has_clause;

    out.imports.push(RawImport {
        specifier,
        kind,
        span: span(node),
        side_effect_only,
        type_only,
        confidence: Confidence::Certain,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(src: &str) -> Vec<(String, SymbolKind, bool)> {
        extract("f.ts", src.as_bytes())
            .declarations
            .into_iter()
            .map(|d| (d.name.to_string(), d.kind, d.exported))
            .collect()
    }

    #[test]
    fn exported_function() {
        let d = decls("export function foo(x: number): number { return x; }");
        assert_eq!(d, vec![("foo".into(), SymbolKind::Function, true)]);
    }

    #[test]
    fn unexported_function() {
        let d = decls("function foo() {}");
        assert_eq!(d, vec![("foo".into(), SymbolKind::Function, false)]);
    }

    #[test]
    fn default_export_named_function_keeps_its_name() {
        let d = decls("export default function bar() {}");
        assert_eq!(d, vec![("bar".into(), SymbolKind::Function, true)]);
    }

    #[test]
    fn default_export_anonymous_class_gets_synthetic_name() {
        let d = decls("export default class {}");
        assert_eq!(d, vec![("default".into(), SymbolKind::Class, true)]);
    }

    #[test]
    fn interface_and_type_alias() {
        let d = decls("export interface Quux { id: number; }\nexport type Alias = string;");
        assert_eq!(
            d,
            vec![
                ("Quux".into(), SymbolKind::Interface, true),
                ("Alias".into(), SymbolKind::TypeAlias, true),
            ]
        );
    }

    #[test]
    fn enum_with_members() {
        let d = decls(r#"export enum Color { Red, Green = "g" }"#);
        assert_eq!(
            d,
            vec![
                ("Color".into(), SymbolKind::Enum, true),
                ("Red".into(), SymbolKind::EnumMember, true),
                ("Green".into(), SymbolKind::EnumMember, true),
            ]
        );
    }

    #[test]
    fn multiple_const_declarators_and_let() {
        let d = decls("export const a = 1, b = 2;\nlet c = 3;");
        assert_eq!(
            d,
            vec![
                ("a".into(), SymbolKind::Const, true),
                ("b".into(), SymbolKind::Const, true),
                ("c".into(), SymbolKind::Variable, false),
            ]
        );
    }

    #[test]
    fn esm_static_import_is_certain() {
        let facts = extract("f.ts", b"import { x } from './mod';");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert_eq!(imp.specifier.as_str(), "./mod");
        assert_eq!(imp.kind, ImportKind::Relative);
        assert_eq!(imp.confidence, Confidence::Certain);
        assert!(!imp.type_only);
        assert!(!imp.side_effect_only);
    }

    #[test]
    fn package_specifier_is_not_relative() {
        let facts = extract("f.ts", b"import x from 'lodash';");
        assert_eq!(facts.imports[0].kind, ImportKind::Package);
    }

    #[test]
    fn import_type_is_flagged_type_only() {
        let facts = extract("f.ts", b"import type { T } from './types';");
        assert!(facts.imports[0].type_only);
    }

    #[test]
    fn side_effect_import_has_no_clause() {
        let facts = extract("f.ts", b"import './polyfill';");
        assert!(facts.imports[0].side_effect_only);
    }

    #[test]
    fn broken_syntax_does_not_panic_and_flags_a_diagnostic() {
        let facts = extract("f.ts", b"export function foo( {{{ this is not valid");
        assert!(!facts.diagnostics.is_empty());
    }

    #[test]
    fn tsx_extension_parses_jsx() {
        // Would fail to parse (or mis-parse) under the plain TS grammar — proves the
        // extension-based grammar switch in `extract()` actually takes effect.
        let facts = extract("f.tsx", b"export function App() { return <div/>; }");
        assert_eq!(facts.declarations.len(), 1);
        assert!(facts.diagnostics.is_empty());
    }
}

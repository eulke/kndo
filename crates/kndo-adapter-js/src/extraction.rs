//! Declaration & import extraction — spec docs/adapters/js-ts.md §2–3, first slice.
//!
//! Field names below are verified against the real tree-sitter-typescript grammar (not
//! assumed) — see `kndo_adapter_toolkit::parsing::introspect` for the probe this was built
//! against. Scope of this slice: top-level declarations (functions, classes, interfaces,
//! type aliases, enums + members, const/let), ESM static imports, and `export ... from`
//! re-exports (barrels — `handle_reexport_statement`). Deferred to later commits (each
//! already flagged in the spec, not silently missing): class/interface members, CJS export
//! patterns, JSX references, dynamic constructs, cyclomatic complexity, fingerprints,
//! suppressions, `export { a as b }` with no `from` clause (a local re-export, not a barrel
//! pass-through).

use kndo_core::adapter::{
    Declaration, Diagnostic, DiagnosticLevel, FileFacts, ImportBinding, ImportKind, RawImport,
    RawReference, Span, VisibilityLevel,
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
            path: None, // filled in by the core when merging FileFacts into RunResult
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
            path: None,
            message: "syntax errors in file — extraction is best-effort".into(),
            span: None,
        });
    }

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        handle_statement(child, content, false, &mut out);
    }
    // Separate full-tree walk (declarations above only visit top-level statements — a
    // reference can appear at any nesting depth, inside any function/block).
    collect_references(root, content, &mut out.references);
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
        return;
    }
    if let Some(source_node) = node.child_by_field_name("source") {
        handle_reexport_statement(node, source_node, src, out);
    }
    // Otherwise: `export { a as c }` (no `from` clause — re-exporting an already-declared
    // local symbol under a new public name) — export-surface binding nuances (RFC 0005 §13
    // redundant-export-binding territory); no new declarations to extract here, deliberately
    // not attempted in this slice.
}

/// `export ... from "specifier"` — a re-export (js-ts.md §5: "Barrel files… resolved through,
/// transparently"). Modeled as a [`RawImport`] whose bindings are also this file's own export
/// surface (`RawImport::reexported`) — `export {a, b as c} from './x'` and
/// `export type {a, b as c} from './x'` collect real bindings via `export_clause`, same shape
/// as `import_clause`'s `named_imports`; the bare-star forms (`export * from`, `export * as ns
/// from`) contribute no bindings, same precedent as a plain `import * as ns` (namespace member
/// access isn't reference-resolved in this slice) — the import edge itself still lands, which
/// is what fixes those targets' file-level reachability.
///
/// Verified against the real grammar (`kndo_adapter_toolkit::parsing::introspect::
/// dump_reexport_shapes`): `export type { a } from` (the named-clause form) parses cleanly, but
/// `export type * from` is a grammar ERROR in tree-sitter-typescript 0.23.2 specifically around
/// the `type` token in the bare-star form — the `source` field survives regardless, so the
/// specifier is still recoverable; the file still gets its (accurate) "syntax errors" diagnostic.
fn handle_reexport_statement(node: Node, source_node: Node, src: &[u8], out: &mut FileFacts) {
    let Some(specifier) = string_literal_value(source_node, src) else {
        return;
    };
    let kind =
        if specifier.starts_with('.') || specifier.starts_with('/') || specifier.starts_with('#') {
            ImportKind::Relative
        } else {
            ImportKind::Package
        };

    let mut cursor = node.walk();
    let mut type_only = false;
    let mut bindings = Vec::new();
    for c in node.children(&mut cursor) {
        match c.kind() {
            "type" => type_only = true,
            "ERROR" => {
                let mut inner = c.walk();
                if c.children(&mut inner).any(|gc| gc.kind() == "type") {
                    type_only = true;
                }
            }
            "export_clause" => bindings = collect_export_bindings(c, src),
            _ => {}
        }
    }

    out.imports.push(RawImport {
        specifier,
        kind,
        span: span(node),
        side_effect_only: false,
        type_only,
        confidence: Confidence::Certain,
        bindings,
        reexported: true,
    });
}

/// `export_clause`'s children: `export_specifier` nodes, the export-direction mirror of
/// `collect_import_bindings`'s `import_specifier` handling. `name` is always the *original*
/// name in the `from` target (what the re-export resolves through to); `alias`, when present,
/// is the name this file re-exposes it as — `local` here matches that "name used in this
/// file's own export surface" role, same as a regular import binding's `local`.
fn collect_export_bindings(export_clause: Node, src: &[u8]) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    let mut cursor = export_clause.walk();
    for spec in export_clause.children(&mut cursor) {
        if spec.kind() != "export_specifier" {
            continue;
        }
        let Some(name_node) = spec.child_by_field_name("name") else {
            continue;
        };
        let name = text(name_node, src);
        let local = spec
            .child_by_field_name("alias")
            .map(|a| text(a, src))
            .unwrap_or(name);
        bindings.push(ImportBinding {
            local: SmolStr::new(local),
            imported: Some(SmolStr::new(name)),
        });
    }
    bindings
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
    let mut bindings = Vec::new();
    for c in node.children(&mut cursor) {
        match c.kind() {
            "type" => has_type_token = true,
            "import_clause" => {
                has_clause = true;
                bindings = collect_import_bindings(c, src);
            }
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
        bindings,
        reexported: false,
    });
}

/// `import_clause`'s children: a bare `identifier` (default import), `named_imports` (each
/// `import_specifier` an exported name plus optional local `alias`), or `namespace_import`
/// (`* as ns` — deferred: resolving `ns.foo` back to a specific export needs member-expression-
/// aware reference resolution this slice doesn't attempt; the import edge is unaffected, only
/// the finer per-binding fact is missed).
fn collect_import_bindings(import_clause: Node, src: &[u8]) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    let mut cursor = import_clause.walk();
    for child in import_clause.children(&mut cursor) {
        match child.kind() {
            "identifier" => bindings.push(ImportBinding {
                local: SmolStr::new(text(child, src)),
                imported: None,
            }),
            "named_imports" => {
                let mut inner = child.walk();
                for spec in child.children(&mut inner) {
                    if spec.kind() != "import_specifier" {
                        continue;
                    }
                    let Some(name_node) = spec.child_by_field_name("name") else {
                        continue;
                    };
                    let name = text(name_node, src);
                    let local = spec
                        .child_by_field_name("alias")
                        .map(|a| text(a, src))
                        .unwrap_or(name);
                    bindings.push(ImportBinding {
                        local: SmolStr::new(local),
                        imported: Some(SmolStr::new(name)),
                    });
                }
            }
            _ => {}
        }
    }
    bindings
}

/// Every identifier/type-identifier usage in the tree, recursively — the shapes below are
/// verified against the real grammar (`kndo_adapter_toolkit::parsing::introspect`, the
/// `dump_reference_shapes`/`dump_binding_shapes`/`dump_import_clause_shapes` probes), not
/// assumed. Two things a naive "collect every identifier" walk gets wrong, handled explicitly:
///
/// 1. **Property/member names are never references** — `property_identifier` (object literal
///    keys, `.member` access, class member names) names a *position*, not a scope lookup, so
///    it's simply never in the collectible-kinds list below (no per-site check needed —
///    excluded by construction).
/// 2. **Binding positions introduce a name rather than look one up** — a declaration's own
///    name, a parameter/catch/for-loop binding, or a destructuring pattern (which can nest
///    arbitrarily) — so those are skipped as *whole subtrees*, not just their own node, keyed
///    by (parent kind, field). A destructuring pattern's *value* side (`= obj` in
///    `const { a } = obj`) is a real reference and is walked normally; only the *pattern* side
///    is skipped.
///
/// Where in doubt, this errs toward collecting (a reference to a name nothing declares simply
/// fails to resolve later and is dropped, silently and safely) rather than excluding (which
/// would risk marking genuinely-used code `unused` — the direction that actually matters).
fn collect_references(node: Node, src: &[u8], out: &mut Vec<RawReference>) {
    // Import statements are entirely declarative name-binding syntax (already turned into
    // `ImportBinding` facts by `collect_import_bindings`) — nothing inside one is a reference.
    if node.kind() == "import_statement" {
        return;
    }

    let skip_field: Option<&str> = match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "class_declaration"
        | "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration"
        | "variable_declarator" => Some("name"),
        "required_parameter" | "optional_parameter" => Some("pattern"),
        "for_in_statement" => Some("left"),
        "catch_clause" => Some("parameter"),
        _ => None,
    };
    let skip_id = skip_field
        .and_then(|f| node.child_by_field_name(f))
        .map(|n| n.id());

    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "shorthand_property_identifier"
    ) {
        out.push(RawReference {
            name: SmolStr::new(text(node, src)),
            scope_context: None,
            span: span(node),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if Some(child.id()) == skip_id {
            continue;
        }
        collect_references(child, src, out);
    }
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

    fn refs(src: &str) -> Vec<String> {
        extract("f.ts", src.as_bytes())
            .references
            .into_iter()
            .map(|r| r.name.to_string())
            .collect()
    }

    fn refs_tsx(src: &str) -> Vec<String> {
        extract("f.tsx", src.as_bytes())
            .references
            .into_iter()
            .map(|r| r.name.to_string())
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

    // ---------------------------------------------------------------- references

    #[test]
    fn function_call_is_a_reference_but_the_declaration_name_is_not() {
        let r = refs("function outer() { foo(); }");
        assert!(r.contains(&"foo".to_string()));
        assert!(!r.contains(&"outer".to_string()));
    }

    #[test]
    fn member_expression_object_is_a_reference_but_the_property_is_not() {
        let r = refs("function f() { obj.method(); }");
        assert!(r.contains(&"obj".to_string()));
        assert!(!r.contains(&"method".to_string()));
    }

    #[test]
    fn object_literal_key_is_not_a_reference_but_shorthand_value_is() {
        let r = refs("function f() { const o = { key: 1, shorthand }; }");
        assert!(!r.contains(&"key".to_string()));
        assert!(r.contains(&"shorthand".to_string()));
    }

    #[test]
    fn destructuring_names_are_not_references_but_the_source_is() {
        let r = refs("function f() { const { a, b: renamed } = source; }");
        assert!(!r.contains(&"a".to_string()));
        assert!(!r.contains(&"renamed".to_string()));
        assert!(r.contains(&"source".to_string()));
    }

    #[test]
    fn parameter_names_are_not_references_but_default_values_are() {
        let r = refs("function f(x, y = fallback) {}");
        assert!(!r.contains(&"x".to_string()));
        assert!(!r.contains(&"y".to_string()));
        assert!(r.contains(&"fallback".to_string()));
    }

    #[test]
    fn class_extends_and_implements_are_references_but_its_own_name_is_not() {
        let r = refs("class C extends Base implements IFace {}");
        assert!(r.contains(&"Base".to_string()));
        assert!(r.contains(&"IFace".to_string()));
        assert!(!r.contains(&"C".to_string()));
    }

    #[test]
    fn type_annotation_is_a_reference() {
        let r = refs("function f() { let x: SomeType; }");
        assert!(r.contains(&"SomeType".to_string()));
        assert!(!r.contains(&"x".to_string()));
    }

    #[test]
    fn for_of_binding_is_not_a_reference_but_the_iterable_is() {
        let r = refs("function f() { for (const item of items) { use(item); } }");
        assert!(r.contains(&"items".to_string()));
        assert!(r.contains(&"use".to_string()));
        // "item" is used once, inside `use(item)` — the loop binding itself must not add a
        // second, spurious occurrence.
        assert_eq!(r.iter().filter(|n| *n == "item").count(), 1);
    }

    #[test]
    fn catch_binding_is_not_a_reference_but_its_use_inside_the_block_is() {
        let r = refs("function f() { try {} catch (e) { log(e); } }");
        assert!(r.contains(&"log".to_string()));
        assert_eq!(r.iter().filter(|n| *n == "e").count(), 1);
    }

    #[test]
    fn jsx_component_name_is_a_reference() {
        let r = refs_tsx("function App() { return <Foo bar={baz} />; }");
        assert!(r.contains(&"Foo".to_string()));
        assert!(r.contains(&"baz".to_string()));
        assert!(!r.contains(&"bar".to_string())); // the JSX attribute name, not a value lookup
    }

    #[test]
    fn import_statement_contributes_no_references() {
        let r = refs("import { x } from './mod';");
        assert!(r.is_empty());
    }

    // ---------------------------------------------------------------- import bindings

    fn bindings(src: &str) -> Vec<(String, Option<String>)> {
        extract("f.ts", src.as_bytes()).imports[0]
            .bindings
            .iter()
            .map(|b| {
                (
                    b.local.to_string(),
                    b.imported.as_ref().map(|s| s.to_string()),
                )
            })
            .collect()
    }

    #[test]
    fn default_import_binds_with_no_imported_name() {
        assert_eq!(
            bindings("import def from './a';"),
            vec![("def".into(), None)]
        );
    }

    #[test]
    fn named_imports_bind_by_exported_name() {
        assert_eq!(
            bindings("import { x, y } from './b';"),
            vec![
                ("x".into(), Some("x".into())),
                ("y".into(), Some("y".into()))
            ]
        );
    }

    #[test]
    fn renamed_named_import_binds_local_to_the_original_exported_name() {
        assert_eq!(
            bindings("import { x as z } from './b';"),
            vec![("z".into(), Some("x".into()))]
        );
    }

    #[test]
    fn default_and_named_combo_binds_both() {
        assert_eq!(
            bindings("import def, { named } from './c';"),
            vec![("def".into(), None), ("named".into(), Some("named".into()))]
        );
    }

    #[test]
    fn namespace_import_binds_nothing_but_the_import_edge_still_exists() {
        let facts = extract("f.ts", b"import * as ns from './d';");
        assert!(facts.imports[0].bindings.is_empty());
        assert!(!facts.imports[0].side_effect_only);
    }

    #[test]
    fn side_effect_import_has_no_bindings() {
        assert!(bindings("import './polyfill';").is_empty());
    }

    // ---------------------------------------------------------------- re-exports

    #[test]
    fn named_reexport_is_flagged_and_binds_by_original_name() {
        let facts = extract("f.ts", b"export { a, b as c } from './x';");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert_eq!(imp.specifier.as_str(), "./x");
        assert_eq!(imp.kind, ImportKind::Relative);
        assert!(imp.reexported);
        assert!(!imp.type_only);
        assert_eq!(
            imp.bindings
                .iter()
                .map(|b| (
                    b.local.to_string(),
                    b.imported.as_ref().map(|s| s.to_string())
                ))
                .collect::<Vec<_>>(),
            vec![
                ("a".into(), Some("a".into())),
                ("c".into(), Some("b".into())),
            ]
        );
    }

    #[test]
    fn typed_named_reexport_is_type_only() {
        let facts = extract("f.ts", b"export type { T } from './types';");
        assert!(facts.imports[0].reexported);
        assert!(facts.imports[0].type_only);
    }

    #[test]
    fn star_reexport_has_no_bindings_but_still_reexports() {
        let facts = extract("f.ts", b"export * from './all';");
        let imp = &facts.imports[0];
        assert!(imp.reexported);
        assert!(imp.bindings.is_empty());
        assert_eq!(imp.specifier.as_str(), "./all");
    }

    #[test]
    fn typed_star_reexport_still_recovers_the_specifier_despite_the_grammar_gap() {
        // `export type *` is a tree-sitter-typescript 0.23.2 grammar ERROR around the `type`
        // token specifically for the bare-star form (verified via
        // kndo_adapter_toolkit::parsing::introspect::dump_reexport_shapes) — the `source`
        // field survives regardless, so extraction still recovers the specifier and still
        // flags `type_only`, just alongside the (accurate) syntax-error diagnostic.
        let facts = extract("f.ts", b"export type * from './all-types';");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert!(imp.reexported);
        assert!(imp.type_only);
        assert_eq!(imp.specifier.as_str(), "./all-types");
        assert!(!facts.diagnostics.is_empty());
    }

    #[test]
    fn namespace_reexport_has_no_bindings() {
        let facts = extract("f.ts", b"export * as ns from './d';");
        let imp = &facts.imports[0];
        assert!(imp.reexported);
        assert!(imp.bindings.is_empty());
    }

    #[test]
    fn plain_reexport_of_a_local_symbol_contributes_no_import() {
        // No `from` clause — re-exporting an already-declared local symbol, not a barrel
        // pass-through. Out of scope for this slice (see handle_export_statement).
        let facts = extract("f.ts", b"function f() {}\nexport { f as g };");
        assert!(facts.imports.is_empty());
    }

    #[test]
    fn ordinary_import_is_never_flagged_as_a_reexport() {
        let facts = extract("f.ts", b"import { x } from './mod';");
        assert!(!facts.imports[0].reexported);
    }
}

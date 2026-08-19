//! Declaration & import extraction — spec docs/adapters/js-ts.md §2–3, first slice.
//!
//! Field names below are verified against the real tree-sitter-typescript grammar (not
//! assumed) — see `kndo_adapter_toolkit::parsing::introspect` for the probe this was built
//! against. Scope of this slice: top-level declarations (functions, classes, interfaces,
//! type aliases, enums + members, const/let), ESM static imports, `export ... from`
//! re-exports (barrels — `handle_reexport_statement`), and CJS (`require("literal")` at any
//! depth, `module.exports`/`exports.foo` export surface, `module.exports = require(…)`
//! barrels — the `collect_requires`/`collect_cjs_exports` block), and dynamic constructs
//! (`import("literal")`/`require.resolve` at probable; non-literal `import(expr)`/
//! `require(expr)` with static-prefix narrowing, `eval`, `new Function` → `DynamicUse`
//! wildcards). Deferred to later commits (each already flagged in the spec, not silently
//! missing): class/interface members, JSX references, the remaining dynamic-table rows
//! (computed member access `ns[key]`, string-keyed registries) and namespace-member
//! consumption like `env.colors` — the main remaining CJS gap — plus cyclomatic complexity,
//! fingerprints, suppressions, `export { a as b }` with no `from` clause (a local re-export,
//! not a barrel pass-through).

use kndo_core::adapter::{
    Declaration, Diagnostic, DiagnosticLevel, DynamicUse, FileFacts, ImportBinding, ImportKind,
    RawImport, RawReference, Span, VisibilityLevel,
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
    // CJS export surface (`module.exports = …`, `exports.foo = …`) — a separate top-level pass
    // *after* the declaration walk, because `exports.foo = foo` may textually precede
    // `function foo() {}` (hoisting) and the mark-existing-declaration decision needs the
    // complete declaration list.
    collect_cjs_exports(root, content, &mut out);
    // CJS imports (`require("literal")`), dynamic imports (`import(…)`), and dynamic
    // constructs (`eval`, `new Function`) — a full-tree walk like references, because any of
    // them can appear at any nesting depth, not just in top-level statements.
    collect_requires(root, path, content, &mut out);
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
        // `const x = require("./y")` is an import in declaration clothing — the name is an
        // import binding (collect_requires), not a declaration, exactly as `import x from`
        // declares nothing. Emitting both would leave a symbol `x` that nothing ever
        // references (references to `x` resolve to the *target's* symbol via the binding),
        // i.e. a guaranteed false-positive `unused`. The rare `export const x = require(…)`
        // keeps its declaration: the export surface is real even though the value is imported.
        if !exported
            && declarator
                .child_by_field_name("value")
                .is_some_and(|v| is_require_call(v, src))
        {
            continue;
        }
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

// ---------------------------------------------------------------- CJS & dynamic constructs
// (spec §2 export surface + dynamic-constructs table, §3 import table — shapes verified via
// `dump_cjs_shapes` and `dump_dynamic_shapes`)

/// A `require(<single string literal>)` call — the require form that imports at `certain`
/// (spec §3). The non-literal form is handled separately as a [`DynamicUse`] wildcard.
fn is_require_call(node: Node, src: &[u8]) -> bool {
    require_specifier(node, src).is_some()
}

fn require_specifier<'t>(node: Node<'t>, src: &[u8]) -> Option<Node<'t>> {
    if node.kind() != "call_expression" {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    if function.kind() != "identifier" || text(function, src) != "require" {
        return None;
    }
    single_argument(node).filter(|arg| arg.kind() == "string")
}

/// The lone named argument of a call, when there is exactly one.
fn single_argument(call: Node) -> Option<Node> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let real_args: Vec<Node> = arguments
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();
    match real_args.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Full-tree walk for `require`/`import()` calls and dynamic constructs, at any nesting
/// depth — a lazy require inside a function body is still a real edge. `path` is the file's
/// own project-relative path, needed to resolve a dynamic narrowing prefix (`./locales/${x}`)
/// into the project-relative directory the contract's `narrowed_to` expects.
fn collect_requires(node: Node, path: &str, src: &[u8], out: &mut FileFacts) {
    match node.kind() {
        "call_expression" => handle_call_expression(node, path, src, out),
        // `new Function("…")` — spec §2's dynamic table: wildcard, file-wide.
        "new_expression" => {
            if node
                .child_by_field_name("constructor")
                .is_some_and(|c| c.kind() == "identifier" && text(c, src) == "Function")
            {
                out.dynamics.push(DynamicUse {
                    span: span(node),
                    reason: SmolStr::new("new Function"),
                    narrowed_to: None,
                });
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_requires(child, path, src, out);
    }
}

fn handle_call_expression(node: Node, path: &str, src: &[u8], out: &mut FileFacts) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    match function.kind() {
        // `import(…)` — its own node kind in the grammar, not an identifier.
        "import" => match single_argument(node) {
            Some(arg) if arg.kind() == "string" => {
                // spec §3: `import("literal")` → probable (bundler code-split semantics). No
                // bindings this slice: the awaited value is the namespace object, and
                // namespace member resolution is the same deferred gap as `import * as ns`.
                push_dynamic_import(node, arg, Confidence::Probable, src, out);
            }
            Some(arg) => out.dynamics.push(DynamicUse {
                span: span(node),
                reason: SmolStr::new("non-literal import()"),
                narrowed_to: static_prefix_dir(arg, path, src),
            }),
            None => {}
        },
        "identifier" => match text(function, src) {
            "require" => match single_argument(node) {
                Some(arg) if arg.kind() == "string" => handle_literal_require(node, arg, src, out),
                Some(arg) => out.dynamics.push(DynamicUse {
                    span: span(node),
                    reason: SmolStr::new("non-literal require()"),
                    narrowed_to: static_prefix_dir(arg, path, src),
                }),
                None => {}
            },
            // Direct `eval` only — indirect (`window.eval`) doesn't even see local scope.
            "eval" => out.dynamics.push(DynamicUse {
                span: span(node),
                reason: SmolStr::new("eval"),
                narrowed_to: None,
            }),
            _ => {}
        },
        // `require.resolve("literal")` — spec §3: file, probable. The non-literal form is
        // deferred (it resolves a path rather than loading a module, so folding it into the
        // require-wildcard would overstate what it keeps alive).
        "member_expression" => {
            let is_require_resolve = function
                .child_by_field_name("object")
                .is_some_and(|o| o.kind() == "identifier" && text(o, src) == "require")
                && function
                    .child_by_field_name("property")
                    .is_some_and(|p| text(p, src) == "resolve");
            if is_require_resolve {
                if let Some(arg) = single_argument(node).filter(|a| a.kind() == "string") {
                    push_dynamic_import(node, arg, Confidence::Probable, src, out);
                }
            }
        }
        _ => {}
    }
}

/// A bindingless import from a call-shaped construct (`import("literal")`,
/// `require.resolve("literal")`) — the edge is what reachability needs.
fn push_dynamic_import(
    call: Node,
    string_node: Node,
    confidence: Confidence,
    src: &[u8],
    out: &mut FileFacts,
) {
    let Some(specifier) = string_literal_value(string_node, src) else {
        return;
    };
    let kind = import_kind(&specifier);
    out.imports.push(RawImport {
        specifier,
        kind,
        span: span(call),
        side_effect_only: call
            .parent()
            .is_some_and(|p| p.kind() == "expression_statement"),
        type_only: false,
        confidence,
        bindings: Vec::new(),
        reexported: false,
    });
}

fn import_kind(specifier: &str) -> ImportKind {
    if specifier.starts_with('.') || specifier.starts_with('/') || specifier.starts_with('#') {
        ImportKind::Relative
    } else {
        ImportKind::Package
    }
}

/// The static string prefix of a dynamic specifier, resolved to the **project-relative
/// directory** the contract's `narrowed_to` expects (spec §2: `./locales/${x}` → that
/// directory). Two shapes carry a usable prefix (verified via `dump_dynamic_shapes`): a
/// template string whose *leading* piece is a text fragment, and a `+` concatenation whose
/// leftmost operand is a string literal. The prefix narrows only when it's relative (a
/// package-name prefix can't name project files) and contains a `/` (without one there's no
/// directory to name). A prefix resolving to the project root returns `None` — "anywhere in
/// the project" is not a narrowing.
fn static_prefix_dir(arg: Node, path: &str, src: &[u8]) -> Option<SmolStr> {
    let prefix: SmolStr = match arg.kind() {
        "template_string" => {
            let first = arg.named_child(0)?;
            if first.kind() != "string_fragment" {
                return None; // `${x}/…` — no leading static text
            }
            SmolStr::new(text(first, src))
        }
        "binary_expression" => {
            // Leftmost operand of a `+` chain (`"./a/" + x + y` parses left-nested).
            let mut left = arg;
            while left.kind() == "binary_expression" {
                if left.child_by_field_name("operator").map(|o| text(o, src)) != Some("+") {
                    return None;
                }
                left = left.child_by_field_name("left")?;
            }
            if left.kind() != "string" {
                return None;
            }
            string_literal_value(left, src)?
        }
        _ => return None,
    };

    if !(prefix.starts_with("./") || prefix.starts_with("../") || prefix.starts_with('/')) {
        return None;
    }
    let dir_spec = &prefix[..prefix.rfind('/')? + 1];
    let dir =
        kndo_adapter_toolkit::paths::join(kndo_adapter_toolkit::paths::dirname(path), dir_spec);
    (!dir.is_empty()).then(|| SmolStr::new(dir))
}

/// The `require("literal")` import itself — `certain` (spec §3), bindings from the enclosing
/// context, mirroring what `collect_import_bindings` does for ESM clauses:
/// - `const x = require("./y")` → one binding `{local: x, imported: None}` — the whole
///   `module.exports` value, which is exactly what the synthetic `default` name means on the
///   target side (both for a CJS target's `module.exports = …` and an ESM target's default).
/// - `const {a, b: c} = require("./y")` → named bindings, `pair_pattern`'s `key` being the
///   target's exported name and `value` the local — the CJS mirror of `import {a, b as c}`.
/// - Statement-level bare `require("./y")` → `side_effect_only`, same as `import "./y"`.
/// - Any other context (argument position, ternary arm, …) → the import edge alone, no
///   bindings — same stance as `import * as ns` (the edge is what reachability needs).
fn handle_literal_require(node: Node, string_node: Node, src: &[u8], out: &mut FileFacts) {
    let Some(specifier) = string_literal_value(string_node, src) else {
        return;
    };
    let kind = import_kind(&specifier);
    let parent = node.parent();
    let side_effect_only = parent.is_some_and(|p| p.kind() == "expression_statement");
    // `module.exports = require("./x")` — the CJS barrel, mirror of `export * from`:
    // the target's whole-module value becomes this file's own export surface, so it's
    // a re-export binding the synthetic `default` on both sides. Deliberately at *any*
    // nesting depth (unlike declaration extraction, which stays top-level): a
    // conditional `if (…) module.exports = require("./a") else … ("./b")` aliases both
    // targets — over-approximation in the keep-alive direction, the same doctrine the
    // reference walk documents (collecting too much fails safe; excluding risks
    // marking genuinely-used code unused).
    let reexported = parent.is_some_and(|p| {
        p.kind() == "assignment_expression"
            && p.child_by_field_name("left")
                .is_some_and(|l| is_module_exports(l, src))
    });
    let bindings = if reexported {
        vec![ImportBinding {
            local: SmolStr::new("default"),
            imported: None,
        }]
    } else {
        parent
            .filter(|p| p.kind() == "variable_declarator")
            .and_then(|p| p.child_by_field_name("name"))
            .map(|pattern| collect_require_bindings(pattern, src))
            .unwrap_or_default()
    };
    out.imports.push(RawImport {
        specifier,
        kind,
        span: span(node),
        side_effect_only,
        type_only: false,
        confidence: Confidence::Certain,
        bindings,
        reexported,
    });
}

fn collect_require_bindings(pattern: Node, src: &[u8]) -> Vec<ImportBinding> {
    match pattern.kind() {
        "identifier" => vec![ImportBinding {
            local: SmolStr::new(text(pattern, src)),
            imported: None,
        }],
        "object_pattern" => {
            let mut bindings = Vec::new();
            let mut cursor = pattern.walk();
            for entry in pattern.children(&mut cursor) {
                match entry.kind() {
                    "shorthand_property_identifier_pattern" => {
                        let name = text(entry, src);
                        bindings.push(ImportBinding {
                            local: SmolStr::new(name),
                            imported: Some(SmolStr::new(name)),
                        });
                    }
                    "pair_pattern" => {
                        let key = entry.child_by_field_name("key");
                        let value = entry.child_by_field_name("value");
                        if let (Some(key), Some(value)) = (key, value) {
                            // Nested destructuring (`{a: {b}}`) and defaults skipped — only a
                            // plain identifier value is a resolvable module-level binding.
                            if key.kind() == "property_identifier" && value.kind() == "identifier" {
                                bindings.push(ImportBinding {
                                    local: SmolStr::new(text(value, src)),
                                    imported: Some(SmolStr::new(text(key, src))),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            bindings
        }
        // Array patterns etc. — the import edge still lands, only the per-name fact is missed.
        _ => Vec::new(),
    }
}

/// CJS export surface, top-level statements only (spec §2): `module.exports = …` and
/// `exports.foo = …` / `module.exports.foo = …`. Conditional exports inside blocks
/// (`if (x) module.exports = …`) are dynamic behavior — deferred with the rest of the
/// dynamic-constructs table, not silently treated as unconditional.
///
/// Two distinct outcomes, chosen per assignment:
/// - **Mark an existing declaration exported** when the export names one (`exports.foo = foo`,
///   `module.exports = {a, b}` shorthand, `exports.pub = localName`) — the declaration *is*
///   the exported thing; fabricating a second symbol for it would leave one of the two
///   unreferenced and falsely `unused`. When the external name differs from the local one
///   (`exports.pub = localName`), the local is what gets marked — the alias fact itself is the
///   same gap as ESM's local `export { a as b }`, deferred with it.
/// - **Declare a new exported symbol** otherwise: `module.exports = <expr>` declares the
///   synthetic `default` (the name CJS interop binds — same convention as ESM anonymous
///   default exports), and `exports.foo = <non-identifier>` declares `foo` (kind by the
///   value's shape).
///
/// Spec §2's "computed/spread members demote the file's export surface to `probable`" has no
/// mechanism yet — `Declaration` carries no confidence and `FileFacts::dynamics` isn't
/// consumed by assembly — so those members contribute nothing for now, deferred alongside the
/// wildcard wiring rather than modeled wrong.
fn collect_cjs_exports(root: Node, src: &[u8], out: &mut FileFacts) {
    let mut cursor = root.walk();
    for statement in root.children(&mut cursor) {
        if statement.kind() != "expression_statement" {
            continue;
        }
        let Some(assignment) = statement.named_child(0) else {
            continue;
        };
        if assignment.kind() != "assignment_expression" {
            continue;
        }
        let (Some(left), Some(right)) = (
            assignment.child_by_field_name("left"),
            assignment.child_by_field_name("right"),
        ) else {
            continue;
        };

        if is_module_exports(left, src) {
            handle_cjs_module_exports(assignment, right, src, out);
        } else if let Some(name) = cjs_named_export(left, src) {
            handle_cjs_named_export(name, assignment, right, src, out);
        }
    }
}

/// `module.exports` — the whole-module export target.
fn is_module_exports(node: Node, src: &[u8]) -> bool {
    node.kind() == "member_expression"
        && node
            .child_by_field_name("object")
            .is_some_and(|o| o.kind() == "identifier" && text(o, src) == "module")
        && node
            .child_by_field_name("property")
            .is_some_and(|p| text(p, src) == "exports")
}

/// `exports.<name>` or `module.exports.<name>` — a single named export. Returns the name.
fn cjs_named_export(node: Node, src: &[u8]) -> Option<SmolStr> {
    if node.kind() != "member_expression" {
        return None;
    }
    let object = node.child_by_field_name("object")?;
    let is_exports_object = (object.kind() == "identifier" && text(object, src) == "exports")
        || is_module_exports(object, src);
    if !is_exports_object {
        return None;
    }
    let property = node.child_by_field_name("property")?;
    (property.kind() == "property_identifier").then(|| SmolStr::new(text(property, src)))
}

fn handle_cjs_module_exports(assignment: Node, right: Node, src: &[u8], out: &mut FileFacts) {
    // `module.exports = require("./x")` — the CJS barrel. collect_requires owns it (a
    // re-exported import binding `default` straight through to the target); synthesizing a
    // local `default` declaration here too would shadow that alias with a symbol nothing
    // ever references.
    if is_require_call(right, src) {
        return;
    }
    if right.kind() == "object" {
        // `module.exports = { a, b: localB, … }` — identifier members export those local
        // declarations (spec §2: certain).
        let mut cursor = right.walk();
        for member in right.children(&mut cursor) {
            let local = match member.kind() {
                "shorthand_property_identifier" => Some(SmolStr::new(text(member, src))),
                "pair" => member
                    .child_by_field_name("value")
                    .filter(|v| v.kind() == "identifier")
                    .map(|v| SmolStr::new(text(v, src))),
                _ => None, // computed/spread/literal members — no mechanism yet, see above
            };
            if let Some(local) = local {
                mark_declaration_exported(&local, out);
            }
        }
        return;
    }
    // `module.exports = localThing` — the local declaration *is* the export: mark it, no
    // synthetic symbol. A synthetic `default` alongside it would be a second symbol for the
    // same value, and whichever of the two nobody happens to reference would read as a
    // false-positive `unused` (found dogfooding against debug-js/debug: `module.exports =
    // setup` produced a phantom dead `default` next to a live `setup`). A whole-module
    // consumer's `default` binding then simply finds no symbol — the file edge still lands,
    // and the local stays alive through the reference this very assignment's RHS contributes.
    if right.kind() == "identifier"
        && mark_declaration_exported(&SmolStr::new(text(right, src)), out)
    {
        return;
    }
    // `module.exports = <anonymous expr>` — the synthetic `default`, exactly the name a
    // consumer's whole-module binding (`const x = require(…)` / `import x from`) looks up;
    // same convention as ESM anonymous default exports.
    let kind = match right.kind() {
        "class" => SymbolKind::Class,
        "function_expression" | "arrow_function" | "generator_function" => SymbolKind::Function,
        _ => SymbolKind::Const,
    };
    out.declarations.push(Declaration {
        name: SmolStr::new("default"),
        kind,
        span: span(assignment),
        exported: true,
        visibility: visibility(true),
    });
}

fn handle_cjs_named_export(
    name: SmolStr,
    assignment: Node,
    right: Node,
    src: &[u8],
    out: &mut FileFacts,
) {
    // In declaration-priority order: the export names an existing declaration
    // (`exports.foo = …` with `function foo` present), the value is one
    // (`exports.pub = localName`), or nothing local matches and the assignment itself is
    // the declaration (`exports.foo = function () {}`).
    if mark_declaration_exported(&name, out) {
        return;
    }
    if right.kind() == "identifier"
        && mark_declaration_exported(&SmolStr::new(text(right, src)), out)
    {
        return;
    }
    let kind = match right.kind() {
        "class" => SymbolKind::Class,
        "function_expression" | "arrow_function" | "generator_function" => SymbolKind::Function,
        _ => SymbolKind::Const,
    };
    out.declarations.push(Declaration {
        name,
        kind,
        span: span(assignment),
        exported: true,
        visibility: visibility(true),
    });
}

fn mark_declaration_exported(name: &SmolStr, out: &mut FileFacts) -> bool {
    let mut found = false;
    for decl in out.declarations.iter_mut().filter(|d| &d.name == name) {
        decl.exported = true;
        decl.visibility = visibility(true);
        found = true;
    }
    found
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

    // ---------------------------------------------------------------- CJS requires

    #[test]
    fn whole_module_require_binds_as_default_and_declares_nothing() {
        let facts = extract("f.js", b"const whole = require('./lib');");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert_eq!(imp.specifier.as_str(), "./lib");
        assert_eq!(imp.kind, ImportKind::Relative);
        assert_eq!(imp.confidence, Confidence::Certain);
        assert!(!imp.side_effect_only);
        assert_eq!(imp.bindings.len(), 1);
        assert_eq!(imp.bindings[0].local.as_str(), "whole");
        assert_eq!(imp.bindings[0].imported, None);
        // `whole` is an import binding, not a declaration — same as `import whole from`.
        assert!(facts.declarations.is_empty());
    }

    #[test]
    fn destructured_require_binds_named_with_rename() {
        let facts = extract("f.js", b"const { a, b: renamed } = require('./lib');");
        let imp = &facts.imports[0];
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
                ("renamed".into(), Some("b".into())),
            ]
        );
        assert!(facts.declarations.is_empty());
    }

    #[test]
    fn bare_require_statement_is_side_effect_only() {
        let facts = extract("f.js", b"require('./polyfill');");
        let imp = &facts.imports[0];
        assert!(imp.side_effect_only);
        assert!(imp.bindings.is_empty());
    }

    #[test]
    fn nested_require_still_produces_an_import() {
        let facts = extract(
            "f.js",
            b"function lazy() { const dep = require('./heavy'); return dep; }",
        );
        assert_eq!(facts.imports.len(), 1);
        assert_eq!(facts.imports[0].specifier.as_str(), "./heavy");
        assert_eq!(facts.imports[0].bindings[0].local.as_str(), "dep");
    }

    #[test]
    fn bare_package_require_is_a_package_import() {
        let facts = extract("f.js", b"const _ = require('lodash');");
        assert_eq!(facts.imports[0].kind, ImportKind::Package);
    }

    #[test]
    fn non_literal_require_is_deferred_not_guessed() {
        let facts = extract("f.js", b"const dyn = require(someVariable);");
        assert!(facts.imports.is_empty());
        // With no recognized require value, the declarator is an ordinary declaration again.
        assert_eq!(facts.declarations.len(), 1);
    }

    #[test]
    fn require_lookalikes_are_not_imports() {
        // (`require.resolve` is deliberately absent here — it IS an import, at probable;
        // see require_resolve_literal_is_a_probable_import.)
        let facts = extract("f.js", b"requireAll('./x'); obj.require('./z');");
        assert!(facts.imports.is_empty());
    }

    #[test]
    fn exported_const_require_keeps_its_declaration_and_the_import() {
        let facts = extract("f.ts", b"export const x = require('./y');");
        assert_eq!(facts.imports.len(), 1);
        assert_eq!(facts.declarations.len(), 1);
        assert!(facts.declarations[0].exported);
    }

    // ---------------------------------------------------------------- CJS export surface

    fn exported_names(facts: &FileFacts) -> Vec<(String, bool)> {
        facts
            .declarations
            .iter()
            .map(|d| (d.name.to_string(), d.exported))
            .collect()
    }

    #[test]
    fn module_exports_object_marks_shorthand_and_identifier_members_exported() {
        let facts = extract(
            "f.js",
            b"function f() {}\nconst localG = 1;\nmodule.exports = { f, g: localG, computed: 1 };",
        );
        assert_eq!(
            exported_names(&facts),
            vec![("f".into(), true), ("localG".into(), true)]
        );
    }

    #[test]
    fn module_exports_expression_declares_the_synthetic_default() {
        let facts = extract("f.js", b"module.exports = function main() {};");
        assert_eq!(exported_names(&facts), vec![("default".into(), true)]);
        assert_eq!(facts.declarations[0].kind, SymbolKind::Function);
    }

    #[test]
    fn module_exports_identifier_marks_the_local_without_a_phantom_default() {
        // A synthetic `default` next to the marked local would be a second symbol for the
        // same value — whichever one nobody references would false-positive as unused.
        let facts = extract("f.js", b"function run() {}\nmodule.exports = run;");
        assert_eq!(exported_names(&facts), vec![("run".into(), true)]);
    }

    #[test]
    fn module_exports_unknown_identifier_still_declares_default() {
        // The RHS names nothing this file declares (e.g. an imported binding) — the synthetic
        // default is then the only record that this module exports *something*.
        let facts = extract("f.js", b"module.exports = somethingImported;");
        assert_eq!(exported_names(&facts), vec![("default".into(), true)]);
    }

    #[test]
    fn exports_dot_name_with_a_function_value_declares_an_exported_symbol() {
        let facts = extract("f.js", b"exports.foo = function () {};");
        assert_eq!(exported_names(&facts), vec![("foo".into(), true)]);
        assert_eq!(facts.declarations[0].kind, SymbolKind::Function);
    }

    #[test]
    fn exports_dot_name_matching_a_local_declaration_marks_it_instead_of_duplicating() {
        let facts = extract("f.js", b"function foo() {}\nexports.foo = foo;");
        assert_eq!(exported_names(&facts), vec![("foo".into(), true)]);
    }

    #[test]
    fn exports_assignment_before_the_declaration_still_marks_it() {
        // Hoisting: the export statement can textually precede the function it exports.
        let facts = extract("f.js", b"exports.foo = foo;\nfunction foo() {}");
        assert_eq!(exported_names(&facts), vec![("foo".into(), true)]);
    }

    #[test]
    fn exports_dot_name_aliasing_a_local_marks_the_local() {
        let facts = extract(
            "f.js",
            b"function internalName() {}\nexports.pub = internalName;",
        );
        assert_eq!(exported_names(&facts), vec![("internalName".into(), true)]);
    }

    #[test]
    fn module_exports_dot_name_works_like_exports_dot_name() {
        let facts = extract("f.js", b"module.exports.baz = 42;");
        assert_eq!(exported_names(&facts), vec![("baz".into(), true)]);
        assert_eq!(facts.declarations[0].kind, SymbolKind::Const);
    }

    #[test]
    fn conditional_module_exports_is_deferred_not_treated_as_unconditional() {
        let facts = extract("f.js", b"if (flag) { module.exports = function () {}; }");
        assert!(facts.declarations.is_empty());
    }

    #[test]
    fn unrelated_member_assignment_is_not_an_export() {
        let facts = extract("f.js", b"obj.exports = 1;\nthing.foo = 2;");
        assert!(facts.declarations.is_empty());
    }

    #[test]
    fn module_exports_require_is_a_cjs_barrel_reexport() {
        let facts = extract("f.js", b"module.exports = require('./impl');");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert!(imp.reexported);
        assert_eq!(imp.bindings.len(), 1);
        assert_eq!(imp.bindings[0].local.as_str(), "default");
        assert_eq!(imp.bindings[0].imported, None);
        // The re-export alias owns `default` — no synthetic local declaration to shadow it.
        assert!(facts.declarations.is_empty());
    }

    #[test]
    fn conditional_module_exports_require_still_reexports_both_branches() {
        // Over-approximation in the keep-alive direction: both branches' targets stay part of
        // this file's surface (one wins at runtime, but "possibly exported" must never read
        // as dead).
        let facts = extract(
            "f.js",
            b"if (isBrowser) { module.exports = require('./browser'); } else { module.exports = require('./node'); }",
        );
        assert_eq!(facts.imports.len(), 2);
        assert!(facts.imports.iter().all(|i| i.reexported));
    }

    #[test]
    fn require_call_wrapped_in_an_invocation_is_not_a_barrel() {
        // `module.exports = require('./common')(exports)` — the require is in function
        // position of an outer call; the module's export is the *call result*, so the
        // synthetic default belongs to this file, and the import is a plain edge.
        let facts = extract("f.js", b"module.exports = require('./common')(exports);");
        assert_eq!(facts.imports.len(), 1);
        assert!(!facts.imports[0].reexported);
        assert_eq!(exported_names(&facts), vec![("default".into(), true)]);
    }

    // ---------------------------------------------------------------- dynamic constructs

    #[test]
    fn literal_dynamic_import_is_a_probable_import() {
        let facts = extract("f.ts", b"async function f() { return import('./lazy'); }");
        assert_eq!(facts.imports.len(), 1);
        let imp = &facts.imports[0];
        assert_eq!(imp.specifier.as_str(), "./lazy");
        assert_eq!(imp.confidence, Confidence::Probable);
        assert!(imp.bindings.is_empty());
        assert!(facts.dynamics.is_empty());
    }

    #[test]
    fn non_literal_dynamic_import_is_a_dynamic_use() {
        let facts = extract("f.ts", b"async function f(m) { return import(m); }");
        assert!(facts.imports.is_empty());
        assert_eq!(facts.dynamics.len(), 1);
        assert_eq!(facts.dynamics[0].reason.as_str(), "non-literal import()");
        assert_eq!(facts.dynamics[0].narrowed_to, None);
    }

    #[test]
    fn template_prefix_narrows_to_the_project_relative_directory() {
        let facts = extract(
            "src/i18n/loader.ts",
            b"export function load(lang) { return import(`./locales/${lang}.json`); }",
        );
        assert_eq!(facts.dynamics.len(), 1);
        assert_eq!(
            facts.dynamics[0].narrowed_to.as_deref(),
            Some("src/i18n/locales")
        );
    }

    #[test]
    fn concatenation_prefix_narrows_too() {
        let facts = extract(
            "src/f.js",
            b"function load(name) { return require('../plugins/' + name); }",
        );
        assert_eq!(facts.dynamics[0].reason.as_str(), "non-literal require()");
        assert_eq!(facts.dynamics[0].narrowed_to.as_deref(), Some("plugins"));
    }

    #[test]
    fn package_name_prefix_does_not_narrow() {
        // A non-relative prefix can't name project files — wildcard stays un-narrowed.
        let facts = extract("f.js", b"const m = require(`lodash/${fn}`);");
        assert_eq!(facts.dynamics.len(), 1);
        assert_eq!(facts.dynamics[0].narrowed_to, None);
    }

    #[test]
    fn template_starting_with_a_substitution_does_not_narrow() {
        let facts = extract("f.js", b"const m = require(`${base}/thing`);");
        assert_eq!(facts.dynamics[0].narrowed_to, None);
    }

    #[test]
    fn direct_eval_and_new_function_are_dynamic_uses_but_indirect_eval_is_not() {
        let facts = extract(
            "f.js",
            b"eval('code');\nconst f = new Function('return 1');\nwindow.eval('indirect');",
        );
        let reasons: Vec<&str> = facts.dynamics.iter().map(|d| d.reason.as_str()).collect();
        assert_eq!(reasons, vec!["eval", "new Function"]);
    }

    #[test]
    fn require_resolve_literal_is_a_probable_import() {
        let facts = extract("f.js", b"const p = require.resolve('./config');");
        assert_eq!(facts.imports.len(), 1);
        assert_eq!(facts.imports[0].confidence, Confidence::Probable);
        assert_eq!(facts.imports[0].specifier.as_str(), "./config");
        assert!(facts.dynamics.is_empty());
    }
}

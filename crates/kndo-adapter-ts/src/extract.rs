//! Extraction: one export pre-scan, one declaration pass over top-level statements
//! (descending into class bodies), one import pass, and one full-tree walk for
//! references and comments. Everything ambiguous degrades toward keep-alive: a name
//! we cannot classify is a use, a binding we cannot name is never declared (and so
//! never accused).

use kndo_contract::evidence::{
    DeclarationId, EvidenceSink, ImportBinding, ImportShape, ImportTarget, Reach, RefKind,
    RegionMode, SymbolKind, Timing,
};
use kndo_contract::vocab::{Confidence, Span};
use kndo_toolkit as tk;
use smol_str::SmolStr;
use std::collections::BTreeMap;
use tree_sitter::Node;

/// `mode` is the embedded region's, when the source is one: a classic
/// script's top-level declarations are the page's globals — reachable from
/// every other script and handler attribute on it — so they are exported
/// rather than private to the region.
pub fn extract(
    source: &[u8],
    tree: &tree_sitter::Tree,
    mode: Option<RegionMode>,
    out: &mut EvidenceSink,
) {
    let root = tree.root_node();
    let globals = mode == Some(RegionMode::Script);
    // The `@generated`/`DO NOT EDIT` convention (graphql-codegen, protobuf):
    // generated code is the generator's business — it declares nothing
    // accusable and the FILE is the generator's output, rooted Tooling so it is
    // never accused of being unimported; its imports and references still keep
    // the rest of the project alive. Same needles as the JVM adapters, whose
    // library-mode roots already cover the file half.
    if tk::generated_marked(source, tk::GENERATED_NEEDLES, &["//", "/*", "*"]) {
        out.root(
            kndo_contract::evidence::RootTarget::WholeFile,
            kndo_contract::evidence::RootKind::Tooling,
            kndo_contract::vocab::Confidence::Probable,
        );
    } else {
        let aliases = export_aliases(root, source);
        declarations(root, source, &aliases, globals, out);
    }
    imports(root, source, out);
    literal_specifiers(root, source, out);
    references_and_comments(root, source, out);
}

/// Package specifiers spelled inside string and template literals — the code
/// a build plugin injects (`\`import "systemjs/dist/s.min.js"\``), a
/// `createRequire`d path (`_require('core-js/package.json')`), a polyfill
/// list. Each lands as a `Possible` side-effect import of the package it
/// spells: enough for a declared dependency to count as used (silence over
/// accusation), too weak to resolve to a sibling or to be judged unresolved.
/// A token qualifies when it is specifier-shaped and either carries a path
/// separator or is the literal's whole text — a bare word inside prose is not
/// a package.
fn literal_specifiers(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        // Strings among the direct children emit in source order; containers
        // are pushed reversed so the stack pops them in source order too.
        for child in &children {
            if matches!(child.kind(), "string" | "template_string") {
                literal_tokens(*child, source, &mut seen, out);
            }
        }
        for child in children.into_iter().rev() {
            match child.kind() {
                // Sources of import/export statements and `require`/`import()`
                // arguments are imports already, at their own confidence.
                "import_statement" | "export_statement" => {}
                "call_expression" if is_require_or_import(child, source) => {}
                "string" | "template_string" => {}
                _ => pending.push(child),
            }
        }
    }
}

fn literal_tokens(
    literal: Node<'_>,
    source: &[u8],
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut EvidenceSink,
) {
    let mut c = literal.walk();
    for frag in literal.named_children(&mut c) {
        if frag.kind() != "string_fragment" {
            continue;
        }
        let text = tk::text(frag, source);
        let separators =
            |ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '`' | ';' | ',' | '(' | ')');
        for token in text.split(separators) {
            if !specifier_shaped(token) || (!token.contains('/') && token != text.trim()) {
                continue;
            }
            if seen.insert(token.to_string()) {
                out.import(
                    ImportTarget::Package(SmolStr::new(token)),
                    ImportShape::Mention,
                    tk::span(literal),
                    Confidence::Possible,
                );
            }
        }
    }
}

fn is_require_or_import(call: Node<'_>, source: &[u8]) -> bool {
    call.child_by_field_name("function")
        .is_some_and(|f| matches!(tk::text(f, source), "require" | "import"))
}

/// A bare npm specifier: an optional `@scope/`, then lowercase/digit-led
/// segments of `[a-z0-9._-]`, `/`-separated — never a relative or absolute
/// path, never a URL.
fn specifier_shaped(token: &str) -> bool {
    let body = token.strip_prefix('@').unwrap_or(token);
    let first = body.chars().next();
    first.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && body.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-' | '/')
        })
        && !body.contains("//")
        && !body.ends_with('/')
        && (!token.starts_with('@') || body.contains('/'))
}

/// Local name → the name the module system exports it under, from clause exports
/// without a source (`export { local }`, `export { local as alias }`). Declarations
/// exported in place (`export const x`) never appear here — their exported name IS
/// their local name.
fn export_aliases(root: Node<'_>, source: &[u8]) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        if stmt.kind() != "export_statement" || stmt.child_by_field_name("source").is_some() {
            continue;
        }
        let Some(clause) = tk::child_of_kind(stmt, "export_clause") else {
            continue;
        };
        let mut c = clause.walk();
        for spec in clause.named_children(&mut c) {
            if spec.kind() != "export_specifier" {
                continue;
            }
            let Some(name) = spec.child_by_field_name("name") else {
                continue;
            };
            let local = tk::text(name, source).to_string();
            let exported = spec
                .child_by_field_name("alias")
                .map(|a| tk::text(a, source).to_string())
                .unwrap_or_else(|| local.clone());
            aliases.entry(local).or_insert(exported);
        }
    }
    aliases
}

fn declarations(
    root: Node<'_>,
    source: &[u8],
    aliases: &BTreeMap<String, String>,
    globals: bool,
    out: &mut EvidenceSink,
) {
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        if stmt.kind() == "export_statement" {
            if let Some(decl) = stmt.child_by_field_name("declaration") {
                let is_default = has_token(stmt, "default");
                declare(decl, source, Some(is_default), aliases, globals, out);
            }
        } else {
            declare(stmt, source, None, aliases, globals, out);
        }
    }
}

/// `export_default` is `None` for a plain top-level statement, `Some(is_default)` for
/// a declaration inside an `export_statement`; `globals` says every top-level
/// declaration is reachable beyond the source (a classic script's).
fn declare(
    node: Node<'_>,
    source: &[u8],
    export_default: Option<bool>,
    aliases: &BTreeMap<String, String>,
    globals: bool,
    out: &mut EvidenceSink,
) {
    let emit = |name: &str, kind: SymbolKind, span: Span, out: &mut EvidenceSink| {
        let in_export = export_default.is_some();
        let clause_alias = aliases.get(name);
        let reach = if globals || in_export || clause_alias.is_some() {
            Reach::Exported
        } else {
            Reach::File
        };
        let id = out.declaration(name, kind, span, reach);
        if export_default == Some(true) {
            out.exported_as(id, "default");
        } else if let Some(alias) = clause_alias
            && alias != name
        {
            out.exported_as(id, alias.as_str());
        }
        id
    };

    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let id = emit(
                    tk::text(n, source),
                    SymbolKind::Function,
                    tk::span(node),
                    out,
                );
                out.metrics(id, function_metrics(node, source));
                // The promise region: name, parameters, return annotation —
                // everything before the body. Free callables only: a class or
                // interface has no body-distinct signature, and treating its
                // whole body as one would make every member reference a
                // "signature" reference.
                if let Some(body) = node.child_by_field_name("body") {
                    out.signature_span(
                        id,
                        Span {
                            start: node.start_byte() as u32,
                            end: body.start_byte() as u32,
                        },
                    );
                }
            }
        }
        "class_declaration" | "abstract_class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let class_id = emit(tk::text(n, source), SymbolKind::Type, tk::span(node), out);
                class_members(node, source, class_id, out);
            }
        }
        "interface_declaration" | "type_alias_declaration" | "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                emit(tk::text(n, source), SymbolKind::Type, tk::span(node), out);
            }
        }
        "internal_module" | "module" => {
            if let Some(n) = node.child_by_field_name("name") {
                emit(tk::text(n, source), SymbolKind::Module, tk::span(node), out);
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            let kind = if node.child(0).is_some_and(|c| c.kind() == "const") {
                SymbolKind::Constant
            } else {
                SymbolKind::Variable
            };
            let mut c = node.walk();
            for d in node.named_children(&mut c) {
                if d.kind() != "variable_declarator" {
                    continue;
                }
                let Some(n) = d.child_by_field_name("name") else {
                    continue;
                };
                // Destructuring patterns bind names this pass does not declare —
                // an undeclared binding can never be accused.
                if n.kind() == "identifier" {
                    let id = emit(tk::text(n, source), kind.clone(), tk::span(d), out);
                    // A function in const clothing is a function to the metrics.
                    if let Some(value) = d.child_by_field_name("value")
                        && matches!(
                            value.kind(),
                            "arrow_function" | "function_expression" | "generator_function"
                        )
                    {
                        out.metrics(id, function_metrics(value, source));
                    }
                }
            }
        }
        _ => {}
    }
}

/// Methods (and arrow-function fields, which are methods in practice) become member
/// declarations owned by their class. Constructors are skipped: a constructor's use
/// surface is the class name itself. Computed names are skipped — unnameable,
/// therefore unaccusable.
fn class_members(class: Node<'_>, source: &[u8], class_id: DeclarationId, out: &mut EvidenceSink) {
    let Some(body) = class.child_by_field_name("body") else {
        return;
    };
    let mut c = body.walk();
    for m in body.named_children(&mut c) {
        let is_method = match m.kind() {
            "method_definition" => true,
            "public_field_definition" => m.child_by_field_name("value").is_some_and(|v| {
                matches!(
                    v.kind(),
                    "arrow_function" | "function_expression" | "generator_function"
                )
            }),
            _ => false,
        };
        if !is_method {
            continue;
        }
        let Some(n) = m.child_by_field_name("name") else {
            continue;
        };
        if !matches!(
            n.kind(),
            "property_identifier" | "private_property_identifier"
        ) {
            continue;
        }
        let name = tk::text(n, source);
        if name == "constructor" {
            continue;
        }
        // Every member reads as its owner's alone: the modifier (`private`,
        // `#name`, `protected`, none) is the migration's to spell.
        let id = out.declaration(name, SymbolKind::Method, tk::span(m), Reach::Owner);
        out.member_of(id, class_id);
        out.metrics(id, function_metrics(m, source));
    }
}

/// Metrics over one function-shaped node. Leaves are normalized by class —
/// identifiers, strings and numbers collapse to their kind — so Type-2 clones
/// (renamed, re-valued) fingerprint identically; everything else keeps its literal
/// kind. Comments never count.
const METRICS: tk::MetricsSpec = tk::MetricsSpec {
    is_branch: |n, _| match n.kind() {
        "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
        | "do_statement" | "switch_case" | "catch_clause" | "ternary_expression" => true,
        // `??` is deliberately absent: value-producing, not a control fork —
        // the shared rule in the spec's contract.
        "binary_expression" => n
            .child_by_field_name("operator")
            .is_some_and(|op| matches!(op.kind(), "&&" | "||")),
        _ => false,
    },
    token_class: |n| match n.kind() {
        "identifier"
        | "property_identifier"
        | "private_property_identifier"
        | "type_identifier"
        | "shorthand_property_identifier"
        | "shorthand_property_identifier_pattern" => Some("id"),
        "string_fragment" => Some("str"),
        "number" => Some("num"),
        "comment" => None,
        other => Some(other),
    },
};

fn function_metrics(node: Node<'_>, source: &[u8]) -> kndo_contract::evidence::FunctionMetrics {
    tk::function_metrics(node, &METRICS, source)
}

fn imports(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        match stmt.kind() {
            "import_statement" => import_statement(stmt, source, out),
            "export_statement" if stmt.child_by_field_name("source").is_some() => {
                reexport_statement(stmt, source, out);
            }
            _ => {}
        }
    }
}

fn target_of(stmt: Node<'_>, source: &[u8]) -> Option<ImportTarget> {
    let src = stmt.child_by_field_name("source")?;
    let text = string_text(src, source);
    if text.is_empty() {
        return None;
    }
    Some(if text.starts_with('.') {
        ImportTarget::Relative(SmolStr::new(text))
    } else {
        ImportTarget::Package(SmolStr::new(text))
    })
}

fn string_text<'a>(string: Node<'_>, source: &'a [u8]) -> &'a str {
    tk::child_of_kind(string, "string_fragment")
        .map(|f| tk::text(f, source))
        .unwrap_or("")
}

fn has_token(node: Node<'_>, token: &str) -> bool {
    let mut c = node.walk();
    node.children(&mut c).any(|ch| ch.kind() == token)
}

fn import_statement(stmt: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let Some(target) = target_of(stmt, source) else {
        // `import x = require(...)` and friends carry no `source` field; nothing is
        // recorded, and an unrecorded import can only under-connect, never accuse.
        return;
    };
    let type_only = has_token(stmt, "type");
    let shape = match tk::child_of_kind(stmt, "import_clause") {
        None => ImportShape::SideEffect,
        Some(clause) => {
            let mut bindings = Vec::new();
            let mut namespace: Option<SmolStr> = None;
            let mut c = clause.walk();
            for ch in clause.named_children(&mut c) {
                match ch.kind() {
                    "identifier" => bindings.push(ImportBinding {
                        imported: SmolStr::new_static("default"),
                        local: SmolStr::new(tk::text(ch, source)),
                    }),
                    "named_imports" => {
                        let mut sc = ch.walk();
                        for spec in ch.named_children(&mut sc) {
                            if spec.kind() != "import_specifier" {
                                continue;
                            }
                            let Some(name) = spec.child_by_field_name("name") else {
                                continue;
                            };
                            let imported = SmolStr::new(tk::text(name, source));
                            let local = spec
                                .child_by_field_name("alias")
                                .map(|a| SmolStr::new(tk::text(a, source)))
                                .unwrap_or_else(|| imported.clone());
                            bindings.push(ImportBinding { imported, local });
                        }
                    }
                    "namespace_import" => {
                        namespace = tk::child_of_kind(ch, "identifier")
                            .map(|n| SmolStr::new(tk::text(n, source)));
                    }
                    _ => {}
                }
            }
            match namespace {
                // A namespace binding keeps the target's whole surface, which
                // subsumes any default binding beside it.
                Some(local) => ImportShape::Namespace { local },
                None => ImportShape::Bindings(bindings),
            }
        }
    };
    // `import type` is erased by every emitter, so it can never take part in
    // an initialization cycle; an inline `type` specifier leaves the statement
    // itself in place (`import {} from "x"` still loads "x"), so it stays a
    // load-time import.
    let timing = if type_only {
        Timing::Erased
    } else {
        Timing::Load
    };
    out.import_at(timing, target, shape, tk::span(stmt), Confidence::Certain);
}

fn reexport_statement(stmt: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let Some(target) = target_of(stmt, source) else {
        return;
    };
    let star = has_token(stmt, "*") || tk::child_of_kind(stmt, "namespace_export").is_some();
    let shape = if star {
        ImportShape::ReexportAll
    } else {
        let mut bindings = Vec::new();
        if let Some(clause) = tk::child_of_kind(stmt, "export_clause") {
            let mut c = clause.walk();
            for spec in clause.named_children(&mut c) {
                if spec.kind() != "export_specifier" {
                    continue;
                }
                let Some(name) = spec.child_by_field_name("name") else {
                    continue;
                };
                let imported = SmolStr::new(tk::text(name, source));
                let local = spec
                    .child_by_field_name("alias")
                    .map(|a| SmolStr::new(tk::text(a, source)))
                    .unwrap_or_else(|| imported.clone());
                bindings.push(ImportBinding { imported, local });
            }
        }
        ImportShape::Reexport(bindings)
    };
    out.import(target, shape, tk::span(stmt), Confidence::Certain);
}

/// Identifier-ish node kinds that can be uses. Everything else never becomes a
/// reference.
const IDENT_KINDS: [&str; 5] = [
    "identifier",
    "type_identifier",
    "property_identifier",
    "private_property_identifier",
    "shorthand_property_identifier",
];

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    tk::walk(root, &mut |n| {
        if n.kind() == "comment" {
            tk::comment_evidence(n, source, &COMMENT_MARKERS, out);
            return;
        }
        if n.kind() == "call_expression" {
            dynamic_import(n, source, out);
            return;
        }
        if !IDENT_KINDS.contains(&n.kind()) {
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

/// `require("...")` and `import("...")` with a literal argument are imports, at any
/// depth. A `require` bound to a name is a namespace binding of the whole module;
/// anything else keeps the target's surface without naming a binding. Non-literal
/// arguments stay unrecorded — absence can under-connect, never accuse.
fn dynamic_import(call: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let Some(function) = call.child_by_field_name("function") else {
        return;
    };
    let is_require = function.kind() == "identifier" && tk::text(function, source) == "require";
    let is_import = function.kind() == "import";
    if !is_require && !is_import {
        return;
    }
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let mut c = args.walk();
    let literals: Vec<Node<'_>> = args.named_children(&mut c).collect();
    let [arg] = literals[..] else { return };
    if arg.kind() != "string" {
        return;
    }
    let text = string_text(arg, source);
    if text.is_empty() {
        return;
    }
    let target = if text.starts_with('.') {
        ImportTarget::Relative(SmolStr::new(text))
    } else {
        ImportTarget::Package(SmolStr::new(text))
    };
    let shape = match (is_require, call.parent()) {
        (true, Some(p)) if p.kind() == "variable_declarator" => {
            match p.child_by_field_name("name") {
                Some(name) if name.kind() == "identifier" => ImportShape::Namespace {
                    local: SmolStr::new(tk::text(name, source)),
                },
                _ => ImportShape::SideEffect,
            }
        }
        _ => ImportShape::SideEffect,
    };
    // A load the file itself makes conditional — inside a function, a branch,
    // a guard, a `||` fallback — is the optional-dependency idiom: `Probable`,
    // a use that keeps its target alive and never an accusation's ground.
    let conditional = is_conditional(call);
    let confidence = if conditional {
        Confidence::Probable
    } else {
        Confidence::Certain
    };
    // `import()` runs when evaluated, after the static graph linked; a
    // `require()` runs at load only at the top level of the module.
    let timing = if is_import || conditional {
        Timing::Lazy
    } else {
        Timing::Load
    };
    out.import_at(timing, target, shape, tk::span(call), confidence);
}

fn is_conditional(call: Node<'_>) -> bool {
    let mut node = call;
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "program" => return false,
            "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "class_body"
            | "if_statement"
            | "else_clause"
            | "try_statement"
            | "catch_clause"
            | "finally_clause"
            | "switch_statement"
            | "for_statement"
            | "for_in_statement"
            | "while_statement"
            | "do_statement"
            | "ternary_expression" => return true,
            "binary_expression"
                if parent
                    .child_by_field_name("operator")
                    .is_some_and(|op| matches!(op.kind(), "||" | "&&" | "??")) =>
            {
                return true;
            }
            _ => {}
        }
        node = parent;
    }
    false
}

/// Binding and naming positions are not uses. The bias is deliberate: excluding too
/// little keeps something alive; excluding too much accuses it — so only positions
/// that are unambiguously bindings are excluded.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        // Import/export machinery binds or renames; it does not use. (Clause exports
        // feed reach and aliases instead — counting them as uses would keep every
        // exported symbol alive in its own file.)
        "import_specifier"
        | "namespace_import"
        | "import_clause"
        | "export_specifier"
        | "namespace_export"
        | "import_require_clause" => false,
        // Object-literal and pattern keys name, they don't use.
        "pair" | "pair_pattern" => parent.child_by_field_name("key") != Some(n),
        // Parameters bind.
        "required_parameter" | "optional_parameter" => {
            parent.child_by_field_name("pattern") != Some(n)
        }
        // Bare enum members declare.
        "enum_body" => false,
        // A declaration's own name declares.
        k => {
            let declares = matches!(
                k,
                "function_declaration"
                    | "generator_function_declaration"
                    | "class_declaration"
                    | "abstract_class_declaration"
                    | "interface_declaration"
                    | "type_alias_declaration"
                    | "enum_declaration"
                    | "internal_module"
                    | "module"
                    | "variable_declarator"
                    | "method_definition"
                    | "public_field_definition"
                    | "property_signature"
                    | "method_signature"
                    | "enum_assignment"
                    | "function_expression"
                    | "class_expression"
            );
            !(declares && parent.child_by_field_name("name") == Some(n))
        }
    }
}

fn classify(n: Node<'_>, parent: Node<'_>) -> RefKind {
    if parent.kind() == "call_expression" && parent.child_by_field_name("function") == Some(n) {
        return RefKind::Call;
    }
    if parent.kind() == "new_expression" && parent.child_by_field_name("constructor") == Some(n) {
        return RefKind::Call;
    }
    if parent.kind() == "member_expression" && parent.child_by_field_name("property") == Some(n) {
        let called = parent.parent().is_some_and(|gp| {
            gp.kind() == "call_expression" && gp.child_by_field_name("function") == Some(parent)
        });
        return if called { RefKind::Call } else { RefKind::Read };
    }
    match parent.kind() {
        "extends_clause" | "class_heritage" | "extends_type_clause" => RefKind::Extend,
        "implements_clause" => RefKind::Implement,
        _ if n.kind() == "type_identifier" => RefKind::TypeUse,
        _ => RefKind::Read,
    }
}

/// JS/TS comment markers; JSDoc's `/**` strips to its text so a pragma inside
/// one parses like any other.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"",
    block_doc: b"*",
};

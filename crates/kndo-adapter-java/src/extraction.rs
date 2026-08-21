//! Java extraction (docs/adapters/java.md §2), written against tree-sitter-java 0.23.5 node
//! shapes verified by `parsing`'s `#[ignore]`d ground-truth dumps.
//!
//! Shapes that matter here and nowhere else:
//! - A top-level/nested type's members share one visibility ladder with the type itself
//!   (spec §0): `[File "private", Package "package-private", Public "protected", Public
//!   "public"]` — `protected` widens to `Public` because cross-package subclass access can't
//!   be ruled out without a typechecker.
//! - `@Override` methods root `Production`/`Probable` unconditionally (spec §0's dispatch
//!   rule — JDK-invoked contract methods like `equals`/`hashCode`/`toString` are never called
//!   by a named site in user source).
//! - A wildcard type import (`import p.*;`) and a wildcard static import
//!   (`import static p.C.*;`) both set `opaque_namespace_use: true` — the core's existing
//!   "Wildcard over the resolved target's symbols" mechanism, exact here (not approximate)
//!   because a Java package's declared types are fully enumerable.
//! - Records extract as a type declaration; their compiler-synthesized accessor methods are
//!   not modeled (spec §2/§5) — a documented recall gap, same class as JS's property-
//!   assignment-callable limitation.

use kndo_adapter_toolkit::metrics::{function_shape, MetricsSyntax};
use kndo_core::adapter::{
    Diagnostic, DiagnosticLevel, FileFacts, FunctionMetrics, ImportBinding, ImportKind, RawImport,
    RawReference, RawRoot, RawRootTarget, RawSuppression, Span,
};
use kndo_core::vocab::{Confidence, RefKind, RootKind, SymbolKind};
use smol_str::SmolStr;
use tree_sitter::Node;

const GENERATED_MARKERS: kndo_adapter_toolkit::classify::ContentMarkers =
    kndo_adapter_toolkit::classify::ContentMarkers {
        generated_markers: &[
            kndo_adapter_toolkit::classify::LineMarker::Contains("@generated"),
            kndo_adapter_toolkit::classify::LineMarker::Contains("Code generated"),
            kndo_adapter_toolkit::classify::LineMarker::Contains("DO NOT EDIT"),
        ],
        scan_window_lines: 64,
        comment_openers: &["//", "/*", "*"],
    };

/// docs/adapters/java.md §2: ternary and `&&`/`||` count as branches same as every other
/// adapter; each `switch_block_statement_group` is one arm past the base (matching JS's
/// n-way-match rule).
pub const METRICS_SYNTAX: MetricsSyntax = MetricsSyntax {
    branch_kinds: &[
        "if_statement",
        "for_statement",
        "enhanced_for_statement",
        "while_statement",
        "do_statement",
        "catch_clause",
        "switch_block_statement_group",
        "ternary_expression",
        "&&",
        "||",
    ],
    identifier_kinds: &["identifier", "type_identifier"],
    literal_kinds: &[
        "string_literal",
        "character_literal",
        "decimal_integer_literal",
        "hex_integer_literal",
        "octal_integer_literal",
        "binary_integer_literal",
        "decimal_floating_point_literal",
        "hex_floating_point_literal",
        "true",
        "false",
        "null_literal",
    ],
    skip_kinds: &["line_comment", "block_comment"],
};

const MIN_CLONE_TOKENS: usize = 50;

/// Item-walk context: the member owner (a class/interface/enum/record's bare name), for
/// `member_of` attribution (RFC 0012 §3).
struct Ctx<'a> {
    owner: Option<&'a str>,
}

pub(crate) fn extract(_path: &str, content: &[u8]) -> FileFacts {
    let mut out = FileFacts::default();
    if GENERATED_MARKERS.detect_generated(content) {
        out.detected_origin = Some(kndo_core::vocab::FileOrigin::Generated);
    }

    let Some(tree) = crate::parsing::parse(content) else {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "failed to initialize the Java parser".to_string(),
            span: None,
        });
        return out;
    };
    let root = tree.root_node();
    if root.has_error() {
        out.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warn,
            path: None,
            message: "parse errors — extraction is partial for this file".to_string(),
            span: None,
        });
    }

    let mut cursor = root.walk();
    for item in root.children(&mut cursor) {
        handle_top_level(item, content, &mut out);
    }
    collect_suppressions(root, content, &mut out);
    out
}

/// `program`'s direct children: `package_declaration` (→ `FileFacts::unit`/`unit_name`),
/// `import_declaration`, and the top-level type declarations. `module_declaration` (JPMS,
/// spec §0) and `package_info` markers fall through unmatched — zero declarations, harmless.
fn handle_top_level(item: Node, src: &[u8], out: &mut FileFacts) {
    match item.kind() {
        "package_declaration" => {
            if let Some(name_node) = item.named_child(0) {
                let name = dotted_text(name_node, src);
                out.unit = Some(SmolStr::new(&name));
                out.unit_name = Some(SmolStr::new(&name));
            }
        }
        "import_declaration" => handle_import(item, src, out),
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration" => handle_type(item, src, &Ctx { owner: None }, out),
        _ => {}
    }
}

/// A `scoped_identifier`/`identifier` path's full dotted text — same string either shape
/// produces, since `scoped_identifier` nests `scope`/`name` recursively left-to-right.
fn dotted_text(node: Node, src: &[u8]) -> String {
    text(node, src).to_string()
}

fn handle_import(item: Node, src: &[u8], out: &mut FileFacts) {
    let mut c = item.walk();
    let is_static = item.children(&mut c).any(|n| n.kind() == "static");
    drop(c);
    let Some(path_node) = item
        .named_child(0)
        .filter(|n| matches!(n.kind(), "scoped_identifier" | "identifier"))
    else {
        return;
    };
    let mut c = item.walk();
    let is_wildcard = item.children(&mut c).any(|n| n.kind() == "asterisk");
    drop(c);
    let full = dotted_text(path_node, src);
    let sp = span(item);

    if is_static {
        // `import static com.foo.Bar.*;` — the asterisk is a separate sibling node (not
        // part of the scoped_identifier text), so `full` here is already just the class
        // path "com.foo.Bar" — one split gives package/class.
        //
        // `import static com.foo.Bar.CONST;` — `full` is "com.foo.Bar.CONST"; the member
        // name is the true last segment, so TWO splits are needed: member off first, then
        // package/class off what remains.
        if is_wildcard {
            let Some((pkg, cls)) = full.rsplit_once('.') else {
                return;
            };
            let specifier = format!("{pkg}::{cls}");
            out.imports
                .push(make_import(&specifier, sp, false, Vec::new(), true));
        } else {
            let Some((cls_path, member)) = full.rsplit_once('.') else {
                return;
            };
            let Some((pkg, cls)) = cls_path.rsplit_once('.') else {
                return;
            };
            let specifier = format!("{pkg}::{cls}");
            out.imports.push(make_import(
                &specifier,
                sp,
                false,
                vec![ImportBinding {
                    local: SmolStr::new(member),
                    imported: Some(SmolStr::new(member)),
                }],
                false,
            ));
        }
        return;
    }

    if is_wildcard {
        // `import com.foo.*;` — package-level wildcard, exactly enumerable (spec §3).
        out.imports
            .push(make_import(&full, sp, false, Vec::new(), true));
        return;
    }

    // `import com.foo.Bar;` — specifier is the PACKAGE half, binding is the type name
    // (spec §3): the grammar's own `scoped_identifier` nesting already gives the split for
    // free, no two-step tail-rule guessing needed.
    let Some((pkg, ty)) = full.rsplit_once('.') else {
        return;
    };
    out.imports.push(make_import(
        pkg,
        sp,
        false,
        vec![ImportBinding {
            local: SmolStr::new(ty),
            imported: Some(SmolStr::new(ty)),
        }],
        false,
    ));
}

fn make_import(
    specifier: &str,
    span: Span,
    side_effect_only: bool,
    bindings: Vec<ImportBinding>,
    opaque_namespace_use: bool,
) -> RawImport {
    RawImport {
        specifier: SmolStr::new(specifier),
        kind: ImportKind::Package,
        span,
        side_effect_only,
        type_only: false,
        confidence: Confidence::Certain,
        bindings,
        reexported: false,
        opaque_namespace_use,
        local_alias: None,
    }
}

/// `[File "private", Package "package-private", Public "protected", Public "public"]` (spec
/// §0). Reads the `modifiers` node's direct child tokens; a top-level type never carries
/// `private`/`protected` (illegal Java), so this naturally yields only levels 1/3 for them.
fn visibility(node: Node) -> (u8, bool) {
    let Some(modifiers) = modifiers_node(node) else {
        return (1, false); // package-private default
    };
    let mut c = modifiers.walk();
    let mut private = false;
    let mut protected = false;
    let mut public = false;
    for child in modifiers.children(&mut c) {
        match child.kind() {
            "private" => private = true,
            "protected" => protected = true,
            "public" => public = true,
            _ => {}
        }
    }
    if public {
        (3, true)
    } else if protected {
        (2, false)
    } else if private {
        (0, false)
    } else {
        (1, false)
    }
}

/// `modifiers`' children include modifier keywords (`public`, `static`, …) as anonymous
/// tokens — invisible in `to_sexp()` dumps, but real tree nodes reachable by kind.
fn modifiers_node(item: Node) -> Option<Node> {
    let mut c = item.walk();
    let found = item.children(&mut c).find(|n| n.kind() == "modifiers");
    found
}

fn has_modifier(item: Node, keyword: &str) -> bool {
    let Some(modifiers) = modifiers_node(item) else {
        return false;
    };
    let mut c = modifiers.walk();
    let found = modifiers.children(&mut c).any(|n| n.kind() == keyword);
    found
}

fn has_annotation(node: Node, src: &[u8], name: &str) -> bool {
    let Some(modifiers) = modifiers_node(node) else {
        return false;
    };
    let mut c = modifiers.walk();
    let found = modifiers.children(&mut c).any(|child| {
        matches!(child.kind(), "marker_annotation" | "annotation")
            && child
                .child_by_field_name("name")
                .is_some_and(|n| text(n, src) == name)
    });
    found
}

/// Dispatches a top-level or nested type declaration to its body handler (spec §2). Shared by
/// `handle_top_level` and nested-type recursion inside `handle_type_body`.
fn handle_type(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let kind = item.kind();
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let vis = visibility(item);
    let symbol_kind = match kind {
        "class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "record_declaration" => SymbolKind::Other(SmolStr::new("record")),
        "annotation_type_declaration" => SymbolKind::Other(SmolStr::new("annotation")),
        _ => return,
    };
    push_declaration(out, name, symbol_kind, item, None, ctx.owner, vis);

    // superclass / implements / extends_interfaces → Extend; everything else in the header
    // (type parameters' bounds) → TypeUse, walked generically below.
    if let Some(superclass) = item.child_by_field_name("superclass") {
        walk_extend_refs(superclass, src, name, out);
    }
    if let Some(interfaces) = item.child_by_field_name("interfaces") {
        walk_extend_refs(interfaces, src, name, out);
    }
    let mut c = item.walk();
    for child in item.children(&mut c) {
        match child.kind() {
            "superclass" | "interfaces" | "extends_interfaces" => {
                if child.kind() == "extends_interfaces" {
                    walk_extend_refs(child, src, name, out);
                }
            }
            "class_body" | "interface_body" | "enum_body" | "annotation_type_body" => {
                handle_body(child, src, name, out)
            }
            "formal_parameters" => {
                // Record components (spec §2): TypeUse refs for their types, no declarations.
                walk_type_refs(child, src, Some(name), out);
            }
            "type_parameters" => walk_type_refs(child, src, Some(name), out),
            _ => {}
        }
    }
}

fn walk_extend_refs(node: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    match node.kind() {
        "type_identifier" => out.references.push(RawReference {
            name: SmolStr::new(text(node, src)),
            scope_context: None,
            span: span(node),
            within: Some(SmolStr::new(owner)),
            kind: RefKind::Extend,
        }),
        "scoped_type_identifier" => {
            if let Some(last) = last_type_name(node, src) {
                out.references.push(RawReference {
                    name: SmolStr::new(last),
                    scope_context: None,
                    span: span(node),
                    within: Some(SmolStr::new(owner)),
                    kind: RefKind::Extend,
                });
            }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                walk_extend_refs(child, src, owner, out);
            }
        }
    }
}

/// The final segment of a dotted type path (`java.io.Closeable` → `Closeable`).
fn last_type_name<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    text(node, src).rsplit('.').next()
}

/// Walks a type/interface/enum/annotation body, dispatching each member (spec §2). One
/// function for all four body kinds — their member shapes overlap enough (fields, methods,
/// constructors, nested types) that a single dispatch is honest, not a lossy generalization.
fn handle_body(body: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    let ctx = Ctx { owner: Some(owner) };
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        match member.kind() {
            "field_declaration" => handle_field(member, src, &ctx, out),
            "method_declaration" => handle_method(member, src, &ctx, out),
            "constructor_declaration" => handle_constructor(member, src, &ctx, out),
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration" => handle_type(member, src, &ctx, out),
            "enum_constant" => {
                if let Some(name_node) = member.child_by_field_name("name") {
                    push_declaration(
                        out,
                        text(name_node, src),
                        SymbolKind::EnumMember,
                        member,
                        None,
                        Some(owner),
                        (3, true), // enum constants share the enum's own visibility (§2)
                    );
                }
                // A constant's own class-body override (`RED { void tag() {} }`) is rare and
                // not walked separately in v1 — its body still reaches ordinary reference
                // extraction via the generic body walker below when present as a body field.
                if let Some(body) = member.child_by_field_name("body") {
                    walk_body(body, src, Some(owner), out);
                }
            }
            "enum_body_declarations" => handle_body(member, src, owner, out),
            "static_initializer" => {
                if let Some(block) = member.child_by_field_name("body").or_else(|| {
                    member
                        .children(&mut member.walk())
                        .find(|n| n.kind() == "block")
                }) {
                    walk_body(block, src, Some(owner), out);
                }
            }
            "annotation_type_element_declaration" => {
                if let Some(name_node) = member.child_by_field_name("name") {
                    push_declaration(
                        out,
                        text(name_node, src),
                        SymbolKind::Method,
                        member,
                        None,
                        Some(owner),
                        (3, true),
                    );
                }
                if let Some(ty) = member.child_by_field_name("type") {
                    walk_type_refs(ty, src, Some(owner), out);
                }
            }
            _ => {}
        }
    }
}

fn handle_field(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let vis = visibility(item);
    let Some(ty) = item.child_by_field_name("type") else {
        return;
    };
    let mut c = item.walk();
    for declarator in item.children(&mut c) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_node) = declarator.child_by_field_name("name") else {
            continue;
        };
        push_declaration(
            out,
            text(name_node, src),
            SymbolKind::Field,
            declarator,
            None,
            ctx.owner,
            vis,
        );
        walk_type_refs(ty, src, ctx.owner, out);
        if let Some(value) = declarator.child_by_field_name("value") {
            walk_body(value, src, ctx.owner, out);
        }
    }
}

fn handle_method(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let vis = visibility(item);
    let body = item.child_by_field_name("body");
    let signature_span = body.map(|b| Span {
        start: span(item).start,
        end: span(b).start,
    });
    let (kind, qualified) = match ctx.owner {
        Some(owner) => (SymbolKind::Method, format!("{owner}.{name}")),
        None => (SymbolKind::Function, name.to_string()),
    };
    push_declaration(out, name, kind, item, signature_span, ctx.owner, vis);

    // `public static void main(String[] args)` — the JVM entry point, any class (spec §2).
    if ctx.owner.is_some() && name == "main" && vis.1 && has_modifier(item, "static") {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(&qualified)),
            confidence: Confidence::Probable,
        });
    }
    // `@Override` dispatch rooting (spec §0) — the JDK/collections call site never names the
    // override, so the duck-typed fallback can't be trusted to see it.
    if has_annotation(item, src, "Override") {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(&qualified)),
            confidence: Confidence::Probable,
        });
    }

    let mut c = item.walk();
    for child in item.children(&mut c) {
        match child.kind() {
            "void_type"
            | "type_identifier"
            | "scoped_type_identifier"
            | "generic_type"
            | "array_type"
            | "integral_type"
            | "floating_point_type"
            | "boolean_type" => walk_type_refs(child, src, Some(&qualified), out),
            "formal_parameters" => walk_type_refs(child, src, Some(&qualified), out),
            "throws" => walk_type_refs(child, src, Some(&qualified), out),
            "type_parameters" => walk_type_refs(child, src, Some(&qualified), out),
            _ => {}
        }
    }
    if let Some(body) = body {
        walk_body(body, src, Some(&qualified), out);
        let shape = function_shape(body, &METRICS_SYNTAX, MIN_CLONE_TOKENS);
        out.functions.push(FunctionMetrics {
            symbol: SmolStr::new(&qualified),
            cyclomatic: shape.cyclomatic,
            loc: shape.loc,
            token_count: shape.token_count as u32,
            fingerprints: shape.fingerprints,
        });
    }
}

fn handle_constructor(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(owner) = ctx.owner else { return };
    let vis = visibility(item);
    let qualified = format!("{owner}.<init>");
    let body = item.child_by_field_name("body");
    let signature_span = body.map(|b| Span {
        start: span(item).start,
        end: span(b).start,
    });
    push_declaration(
        out,
        "<init>",
        SymbolKind::Method,
        item,
        signature_span,
        Some(owner),
        vis,
    );
    if let Some(params) = item.child_by_field_name("parameters") {
        walk_type_refs(params, src, Some(&qualified), out);
    }
    if let Some(body) = item.child_by_field_name("body").or(body) {
        walk_body(body, src, Some(&qualified), out);
        let shape = function_shape(body, &METRICS_SYNTAX, MIN_CLONE_TOKENS);
        out.functions.push(FunctionMetrics {
            symbol: SmolStr::new(&qualified),
            cyclomatic: shape.cyclomatic,
            loc: shape.loc,
            token_count: shape.token_count as u32,
            fingerprints: shape.fingerprints,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn push_declaration(
    out: &mut FileFacts,
    name: &str,
    kind: SymbolKind,
    item: Node,
    signature_span: Option<Span>,
    member_of: Option<&str>,
    (level, exported): (u8, bool),
) {
    out.declarations.push(kndo_core::adapter::Declaration {
        name: SmolStr::new(name),
        kind,
        span: span(item),
        exported,
        visibility: kndo_core::adapter::VisibilityLevel(level),
        member_of: member_of.map(SmolStr::new),
        signature_span,
    });
}

/// Type-position walk (field/param/return/throws/generic-bound types) → `TypeUse` (spec §2).
fn walk_type_refs(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    match node.kind() {
        "line_comment" | "block_comment" => {}
        "type_identifier" => out.references.push(RawReference {
            name: SmolStr::new(text(node, src)),
            scope_context: None,
            span: span(node),
            within: within.map(SmolStr::new),
            kind: RefKind::TypeUse,
        }),
        "scoped_type_identifier" => {
            if let Some(last) = last_type_name(node, src) {
                out.references.push(RawReference {
                    name: SmolStr::new(last),
                    scope_context: None,
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::TypeUse,
                });
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_type_refs(child, src, within, out);
            }
        }
    }
}

/// Expression/statement bodies: calls, field access, identifier reads, method references,
/// lambdas, anonymous classes (spec §2/§5).
fn walk_body(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    match node.kind() {
        "line_comment" | "block_comment" => return,
        "method_invocation" => {
            let name = node.child_by_field_name("name");
            let object = node.child_by_field_name("object");
            if let (Some(name_node), Some(object)) = (name, object) {
                if matches!(object.kind(), "identifier" | "this") {
                    out.references.push(RawReference {
                        name: SmolStr::new(text(name_node, src)),
                        scope_context: Some(SmolStr::new(text(object, src))),
                        span: span(node),
                        within: within.map(SmolStr::new),
                        kind: RefKind::Call,
                    });
                    if object.kind() == "identifier" {
                        out.references.push(RawReference {
                            name: SmolStr::new(text(object, src)),
                            scope_context: None,
                            span: span(object),
                            within: within.map(SmolStr::new),
                            kind: RefKind::Read,
                        });
                    }
                } else {
                    out.references.push(RawReference {
                        name: SmolStr::new(text(name_node, src)),
                        scope_context: Some(SmolStr::new("<expr>")),
                        span: span(node),
                        within: within.map(SmolStr::new),
                        kind: RefKind::Call,
                    });
                    walk_body(object, src, within, out);
                }
            } else if let Some(name_node) = name {
                out.references.push(RawReference {
                    name: SmolStr::new(text(name_node, src)),
                    scope_context: None,
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::Call,
                });
            }
            if let Some(args) = node.child_by_field_name("arguments") {
                walk_body(args, src, within, out);
            }
            return;
        }
        "field_access" => {
            let (Some(object), Some(field)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("field"),
            ) else {
                return;
            };
            if matches!(object.kind(), "identifier" | "this") {
                out.references.push(RawReference {
                    name: SmolStr::new(text(field, src)),
                    scope_context: Some(SmolStr::new(text(object, src))),
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::Read,
                });
                if object.kind() == "identifier" {
                    out.references.push(RawReference {
                        name: SmolStr::new(text(object, src)),
                        scope_context: None,
                        span: span(object),
                        within: within.map(SmolStr::new),
                        kind: RefKind::Read,
                    });
                }
            } else {
                out.references.push(RawReference {
                    name: SmolStr::new(text(field, src)),
                    scope_context: Some(SmolStr::new("<expr>")),
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::Read,
                });
                walk_body(object, src, within, out);
            }
            return;
        }
        "method_reference" => {
            // `Type::method` / `this::method` / `Type::new` (spec §2, §5).
            let mut c = node.walk();
            let children: Vec<Node> = node.children(&mut c).collect();
            if let (Some(qualifier), Some(last)) = (children.first(), children.last()) {
                let name = text(*last, src);
                if matches!(qualifier.kind(), "identifier" | "this") {
                    out.references.push(RawReference {
                        name: SmolStr::new(name),
                        scope_context: Some(SmolStr::new(text(*qualifier, src))),
                        span: span(node),
                        within: within.map(SmolStr::new),
                        kind: RefKind::Call,
                    });
                }
            }
            return;
        }
        "identifier" => {
            if is_reference_position(node) {
                out.references.push(RawReference {
                    name: SmolStr::new(text(node, src)),
                    scope_context: None,
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::Read,
                });
            }
            return;
        }
        "type_identifier" => {
            out.references.push(RawReference {
                name: SmolStr::new(text(node, src)),
                scope_context: None,
                span: span(node),
                within: within.map(SmolStr::new),
                kind: RefKind::TypeUse,
            });
            return;
        }
        "scoped_type_identifier" => {
            if let Some(last) = last_type_name(node, src) {
                out.references.push(RawReference {
                    name: SmolStr::new(last),
                    scope_context: None,
                    span: span(node),
                    within: within.map(SmolStr::new),
                    kind: RefKind::TypeUse,
                });
            }
            return;
        }
        "object_creation_expression" => {
            if let Some(ty) = node.child_by_field_name("type") {
                walk_type_refs(ty, src, within, out);
            }
            if let Some(args) = node.child_by_field_name("arguments") {
                walk_body(args, src, within, out);
            }
            // Anonymous class body (spec §5): walked for references, no declaration emitted.
            // The anonymous class_body, when present, is an UNLABELED trailing child (no
            // "body" field on this node — unlike method_declaration's) — found by kind.
            let mut oc = node.walk();
            let anon_body = node.children(&mut oc).find(|n| n.kind() == "class_body");
            if let Some(anon_body) = anon_body {
                let mut c = anon_body.walk();
                for member in anon_body.children(&mut c) {
                    if member.kind() == "method_declaration" {
                        if let Some(body) = member.child_by_field_name("body") {
                            walk_body(body, src, within, out);
                        }
                        if let Some(ty) = member.child_by_field_name("type") {
                            walk_type_refs(ty, src, within, out);
                        }
                    }
                }
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_body(child, src, within, out);
    }
}

/// Whether a bare identifier here reads a value (vs. binding a new name in a pattern/decl).
fn is_reference_position(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "formal_parameter"
        | "catch_formal_parameter"
        | "lambda_parameters"
        | "inferred_parameters"
        | "resource" => parent
            .child_by_field_name("name")
            .is_none_or(|n| n.id() != node.id()),
        "variable_declarator" => parent
            .child_by_field_name("name")
            .is_none_or(|n| n.id() != node.id()),
        _ => true,
    }
}

fn span(node: Node) -> Span {
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

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

fn collect_suppressions(node: Node, src: &[u8], out: &mut FileFacts) {
    if matches!(node.kind(), "line_comment" | "block_comment") {
        if let Some(pragma) =
            kndo_adapter_toolkit::suppression::parse_suppression_pragma(text(node, src))
        {
            out.suppressions.push(RawSuppression {
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

    fn facts(src: &str) -> FileFacts {
        extract("src/Main.java", src.as_bytes())
    }

    fn decl<'a>(f: &'a FileFacts, name: &str) -> &'a kndo_core::adapter::Declaration {
        f.declarations.iter().find(|d| d.name == name).unwrap()
    }

    #[test]
    fn package_sets_unit_and_unit_name() {
        let f = facts("package com.foo.bar;\npublic class Widget {}\n");
        assert_eq!(f.unit.as_deref(), Some("com.foo.bar"));
        assert_eq!(f.unit_name.as_deref(), Some("com.foo.bar"));
    }

    #[test]
    fn no_package_leaves_unit_none() {
        let f = facts("public class Widget {}\n");
        assert_eq!(f.unit, None);
    }

    #[test]
    fn declarations_cover_the_type_zoo() {
        let f = facts(
            "package p;\n\
             public class Widget {\n\
             \x20   private int x;\n\
             \x20   protected static final String NAME = \"w\";\n\
             \x20   public Widget(int x) {}\n\
             \x20   public void run() {}\n\
             \x20   class Inner { void m() {} }\n\
             \x20   interface Sub { void go(); }\n\
             \x20   enum Color { RED, GREEN }\n\
             \x20   record Point(int x, int y) {}\n\
             }\n\
             interface Foo {}\n\
             @interface MyAnno { String value(); }\n",
        );
        let names: Vec<&str> = f.declarations.iter().map(|d| d.name.as_str()).collect();
        for expected in [
            "Widget", "x", "NAME", "<init>", "run", "Inner", "m", "Sub", "go", "Color", "RED",
            "GREEN", "Point", "Foo", "MyAnno", "value",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }
        assert_eq!(decl(&f, "Inner").member_of.as_deref(), Some("Widget"));
        assert_eq!(decl(&f, "m").member_of.as_deref(), Some("Inner"));
    }

    #[test]
    fn visibility_maps_to_the_four_rung_ladder() {
        let f = facts(
            "package p;\n\
             class C {\n\
             \x20   private int a;\n\
             \x20   int b;\n\
             \x20   protected int c;\n\
             \x20   public int d;\n\
             }\n",
        );
        assert_eq!(decl(&f, "a").visibility.0, 0);
        assert!(!decl(&f, "a").exported);
        assert_eq!(decl(&f, "b").visibility.0, 1);
        assert_eq!(
            decl(&f, "c").visibility.0,
            2,
            "protected widens to Public scope, not exported here"
        );
        assert!(!decl(&f, "c").exported);
        assert_eq!(decl(&f, "d").visibility.0, 3);
        assert!(decl(&f, "d").exported);
        // Top-level type itself: public/package-private only.
        assert_eq!(decl(&f, "C").visibility.0, 1);
    }

    #[test]
    fn plain_import_splits_package_from_type() {
        let f = facts("package p;\nimport com.foo.Bar;\nclass C {}\n");
        let imp = f.imports.iter().find(|i| i.specifier == "com.foo").unwrap();
        assert!(imp.bindings.iter().any(|b| b.local == "Bar"));
    }

    #[test]
    fn wildcard_type_import_is_opaque_namespace_use() {
        let f = facts("package p;\nimport com.foo.*;\nclass C {}\n");
        let imp = f.imports.iter().find(|i| i.specifier == "com.foo").unwrap();
        assert!(imp.opaque_namespace_use);
        assert!(imp.bindings.is_empty());
    }

    #[test]
    fn static_import_binds_the_member_name() {
        let f = facts("package p;\nimport static com.foo.Bar.CONST;\nclass C {}\n");
        let imp = f
            .imports
            .iter()
            .find(|i| i.specifier == "com.foo::Bar")
            .expect("static import specifier");
        assert!(imp.bindings.iter().any(|b| b.local == "CONST"));
        assert!(!imp.opaque_namespace_use);
    }

    #[test]
    fn wildcard_static_import_is_opaque() {
        let f = facts("package p;\nimport static com.foo.Bar.*;\nclass C {}\n");
        let imp = f
            .imports
            .iter()
            .find(|i| i.specifier == "com.foo::Bar")
            .unwrap();
        assert!(imp.opaque_namespace_use);
    }

    #[test]
    fn override_roots_production_probable() {
        let f = facts(
            "package p;\n\
             interface Runnable2 { void run(); }\n\
             class C implements Runnable2 {\n\
             \x20   @Override\n\
             \x20   public void run() {}\n\
             \x20   public void plain() {}\n\
             }\n",
        );
        let root = f
            .roots
            .iter()
            .find(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "C.run"))
            .expect("@Override roots");
        assert_eq!(root.kind, RootKind::Production);
        assert_eq!(root.confidence, Confidence::Probable);
        assert!(!f
            .roots
            .iter()
            .any(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "C.plain")));
    }

    #[test]
    fn main_method_roots_production() {
        let f = facts(
            "package p;\nclass C {\n\
             \x20   public static void main(String[] args) {}\n\
             }\n",
        );
        assert!(f.roots.iter().any(
            |r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "C.main")
                && r.kind == RootKind::Production
        ));
    }

    #[test]
    fn body_references_carry_within_and_qualifiers() {
        let f = facts(
            "package p;\nclass C {\n\
             \x20   void run() { helper(); Other.staticCall(); this.field = 1; }\n\
             \x20   void helper() {}\n\
             \x20   int field;\n\
             }\n",
        );
        let helper = f.references.iter().find(|r| r.name == "helper").unwrap();
        assert_eq!(helper.within.as_deref(), Some("C.run"));
        assert_eq!(helper.kind, RefKind::Call);

        let call = f
            .references
            .iter()
            .find(|r| r.name == "staticCall")
            .unwrap();
        assert_eq!(call.scope_context.as_deref(), Some("Other"));
    }

    #[test]
    fn extends_and_implements_emit_extend_refs() {
        let f = facts(
            "package p;\n\
             class Base {}\n\
             interface Marker {}\n\
             class C extends Base implements Marker {}\n",
        );
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "Base" && r.kind == RefKind::Extend));
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "Marker" && r.kind == RefKind::Extend));
    }

    #[test]
    fn enum_constants_and_methods_are_members() {
        let f = facts("package p;\nenum Color { RED, GREEN; void tag() {} }\n");
        assert_eq!(decl(&f, "RED").member_of.as_deref(), Some("Color"));
        assert_eq!(decl(&f, "tag").member_of.as_deref(), Some("Color"));
    }

    #[test]
    fn lambda_and_method_reference_bodies_are_walked() {
        let f = facts(
            "package p;\nimport java.util.List;\nclass C {\n\
             \x20   void run(List<String> items) {\n\
             \x20       items.forEach(x -> helper(x));\n\
             \x20       items.forEach(C::helper2);\n\
             \x20   }\n\
             \x20   static void helper(String s) {}\n\
             \x20   static void helper2(String s) {}\n\
             }\n",
        );
        assert!(f.references.iter().any(|r| r.name == "helper"));
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "helper2" && r.scope_context.as_deref() == Some("C")));
    }

    #[test]
    fn anonymous_class_body_is_walked_but_not_declared() {
        let f = facts(
            "package p;\ninterface Runnable2 { void run(); }\nclass C {\n\
             \x20   void go() {\n\
             \x20       Runnable2 r = new Runnable2() { public void run() { helper(); } };\n\
             \x20   }\n\
             \x20   void helper() {}\n\
             }\n",
        );
        assert!(f.references.iter().any(|r| r.name == "helper"));
        assert!(!f
            .declarations
            .iter()
            .any(|d| d.name == "run" && d.member_of.is_none()));
    }

    #[test]
    fn record_type_is_declared_without_component_accessors() {
        let f = facts("package p;\nrecord Point(int x, int y) {}\n");
        assert!(f.declarations.iter().any(|d| d.name == "Point"));
        assert!(!f.declarations.iter().any(|d| d.name == "x"));
    }

    #[test]
    fn generated_marker_sets_detected_origin() {
        let f = facts("// Code generated by protoc. DO NOT EDIT.\npackage p;\nclass C {}\n");
        assert_eq!(
            f.detected_origin,
            Some(kndo_core::vocab::FileOrigin::Generated)
        );
    }

    #[test]
    fn suppression_pragma_is_collected() {
        let f = facts("package p;\nclass C {\n// kndo:allow unused\nvoid m() {}\n}\n");
        assert_eq!(f.suppressions.len(), 1);
    }

    #[test]
    fn cyclomatic_complexity_counts_branches() {
        let f = facts(
            "package p;\nclass C {\n\
             \x20   void m(int x) {\n\
             \x20       if (x > 0) { x = 1; } else { x = 2; }\n\
             \x20       for (int i = 0; i < 10; i++) {}\n\
             \x20   }\n\
             }\n",
        );
        let fm = f.functions.iter().find(|f| f.symbol == "C.m").unwrap();
        assert_eq!(fm.cyclomatic, 3); // base 1 + if + for
    }
}

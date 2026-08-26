//! Kotlin extraction, written against tree-sitter-kotlin-ng 1.1.0
//! node shapes verified by `parsing`'s `#[ignore]`d ground-truth dumps.
//!
//! Declaration/member dispatch is TABLE-driven (`DECL_HANDLERS`), not a `match` with one arm
//! per node kind: kndo's own CRAP gate flags any function whose branch count (a flat `match`
//! arm counts as one) pushes complexity past 5 under zero coverage, and this adapter has more
//! declaration shapes (class/object/companion/function/secondary-constructor/property/type-
//! alias/init-block/enum-entry) than that ceiling allows in one dispatcher. A lookup-and-call
//! keeps the dispatcher itself at effectively zero branches regardless of how many shapes it
//! covers — the same pattern `kndo_adapter_toolkit::jvm_manifest::gradle_scope` already uses.

use kndo_adapter_toolkit::metrics::{
    push_function_metrics as toolkit_push_function_metrics, MetricsSyntax, MIN_CLONE_TOKENS,
};
use kndo_adapter_toolkit::parsing::{find_child, span, text};
use kndo_core::adapter::{
    AdapterDiagnostic, DiagnosticLevel, FileFacts, ImportBinding, ImportKind, RawImport,
    RawReference, RawRoot, RawRootTarget, Span,
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

/// `when_entry` counts once per arm (matching Java/JS's n-way-
/// match rule); elvis (`?:`) and not-null (`!!`) are deliberately NOT branches — value-
/// producing fallback/assertion operators, not control-flow forks.
const METRICS_SYNTAX: MetricsSyntax = MetricsSyntax {
    branch_kinds: &[
        "if_expression",
        "when_entry",
        "for_statement",
        "while_statement",
        "try_expression",
        "&&",
        "||",
    ],
    identifier_kinds: &["identifier", "type_identifier"],
    literal_kinds: &[
        "string_literal",
        "character_literal",
        "integer_literal",
        "long_literal",
        "hex_literal",
        "bin_literal",
        "real_literal",
        "boolean_literal",
        "null_literal",
    ],
    skip_kinds: &["line_comment", "multiline_comment"],
};

/// Item-walk context: the member owner (a class/object/companion's bare name), for `member_of`
/// attribution. Companion-object members carry the ENCLOSING class's name here,
/// not the companion's own (a pragmatic call).
struct Ctx<'a> {
    owner: Option<&'a str>,
}

type DeclHandler = fn(Node, &[u8], &Ctx<'_>, &mut FileFacts);

const DECL_HANDLERS: &[(&str, DeclHandler)] = &[
    ("class_declaration", handle_type),
    ("object_declaration", handle_object),
    ("companion_object", handle_companion),
    ("function_declaration", handle_function),
    ("secondary_constructor", handle_secondary_constructor),
    ("property_declaration", handle_property),
    ("type_alias", handle_type_alias),
    ("anonymous_initializer", handle_init_block),
    ("enum_entry", handle_enum_entry),
];

fn dispatch_declaration(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some((_, handler)) = DECL_HANDLERS.iter().find(|(k, _)| *k == item.kind()) else {
        return;
    };
    handler(item, src, ctx, out);
}

pub(crate) fn extract(_path: &str, content: &[u8]) -> FileFacts {
    let mut out = FileFacts::default();
    if GENERATED_MARKERS.detect_generated(content) {
        out.detected_origin = Some(kndo_core::vocab::FileOrigin::Generated);
    }

    let Some(tree) = crate::parsing::parse(content) else {
        out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: "failed to initialize the Kotlin parser".to_string(),
            span: None,
        });
        return out;
    };
    let root = tree.root_node();
    if root.has_error() {
        out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: "parse errors — extraction is partial for this file".to_string(),
            span: None,
        });
    }

    walk_top_level(root, src(content), &mut out);
    kndo_adapter_toolkit::suppression::collect_suppressions(
        root,
        src(content),
        &["line_comment", "multiline_comment"],
        &mut out.suppressions,
    );
    out
}

fn src(content: &[u8]) -> &[u8] {
    content
}

fn walk_top_level(root: Node, src: &[u8], out: &mut FileFacts) {
    let top = Ctx { owner: None };
    for item in root.children(&mut root.walk()) {
        match item.kind() {
            "package_header" => handle_package(item, src, out),
            "import" => handle_import(item, src, out),
            _ => dispatch_declaration(item, src, &top, out),
        }
    }
}

fn handle_package(item: Node, src: &[u8], out: &mut FileFacts) {
    let Some(name_node) = item
        .children(&mut item.walk())
        .find(|n| n.kind() == "qualified_identifier")
    else {
        return;
    };
    let name = text(name_node, src);
    out.unit = Some(SmolStr::new(name));
    out.unit_name = Some(SmolStr::new(name));
}

/// `import com.foo.Bar` / `import com.foo.Bar as Alias` / `import com.foo.*`. Kotlin
/// has no `import static` sentinel shape (a top-level `const val`/function imports the same
/// way a class does).
fn handle_import(item: Node, src: &[u8], out: &mut FileFacts) {
    let Some(path_node) = item
        .children(&mut item.walk())
        .find(|n| n.kind() == "qualified_identifier")
    else {
        return;
    };
    let full = text(path_node, src).to_string();
    let sp = span(item);
    let is_wildcard = item.children(&mut item.walk()).any(|n| n.kind() == "*");
    let alias = import_alias(item, src);

    if is_wildcard {
        // `import kotlinx.coroutines.internal.*` is BOTH facts at once: a wildcard over the
        // target's exports (`opaque_namespace_use` — keeps the target alive without naming
        // what it took), and the language's scoping rule that every top-level name of that
        // package is now legal HERE, unqualified (`module_names_visible` — the bare-name
        // fallback consults the unit's table at Certain). Only the first was emitted, so a
        // bare call to a wildcard-imported top-level function resolved to nothing at all:
        // kotlinx.coroutines calls `recoverStackTrace(…)` this way from dozens of files in
        // other packages, and every declaration of it read `unused`.
        let mut imp = make_import(&full, sp, Vec::new(), true, None);
        imp.module_names_visible = true;
        out.imports.push(imp);
        return;
    }
    let Some((pkg, ty)) = full.rsplit_once('.') else {
        return;
    };
    out.imports.push(make_import(
        pkg,
        sp,
        vec![ImportBinding {
            local: SmolStr::new(alias.unwrap_or(ty)),
            imported: Some(SmolStr::new(ty)),
        }],
        false,
        None,
    ));
}

/// `import com.foo.Bar as Alias` — the identifier after `as`, when present.
fn import_alias<'a>(item: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut saw_as = false;
    for child in item.children(&mut item.walk()) {
        if saw_as && child.kind() == "identifier" {
            return Some(text(child, src));
        }
        saw_as = child.kind() == "as";
    }
    None
}

fn make_import(
    specifier: &str,
    span: Span,
    bindings: Vec<ImportBinding>,
    opaque_namespace_use: bool,
    local_alias: Option<SmolStr>,
) -> RawImport {
    RawImport {
        specifier: SmolStr::new(specifier),
        kind: ImportKind::Package,
        span,
        side_effect_only: false,
        type_only: false,
        confidence: Confidence::Certain,
        bindings,
        reexported: false,
        opaque_namespace_use,
        module_names_visible: false,
        local_alias,
        reconstructed: false,
    }
}

// ---------------------------------------------------------------- visibility

const VISIBILITY_LEVELS: &[(&str, u8)] = &[
    ("private", 0),
    ("internal", 1),
    ("protected", 2),
    ("public", 3),
];

/// `[File "private", Package "internal", Public "protected", Public "public"]`. No
/// modifier at all defaults to `public` (3) — the OPPOSITE default from Java's package-private,
/// a load-bearing difference.
fn visibility(item: Node) -> (u8, bool) {
    let level = visibility_keyword(item)
        .and_then(|kw| VISIBILITY_LEVELS.iter().find(|(k, _)| *k == kw))
        .map_or(3, |(_, l)| *l);
    (level, level == 3)
}

fn visibility_keyword(item: Node) -> Option<&'static str> {
    let modifiers = modifiers_node(item)?;
    let vis = modifiers
        .children(&mut modifiers.walk())
        .find(|n| n.kind() == "visibility_modifier")?;
    vis.children(&mut vis.walk())
        .find_map(|c| VISIBILITY_LEVELS.iter().find(|(k, _)| *k == c.kind()))
        .map(|(k, _)| *k)
}

fn modifiers_node(item: Node) -> Option<Node> {
    item.children(&mut item.walk())
        .find(|n| n.kind() == "modifiers")
}

fn has_modifier_wrapper(item: Node, wrapper: &str, keyword: &str) -> bool {
    let Some(modifiers) = modifiers_node(item) else {
        return false;
    };
    let Some(found) = modifiers
        .children(&mut modifiers.walk())
        .find(|n| n.kind() == wrapper)
    else {
        return false;
    };
    found
        .children(&mut found.walk())
        .any(|c| c.kind() == keyword)
}

fn has_keyword_child(item: Node, keyword: &str) -> bool {
    item.children(&mut item.walk()).any(|c| c.kind() == keyword)
}

// ---------------------------------------------------------------- class/interface/enum/object

fn class_symbol_kind(item: Node) -> SymbolKind {
    if has_modifier_wrapper(item, "class_modifier", "enum") {
        return SymbolKind::Enum;
    }
    if has_modifier_wrapper(item, "class_modifier", "annotation") {
        return SymbolKind::Other(SmolStr::new("annotation"));
    }
    if has_keyword_child(item, "interface") {
        return SymbolKind::Interface;
    }
    SymbolKind::Class
}

fn handle_type(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let vis = visibility(item);
    let kind = class_symbol_kind(item);
    push_declaration(out, src, name, kind, item, None, ctx.owner, vis);

    if let Some(primary) = find_child(item, "primary_constructor") {
        handle_primary_constructor(primary, src, name, out);
    }
    if let Some(delegations) = find_child(item, "delegation_specifiers") {
        walk_extend_refs(delegations, src, name, out);
    }
    if let Some(body) = find_any_child(item, &["class_body", "enum_class_body"]) {
        handle_body(body, src, name, out);
    }
}

/// `class C(private val x: Int, var y: String, z: Boolean)` — a `class_parameter` carrying
/// `val`/`var` is a real constructor-promoted property; a bare one is just a
/// constructor argument, its type still a `TypeUse` under the class's own liveness.
fn handle_primary_constructor(primary: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    let Some(params) = find_child(primary, "class_parameters") else {
        return;
    };
    for param in params.children(&mut params.walk()) {
        if param.kind() != "class_parameter" {
            continue;
        }
        handle_class_parameter(param, src, owner, out);
    }
}

fn handle_class_parameter(param: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    let is_property = has_keyword_child(param, "val") || has_keyword_child(param, "var");
    let ty = find_any_child(param, &["user_type", "nullable_type"]);
    if is_property {
        if let Some(name_node) = find_child(param, "identifier") {
            let vis = visibility(param);
            push_declaration(
                out,
                src,
                text(name_node, src),
                SymbolKind::Field,
                param,
                None,
                Some(owner),
                vis,
            );
        }
    }
    if let Some(ty) = ty {
        walk_type_refs(ty, src, Some(owner), out);
    }
    // `class Hasher(val cost: Int = DEFAULT_COST)` — the default value is an expression that
    // runs when the constructor runs, and its references are real. Only the parameter's TYPE
    // was walked, so a companion const used exactly this way (Exposed's
    // `SCryptHasher.DEFAULT_CPU_COST`) had no incoming reference at all. Everything after the
    // `=` is the initializer; the type and the name are separate children.
    if let Some(default) = param
        .children(&mut param.walk())
        .skip_while(|c| c.kind() != "=")
        .nth(1)
    {
        walk_body(default, src, Some(owner), out);
    }
}

fn handle_object(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let vis = visibility(item);
    push_declaration(
        out,
        src,
        name,
        SymbolKind::Other(SmolStr::new("object")),
        item,
        None,
        ctx.owner,
        vis,
    );
    if let Some(delegations) = find_child(item, "delegation_specifiers") {
        walk_extend_refs(delegations, src, name, out);
    }
    if let Some(body) = find_child(item, "class_body") {
        handle_body(body, src, name, out);
    }
}

/// `companion object Named { … }` — members attribute to the ENCLOSING class, not to
/// the companion itself: `Widget.factory()` is overwhelmingly how real code addresses them.
fn handle_companion(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(owner) = ctx.owner else {
        return;
    };
    if let Some(body) = find_child(item, "class_body") {
        handle_body(body, src, owner, out);
    }
}

/// Iterates a `class_body`/`enum_class_body`'s members through the shared declaration
/// dispatch table (`DECL_HANDLERS` already covers `enum_entry`, so this one function serves
/// both container kinds).
fn handle_body(body: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    let ctx = Ctx { owner: Some(owner) };
    for member in body.children(&mut body.walk()) {
        dispatch_declaration(member, src, &ctx, out);
    }
}

// ---------------------------------------------------------------- functions & constructors

fn handle_function(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    let vis = visibility(item);
    let (kind, qualified) = qualify(ctx.owner, name);
    let body = find_function_body(item);
    let signature_span = body.map(|b| Span {
        start: span(item).start,
        end: span(b).start,
    });
    push_declaration(out, src, name, kind, item, signature_span, ctx.owner, vis);
    root_function_if_entry_point(item, ctx.owner, name, &qualified, out);

    walk_function_signature(item, src, Some(&qualified), out);
    if let Some(body) = body {
        walk_body(body, src, Some(&qualified), out);
        push_function_metrics(out, &qualified, span(item), body);
    }
}

fn qualify(owner: Option<&str>, name: &str) -> (SymbolKind, String) {
    let Some(owner) = owner else {
        return (SymbolKind::Function, name.to_string());
    };
    (SymbolKind::Method, format!("{owner}.{name}"))
}

/// Top-level `fun main()` — the canonical Kotlin JVM entry point (the `@JvmStatic fun
/// main()`-inside-`object` variant is a documented non-goal) — or `override` dispatch
/// rooting (the modifier keyword rather than an annotation).
fn root_function_if_entry_point(
    item: Node,
    owner: Option<&str>,
    name: &str,
    qualified: &str,
    out: &mut FileFacts,
) {
    let is_main = is_top_level_main(owner, name);
    let is_override = has_modifier_wrapper(item, "member_modifier", "override");
    if is_main || is_override {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(qualified)),
            confidence: Confidence::Probable,
        });
    }
    // An `override` member is invoked through a supertype the call site never names — when
    // the supertype is external (KSP's SymbolProcessor, a framework listener) no reference
    // graph can reach it by name. Machinery dispatch: reaching the owner
    // plausibly reaches its overrides.
    if is_override {
        if let Some(decl) = out.declarations.last_mut().filter(|d| d.name == name) {
            decl.implicitly_invoked = true;
        }
    }
}

fn is_top_level_main(owner: Option<&str>, name: &str) -> bool {
    owner.is_none() && name == "main"
}

/// `function_value_parameters`, an extension receiver's `user_type` (`fun String.extFn()`),
/// and the return type — everything in a function's header except the body.
/// The function's own signature children — receiver/parameter/return/generic-bound types.
/// `function_body`'s kind never matches the whitelist below, so it's naturally skipped without
/// needing an explicit exclusion check.
fn walk_function_signature(item: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    for child in item.children(&mut item.walk()) {
        walk_signature_child(child, src, within, out);
    }
}

fn walk_signature_child(child: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    if matches!(
        child.kind(),
        "user_type" | "nullable_type" | "function_value_parameters" | "type_parameters"
    ) {
        walk_type_refs(child, src, within, out);
    }
}

fn find_function_body(item: Node) -> Option<Node> {
    find_child(item, "function_body")
}

fn handle_secondary_constructor(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(owner) = ctx.owner else {
        return;
    };
    let qualified = format!("{owner}.<init>");
    let block = find_child(item, "block");
    let signature_span = block.map(|b| Span {
        start: span(item).start,
        end: span(b).start,
    });
    push_declaration(
        out,
        src,
        "<init>",
        SymbolKind::Constructor,
        item,
        signature_span,
        Some(owner),
        (3, true),
    );
    if let Some(params) = find_child(item, "function_value_parameters") {
        walk_type_refs(params, src, Some(&qualified), out);
    }
    if let Some(delegation) = find_child(item, "constructor_delegation_call") {
        walk_body(delegation, src, Some(&qualified), out);
    }
    if let Some(block) = block {
        walk_body(block, src, Some(&qualified), out);
        push_function_metrics(out, &qualified, span(item), block);
    }
}

fn push_function_metrics(out: &mut FileFacts, qualified: &str, decl_span: Span, body: Node) {
    toolkit_push_function_metrics(
        out,
        qualified,
        decl_span,
        body,
        &METRICS_SYNTAX,
        MIN_CLONE_TOKENS,
    );
}

// ---------------------------------------------------------------- properties, type aliases, init

/// Flip `implicitly_invoked` on the just-pushed declaration when the item carries the
/// `override` modifier — dispatch machinery.
fn mark_override_implicit(item: Node, name: &str, out: &mut FileFacts) {
    if !has_modifier_wrapper(item, "member_modifier", "override") {
        return;
    }
    if let Some(d) = out.declarations.last_mut().filter(|d| d.name == name) {
        d.implicitly_invoked = true;
    }
}

fn handle_property(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(decl) = find_child(item, "variable_declaration") else {
        return;
    };
    let Some(name_node) = find_child(decl, "identifier") else {
        return;
    };
    let vis = visibility(item);
    let kind = if ctx.owner.is_some() {
        SymbolKind::Field
    } else {
        SymbolKind::Variable
    };
    let name = text(name_node, src);
    push_declaration(out, src, name, kind, item, None, ctx.owner, vis);
    // `override val` is dispatch machinery exactly like `override fun` (see
    // root_function_if_entry_point): the supertype's accessor call never names this member.
    mark_override_implicit(item, name, out);
    if let Some(ty) = find_any_child(decl, &["user_type", "nullable_type"]) {
        walk_type_refs(ty, src, ctx.owner, out);
    }
    if let Some(value) = property_initializer(item) {
        walk_body(value, src, ctx.owner, out);
    }
}

/// The expression after `=` in a `property_declaration` — the last child when it isn't the
/// `variable_declaration`/`val`/`var`/`modifiers`/`=` itself (positional, no field name).
fn property_initializer(item: Node) -> Option<Node> {
    let mut c = item.walk();
    item.children(&mut c).last().filter(|n| {
        !matches!(
            n.kind(),
            "variable_declaration" | "val" | "var" | "modifiers" | "="
        )
    })
}

fn handle_type_alias(item: Node, src: &[u8], _ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(name_node) = find_child(item, "identifier") else {
        return;
    };
    push_declaration(
        out,
        src,
        text(name_node, src),
        SymbolKind::TypeAlias,
        item,
        None,
        None,
        (3, true),
    );
    if let Some(ty) = find_any_child(item, &["user_type", "nullable_type"]) {
        walk_type_refs(ty, src, None, out);
    }
}

fn handle_init_block(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(owner) = ctx.owner else {
        return;
    };
    if let Some(block) = find_child(item, "block") {
        walk_body(block, src, Some(owner), out);
    }
}

fn handle_enum_entry(item: Node, src: &[u8], ctx: &Ctx<'_>, out: &mut FileFacts) {
    let Some(owner) = ctx.owner else {
        return;
    };
    let Some(name_node) = find_child(item, "identifier") else {
        return;
    };
    push_declaration(
        out,
        src,
        text(name_node, src),
        SymbolKind::EnumMember,
        item,
        None,
        Some(owner),
        (3, true),
    );
    if let Some(args) = find_child(item, "value_arguments") {
        walk_body(args, src, Some(owner), out);
    }
    if let Some(body) = find_child(item, "class_body") {
        walk_body(body, src, Some(owner), out);
    }
}

// The declaration funnel: one construction site for every Kotlin declaration shape, which
// is exactly why it takes this many facts. Same shape and same allow as the Java adapter's.
#[allow(clippy::too_many_arguments)]
fn push_declaration(
    out: &mut FileFacts,
    src: &[u8],
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
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: markers(item, src),
        signature_span,
    });
}

/// The annotation names written on this declaration, in source order — `Declaration::markers`.
/// Facts, never verdicts: every annotation is reported, and this adapter has no idea which
/// ones a framework acts on. `@Named("x")` parses as `annotation > constructor_invocation >
/// user_type`, the bare `@Repository` as `annotation > user_type` — both contribute the type's
/// own name. A qualified spelling contributes its last segment as well, for the reason
/// `kndo-adapter-java`'s twin of this function documents.
fn markers(item: Node, src: &[u8]) -> Vec<SmolStr> {
    let Some(modifiers) = find_child(item, "modifiers") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut c = modifiers.walk();
    for annotation in modifiers.children(&mut c) {
        if annotation.kind() != "annotation" {
            continue;
        }
        let Some(user_type) = find_child(annotation, "user_type").or_else(|| {
            find_child(annotation, "constructor_invocation")
                .and_then(|call| find_child(call, "user_type"))
        }) else {
            continue;
        };
        let name = text(user_type, src);
        if let Some((_, last)) = name.rsplit_once('.') {
            out.push(SmolStr::new(last));
        }
        out.push(SmolStr::new(name));
    }
    out
}

// ---------------------------------------------------------------- type refs & extends

/// `user_type` positions → `TypeUse`; recurses into `type_arguments` for generics
/// (`List<Foo>` → `Foo` too) but not into the base path's own segments (already captured by
/// `last_identifier_text`).
fn walk_type_refs(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    if node.kind() != "user_type" {
        let mut c = node.walk();
        for child in node.children(&mut c) {
            walk_type_refs(child, src, within, out);
        }
        return;
    }
    if let Some(name) = last_identifier_text(node, src) {
        out.references.push(RawReference {
            name: SmolStr::new(name),
            scope_context: None,
            span: span(node),
            within: within.map(SmolStr::new),
            kind: RefKind::TypeUse,
        });
    }
    if let Some(args) = find_child(node, "type_arguments") {
        walk_type_refs(args, src, within, out);
    }
}

/// The base path's own last `identifier` segment (`java.util.List` used as a type → `List`),
/// recursing into nested `user_type` (qualified-path nesting) but not into `type_arguments`.
fn last_identifier_text<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut result = None;
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "identifier" => result = Some(text(child, src)),
            "user_type" => result = last_identifier_text(child, src).or(result),
            _ => {}
        }
    }
    result
}

fn walk_extend_refs(node: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    match node.kind() {
        "constructor_invocation" => {
            emit_extend_ref(find_child(node, "user_type"), node, src, owner, out);
            // `class MyMeta : Base(MyProvider)` — the supertype is an Extend, but the ARGUMENTS
            // are ordinary expression references and were dropped on the floor. Whatever they
            // name reads as dead unless something else happens to use it, which is how
            // Exposed's `PostgreSQLTypeProvider` — passed to its superclass on the very next
            // declaration in the same file — went unreferenced. RFC 0012 §4 puts code that runs
            // on instantiation under the type itself, so `within` stays the owner.
            if let Some(args) = find_child(node, "value_arguments") {
                walk_body(args, src, Some(owner), out);
            }
        }
        "user_type" => emit_extend_ref(Some(node), node, src, owner, out),
        _ => {
            for child in node.children(&mut node.walk()) {
                walk_extend_refs(child, src, owner, out);
            }
        }
    }
}

fn emit_extend_ref(ty: Option<Node>, site: Node, src: &[u8], owner: &str, out: &mut FileFacts) {
    let Some(name) = ty.and_then(|t| last_identifier_text(t, src)) else {
        return;
    };
    out.references.push(RawReference {
        name: SmolStr::new(name),
        scope_context: None,
        span: span(site),
        within: Some(SmolStr::new(owner)),
        kind: RefKind::Extend,
    });
}

// ---------------------------------------------------------------- expression/body references

type BodyHandler = fn(Node, &[u8], Option<&str>, &mut FileFacts);

const BODY_HANDLERS: &[(&str, BodyHandler)] = &[
    ("call_expression", handle_call),
    ("navigation_expression", handle_navigation),
    ("user_type", walk_type_refs),
    ("identifier", handle_identifier_ref),
];

/// Expression/statement bodies: calls, navigation (field/property access), bare identifier
/// reads, type positions inside expressions (`is`/`as` checks) — table-driven for the same
/// CRAP-ceiling reason `DECL_HANDLERS` is (module doc comment).
fn walk_body(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    if matches!(node.kind(), "line_comment" | "multiline_comment") {
        return;
    }
    if let Some((_, handler)) = BODY_HANDLERS.iter().find(|(k, _)| *k == node.kind()) {
        handler(node, src, within, out);
        return;
    }
    for child in node.children(&mut node.walk()) {
        walk_body(child, src, within, out);
    }
}

fn handle_identifier_ref(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    if !is_reference_position(node) {
        return;
    }
    out.references.push(RawReference {
        name: SmolStr::new(text(node, src)),
        scope_context: None,
        span: span(node),
        within: within.map(SmolStr::new),
        kind: RefKind::Read,
    });
}

fn handle_call(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    let Some(callee) = children.next() else {
        return;
    };
    emit_call_ref(callee, src, within, out);
    for child in children {
        walk_body(child, src, within, out);
    }
}

fn emit_call_ref(callee: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    match callee.kind() {
        "identifier" => out.references.push(RawReference {
            name: SmolStr::new(text(callee, src)),
            scope_context: None,
            span: span(callee),
            within: within.map(SmolStr::new),
            kind: RefKind::Call,
        }),
        "navigation_expression" => emit_navigation_ref(callee, src, within, out, RefKind::Call),
        _ => walk_body(callee, src, within, out),
    }
}

fn handle_navigation(node: Node, src: &[u8], within: Option<&str>, out: &mut FileFacts) {
    emit_navigation_ref(node, src, within, out, RefKind::Read);
}

/// `a.b`/`a.b.c()` — the terminal segment becomes the reference, `scope_context` is set when
/// the immediately-preceding segment is a plain identifier/`this`; a complex receiver
/// (a nested navigation/call) is walked for its own references instead.
fn emit_navigation_ref(
    node: Node,
    src: &[u8],
    within: Option<&str>,
    out: &mut FileFacts,
    kind: RefKind,
) {
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    let Some(qualifier) = children.next() else {
        return;
    };
    let Some(name_node) = children.last() else {
        return;
    };
    out.references.push(RawReference {
        name: SmolStr::new(text(name_node, src)),
        scope_context: simple_qualifier_text(qualifier, src).map(SmolStr::new),
        span: span(node),
        within: within.map(SmolStr::new),
        kind,
    });
    if simple_qualifier_text(qualifier, src).is_some() {
        if qualifier.kind() == "identifier" {
            out.references.push(RawReference {
                name: SmolStr::new(text(qualifier, src)),
                scope_context: None,
                span: span(qualifier),
                within: within.map(SmolStr::new),
                kind: RefKind::Read,
            });
        }
    } else {
        walk_body(qualifier, src, within, out);
    }
}

fn simple_qualifier_text<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    match node.kind() {
        "identifier" => Some(text(node, src)),
        "this_expression" => Some("this"),
        _ => None,
    }
}

/// Whether a bare identifier here reads a value (vs. binding a new name in a parameter/
/// declaration pattern).
fn is_reference_position(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "parameter" | "class_parameter" | "catch_block" | "lambda_parameters" => parent
            .children(&mut parent.walk())
            .find(|n| n.kind() == "identifier")
            .is_none_or(|n| n.id() != node.id()),
        "variable_declaration" => parent
            .children(&mut parent.walk())
            .find(|n| n.kind() == "identifier")
            .is_none_or(|n| n.id() != node.id()),
        _ => true,
    }
}

// ---------------------------------------------------------------- small tree helpers

fn find_any_child<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    node.children(&mut node.walk())
        .find(|n| kinds.contains(&n.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(src: &str) -> FileFacts {
        extract("src/Main.kt", src.as_bytes())
    }

    fn decl<'a>(f: &'a FileFacts, name: &str) -> &'a kndo_core::adapter::Declaration {
        f.declarations.iter().find(|d| d.name == name).unwrap()
    }

    #[test]
    fn a_superclass_constructor_argument_is_a_reference() {
        // `class MyMeta : Base(MyProvider)` — the supertype is an Extend, but the ARGUMENTS are
        // ordinary expression references and were dropped. Whatever they name read as dead
        // unless something else happened to use it: Exposed's `PostgreSQLTypeProvider`, passed
        // to its superclass on the very next declaration in the same file, had no reference at
        // all.
        let f = extract(
            "a.kt",
            b"package p\nopen class Base(val q: Q)\ninterface Q\ninternal object MyProvider : Q\ninternal class MyMeta : Base(MyProvider)\n",
        );
        let r = f
            .references
            .iter()
            .find(|r| r.name == "MyProvider" && r.within.as_deref() == Some("MyMeta"))
            .expect("the superclass argument must be referenced, attributed to the subclass");
        assert_ne!(r.kind, RefKind::Extend, "the argument is not the supertype");
    }

    #[test]
    fn a_default_parameter_value_is_a_reference() {
        // `class Hasher(val cost: Int = DEFAULT_COST)` — only the parameter's TYPE was walked,
        // so a companion const used exactly this way (Exposed's `SCryptHasher.DEFAULT_CPU_COST`)
        // had no incoming reference.
        let f = extract(
            "a.kt",
            b"package p\nclass Hasher(val cost: Int = DEFAULT_COST) {\n  private companion object { private const val DEFAULT_COST = 42 }\n}\n",
        );
        assert!(
            f.references
                .iter()
                .any(|r| r.name == "DEFAULT_COST" && r.within.as_deref() == Some("Hasher")),
            "the default value's references belong to the class that runs them"
        );
    }

    #[test]
    fn package_sets_unit_and_unit_name() {
        let f = facts("package com.foo.bar\nclass Widget\n");
        assert_eq!(f.unit.as_deref(), Some("com.foo.bar"));
        assert_eq!(f.unit_name.as_deref(), Some("com.foo.bar"));
    }

    #[test]
    fn no_package_leaves_unit_none() {
        let f = facts("class Widget\n");
        assert_eq!(f.unit, None);
    }

    #[test]
    fn declarations_cover_the_type_zoo() {
        let f = facts(
            "package p\n\
             class Widget(private val x: Int, var y: String) {\n\
             \x20   val z: Int = 1\n\
             \x20   fun run() {}\n\
             \x20   class Inner {\n\
             \x20       fun m() {}\n\
             \x20   }\n\
             \x20   constructor(a: Int) : this(a, \"\") {\n\
             \x20   }\n\
             }\n\
             interface Sub {\n\
             \x20   fun go()\n\
             }\n\
             enum class Color { RED, GREEN }\n\
             object Singleton {\n\
             \x20   fun s() {}\n\
             }\n\
             fun topLevelFn() {}\n\
             val topLevelVal = 5\n\
             typealias Alias = String\n",
        );
        let names: Vec<&str> = f.declarations.iter().map(|d| d.name.as_str()).collect();
        for expected in [
            "Widget",
            "x",
            "y",
            "z",
            "run",
            "Inner",
            "m",
            "<init>",
            "Sub",
            "go",
            "Color",
            "RED",
            "GREEN",
            "Singleton",
            "s",
            "topLevelFn",
            "topLevelVal",
            "Alias",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }
        assert_eq!(decl(&f, "x").member_of.as_deref(), Some("Widget"));
        assert_eq!(decl(&f, "y").member_of.as_deref(), Some("Widget"));
        assert_eq!(decl(&f, "Inner").member_of.as_deref(), Some("Widget"));
        assert_eq!(decl(&f, "m").member_of.as_deref(), Some("Inner"));
        assert_eq!(decl(&f, "RED").member_of.as_deref(), Some("Color"));
        assert_eq!(decl(&f, "s").member_of.as_deref(), Some("Singleton"));
        assert_eq!(decl(&f, "topLevelFn").member_of, None);
    }

    #[test]
    fn visibility_maps_to_the_four_rung_ladder_with_public_default() {
        let f = facts(
            "package p\n\
             class C {\n\
             \x20   private val a = 1\n\
             \x20   internal val b = 1\n\
             \x20   protected val c = 1\n\
             \x20   public val d = 1\n\
             \x20   val e = 1\n\
             }\n",
        );
        assert_eq!(decl(&f, "a").visibility.0, 0);
        assert!(!decl(&f, "a").exported);
        assert_eq!(decl(&f, "b").visibility.0, 1);
        assert_eq!(decl(&f, "c").visibility.0, 2);
        assert!(
            !decl(&f, "c").exported,
            "protected widens to Public scope, not exported here"
        );
        assert_eq!(decl(&f, "d").visibility.0, 3);
        assert!(decl(&f, "d").exported);
        assert_eq!(
            decl(&f, "e").visibility.0,
            3,
            "no modifier defaults to public"
        );
        assert!(decl(&f, "e").exported);
    }

    #[test]
    fn class_parameter_without_val_or_var_is_not_a_declaration() {
        let f = facts("package p\nclass C(x: Int) { fun use() = x }\n");
        assert!(!f.declarations.iter().any(|d| d.name == "x"));
    }

    #[test]
    fn plain_import_splits_package_from_type() {
        let f = facts("package p\nimport com.foo.Bar\nclass C\n");
        let imp = f.imports.iter().find(|i| i.specifier == "com.foo").unwrap();
        assert!(imp.bindings.iter().any(|b| b.local == "Bar"));
    }

    #[test]
    fn aliased_import_binds_the_alias_name() {
        let f = facts("package p\nimport com.foo.Bar as Alias\nclass C\n");
        let imp = f.imports.iter().find(|i| i.specifier == "com.foo").unwrap();
        let b = imp.bindings.iter().find(|b| b.local == "Alias").unwrap();
        assert_eq!(b.imported.as_deref(), Some("Bar"));
    }

    #[test]
    fn annotations_become_declaration_markers() {
        let f = facts(
            "package p\n\
             @Repository\n\
             @Named(\"x\")\n\
             class Impl {\n\
             \x20   @AfterEach\n\
             \x20   fun cleanup() {}\n\
             \x20   fun plain() {}\n\
             }\n",
        );
        assert_eq!(
            decl(&f, "Impl").markers,
            vec![SmolStr::new("Repository"), SmolStr::new("Named")],
            "the bare form and the argument form alike"
        );
        assert_eq!(decl(&f, "cleanup").markers, vec![SmolStr::new("AfterEach")]);
        assert!(decl(&f, "plain").markers.is_empty());
    }

    #[test]
    fn wildcard_import_is_opaque_namespace_use() {
        let f = facts("package p\nimport com.foo.*\nclass C\n");
        let imp = f.imports.iter().find(|i| i.specifier == "com.foo").unwrap();
        assert!(imp.opaque_namespace_use);
        assert!(imp.bindings.is_empty());
        assert!(
            imp.module_names_visible,
            "it is also the language's scoping rule: every top-level name of that package is \
             legal here unqualified, which is what lets a bare call to a wildcard-imported \
             top-level function resolve at all"
        );
    }

    #[test]
    fn override_roots_production_probable() {
        let f = facts(
            "package p\n\
             interface Iface { fun go() }\n\
             class C : Iface {\n\
             \x20   override fun go() {}\n\
             \x20   fun plain() {}\n\
             }\n",
        );
        let root = f
            .roots
            .iter()
            .find(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "C.go"))
            .expect("override roots");
        assert_eq!(root.kind, RootKind::Production);
        assert_eq!(root.confidence, Confidence::Probable);
        assert!(!f
            .roots
            .iter()
            .any(|r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "C.plain")));
    }

    #[test]
    fn top_level_main_roots_production() {
        let f = facts("package p\nfun main() {}\n");
        assert!(f.roots.iter().any(
            |r| matches!(&r.target, RawRootTarget::Declaration(n) if n == "main")
                && r.kind == RootKind::Production
        ));
    }

    #[test]
    fn body_references_carry_within_and_qualifiers() {
        let f = facts(
            "package p\nclass C {\n\
             \x20   fun run() { helper(); Other.staticCall(); this.field }\n\
             \x20   fun helper() {}\n\
             \x20   val field = 1\n\
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

        let field = f
            .references
            .iter()
            .find(|r| r.name == "field" && r.scope_context.as_deref() == Some("this"))
            .unwrap();
        assert_eq!(field.kind, RefKind::Read);
    }

    #[test]
    fn delegation_specifiers_emit_extend_refs() {
        let f = facts(
            "package p\n\
             open class Base\n\
             interface Marker\n\
             class C : Base(), Marker\n",
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
    fn companion_object_members_attribute_to_the_enclosing_class() {
        let f = facts(
            "package p\n\
             class HasCompanion {\n\
             \x20   companion object Named {\n\
             \x20       fun factory() {}\n\
             \x20   }\n\
             }\n",
        );
        assert_eq!(
            decl(&f, "factory").member_of.as_deref(),
            Some("HasCompanion")
        );
        assert!(!f.declarations.iter().any(|d| d.name == "Named"));
    }

    #[test]
    fn object_declaration_members_use_the_objects_own_name() {
        let f = facts("package p\nobject Singleton {\n    fun s() {}\n}\n");
        assert_eq!(decl(&f, "s").member_of.as_deref(), Some("Singleton"));
    }

    #[test]
    fn secondary_constructor_delegation_call_is_walked() {
        let f = facts(
            "package p\nclass C(val a: Int) {\n\
             \x20   constructor(a: Int, b: Int) : this(a) { helper(b) }\n\
             \x20   fun helper(x: Int) {}\n\
             }\n",
        );
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "helper" && r.within.as_deref() == Some("C.<init>")));
    }

    #[test]
    fn enum_entry_with_arguments_is_a_member_and_arguments_are_walked() {
        let f = facts(
            "package p\nfun label(): String = \"x\"\nenum class Color(val hex: String) {\n\
             \x20   RED(label())\n\
             }\n",
        );
        assert_eq!(decl(&f, "RED").member_of.as_deref(), Some("Color"));
        assert!(f.references.iter().any(|r| r.name == "label"));
    }

    #[test]
    fn generated_marker_sets_detected_origin() {
        let f = facts("// Code generated by protoc. DO NOT EDIT.\npackage p\nclass C\n");
        assert_eq!(
            f.detected_origin,
            Some(kndo_core::vocab::FileOrigin::Generated)
        );
    }

    #[test]
    fn a_non_comment_line_never_triggers_generated_detection() {
        // Same self-reference shape the toolkit guards against: this adapter defines its own
        // GENERATED_MARKERS the same way Java's does — this guards against the
        // unguarded `Contains` behavior for Kotlin specifically. The marker text appears as
        // plain code (a string literal value), not inside an actual comment banner.
        let f = facts("package p\nval x = \"@generated\"\nclass C\n");
        assert_eq!(f.detected_origin, None);
    }

    #[test]
    fn suppression_pragma_is_collected() {
        let f = facts("package p\nclass C {\n// kndo:allow unused\nfun m() {}\n}\n");
        assert_eq!(f.suppressions.len(), 1);
    }

    #[test]
    fn cyclomatic_complexity_counts_branches() {
        let f = facts(
            "package p\nclass C {\n\
             \x20   fun m(x: Int) {\n\
             \x20       if (x > 0) { } else { }\n\
             \x20       for (i in 0..10) { }\n\
             \x20   }\n\
             }\n",
        );
        let fm = f.functions.iter().find(|f| f.symbol == "C.m").unwrap();
        assert_eq!(fm.cyclomatic, 3); // base 1 + if + for
    }

    #[test]
    fn elvis_and_not_null_are_not_branches() {
        let f = facts(
            "package p\nclass C {\n\
             \x20   fun m(n: Int?): Int {\n\
             \x20       val a = n ?: 0\n\
             \x20       return a + n!!\n\
             \x20   }\n\
             }\n",
        );
        let fm = f.functions.iter().find(|f| f.symbol == "C.m").unwrap();
        assert_eq!(fm.cyclomatic, 1);
    }

    #[test]
    fn extension_function_receiver_type_is_a_type_use_and_member_of_is_none() {
        let f = facts("package p\nfun String.extFn(): Int = this.length\n");
        assert_eq!(decl(&f, "extFn").member_of, None);
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "String" && r.kind == RefKind::TypeUse));
    }
}

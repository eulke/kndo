//! Extraction: package-blind top level (the unit is the directory), imports,
//! nominal declarations with Kotlin's own reach rules, and a pruned full-tree
//! walk for references and comments. Kotlin-specific facts spelled here:
//!
//! - No modifier means PUBLIC — the opposite default from Java. `internal`
//!   folds to Exported in the binary reach (module scope is wider than a file;
//!   the ladder's linear rungs are what will tell it apart); `protected` →
//!   Exported; `private` → Private.
//! - `override` is a MODIFIER, not an annotation, and it roots: an override is
//!   invoked through a supertype the call site never names. Top-level
//!   `fun main` roots too (the `@JvmStatic` object variant is a documented
//!   non-goal, v1's own stance).
//! - `val`/`var` class parameters in a primary constructor are REAL properties
//!   and declare as members; bare parameters contribute their types and
//!   default-value references only. Secondary constructors are never declared
//!   (constructor liveness is the type's — `SomeClass(…)` is an ordinary call
//!   naming the type), and neither are enum entries (`values()`/`entries`
//!   reach every one namelessly — the posture guava's own suppression
//!   vindicated for Java).
//! - Companion-object members attribute to the ENCLOSING class:
//!   `Widget.factory()` is how real code addresses them.

use kndo_contract::evidence::{
    DeclarationId, EvidenceSink, ImportBinding, ImportShape, ImportTarget, Reach, RefKind,
    RootKind, RootTarget, SymbolKind,
};
use kndo_contract::vocab::{Confidence, ProjectPath};
use kndo_toolkit as tk;
use smol_str::SmolStr;
use tree_sitter::Node;

/// Kotlin's comment markers, declared where its grammar knowledge lives; the
/// KDoc `/**` star belongs to the marker, so a pragma inside one parses.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"",
    block_doc: b"*",
};

pub fn extract(
    path: &ProjectPath,
    source: &[u8],
    tree: &tree_sitter::Tree,
    out: &mut EvidenceSink,
) {
    let p = path.as_str();
    let file_name = p.rsplit('/').next().unwrap_or(p);
    let is_test = p.starts_with("src/test/kotlin/")
        || p.contains("/src/test/kotlin/")
        || file_name.ends_with("Test.kt")
        || file_name.ends_with("Tests.kt")
        || file_name.ends_with("TestCase.kt");

    if is_test {
        out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Certain);
    } else {
        // Library mode, Go's and Java's stance: any non-test file is importable
        // published surface. Probable — convention, not this file's statement.
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Probable,
        );
    }

    let generated = is_generated(source);

    let root = tree.root_node();
    let mut cursor = root.walk();
    let children: Vec<Node<'_>> = root.named_children(&mut cursor).collect();
    let top = Ctx { owner: None };
    for item in children {
        match item.kind() {
            "import" => import(item, source, out),
            _ if !generated => declaration(item, source, &top, out),
            _ => {}
        }
    }

    references_and_comments(root, source, out);
}

/// Member-walk context: the owning type's declaration id. Companion members
/// carry the ENCLOSING class's id.
struct Ctx {
    owner: Option<DeclarationId>,
}

/// The JVM ecosystem's generated-file needles, declared here; the scan is the
/// toolkit's.
const GENERATED_NEEDLES: &[&str] = &["@generated", "Code generated", "DO NOT EDIT"];

fn is_generated(source: &[u8]) -> bool {
    tk::generated_marked(source, GENERATED_NEEDLES)
}

/// No modifier means public. `internal`/`protected` fold to Exported;
/// `private` alone is Private.
fn reach_of(item: Node<'_>) -> Reach {
    let Some(modifiers) = tk::child_of_kind(item, "modifiers") else {
        return Reach::Exported;
    };
    let mut private = false;
    tk::walk(modifiers, &mut |n| {
        if n.kind() == "private" {
            private = true;
        }
    });
    if private {
        Reach::Private
    } else {
        Reach::Exported
    }
}

fn has_modifier(item: Node<'_>, keyword: &str) -> bool {
    let Some(modifiers) = tk::child_of_kind(item, "modifiers") else {
        return false;
    };
    let mut found = false;
    tk::walk(modifiers, &mut |n| {
        if n.kind() == keyword {
            found = true;
        }
    });
    found
}

fn declaration(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    match item.kind() {
        "class_declaration" => handle_type(item, source, ctx, out),
        "object_declaration" => handle_object(item, source, ctx, out),
        "companion_object" => {
            // Members attribute to the ENCLOSING class — `Widget.factory()` is
            // how real code addresses them; the companion itself declares
            // nothing.
            if let Some(body) = tk::child_of_kind(item, "class_body") {
                handle_body(body, source, ctx, out);
            }
        }
        "function_declaration" => handle_function(item, source, ctx, out),
        "property_declaration" => handle_property(item, source, ctx, out),
        "type_alias" => {
            if let Some(name_node) = tk::child_of_kind(item, "identifier") {
                out.declaration(
                    tk::text(name_node, source),
                    SymbolKind::Type,
                    tk::span(item),
                    reach_of(item),
                );
            }
        }
        // Secondary constructors and init blocks declare nothing; their code is
        // reached by the generic reference walk. Enum entries likewise (their
        // liveness is the enum's).
        _ => {}
    }
}

fn handle_type(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let id = out.declaration(
        tk::text(name_node, source),
        SymbolKind::Type,
        tk::span(item),
        reach_of(item),
    );
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }
    let inner = Ctx { owner: Some(id) };
    if let Some(primary) = tk::child_of_kind(item, "primary_constructor") {
        primary_constructor_properties(primary, source, &inner, out);
    }
    for body_kind in ["class_body", "enum_class_body"] {
        if let Some(body) = tk::child_of_kind(item, body_kind) {
            handle_body(body, source, &inner, out);
        }
    }
}

/// `class C(private val x: Int, y: String)` — a `val`/`var` parameter is a
/// constructor-promoted property, a real member; a bare one is only an
/// argument (its type reaches evidence through the generic walk).
fn primary_constructor_properties(
    primary: Node<'_>,
    source: &[u8],
    ctx: &Ctx,
    out: &mut EvidenceSink,
) {
    let Some(params) = tk::child_of_kind(primary, "class_parameters") else {
        return;
    };
    let mut c = params.walk();
    for param in params.named_children(&mut c) {
        if param.kind() != "class_parameter" {
            continue;
        }
        let is_val = param
            .children(&mut param.walk())
            .any(|n| matches!(n.kind(), "val" | "var"));
        if !is_val {
            continue;
        }
        let Some(name_node) = tk::child_of_kind(param, "identifier") else {
            continue;
        };
        let kind = if param.children(&mut param.walk()).any(|n| n.kind() == "val") {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };
        let id = out.declaration(
            tk::text(name_node, source),
            kind,
            tk::span(param),
            reach_of(param),
        );
        if let Some(owner) = ctx.owner {
            out.member_of(id, owner);
        }
    }
}

fn handle_object(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let id = out.declaration(
        tk::text(name_node, source),
        SymbolKind::Type,
        tk::span(item),
        reach_of(item),
    );
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }
    let inner = Ctx { owner: Some(id) };
    if let Some(body) = tk::child_of_kind(item, "class_body") {
        handle_body(body, source, &inner, out);
    }
}

fn handle_body(body: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        declaration(member, source, ctx, out);
    }
}

fn handle_function(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = tk::text(name_node, source);
    let kind = if ctx.owner.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };
    let id = out.declaration(name, kind, tk::span(item), reach_of(item));
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }
    if let Some(body) = tk::child_of_kind(item, "function_body") {
        out.metrics(id, function_metrics(item, body));
    }

    // Top-level `fun main` — the canonical Kotlin JVM entry point.
    if ctx.owner.is_none() && name == "main" {
        out.root(
            RootTarget::Declaration(id),
            RootKind::Production,
            Confidence::Probable,
        );
    }
    // `override` dispatch: invoked through a supertype the call site never
    // names; `operator` likewise (`invoke` is called as `obj()`, `get` as
    // `obj[i]` — no source line spells the name).
    if has_modifier(item, "override") || has_modifier(item, "operator") {
        out.root(
            RootTarget::Declaration(id),
            RootKind::Production,
            Confidence::Probable,
        );
    }
}

fn handle_property(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(decl) = tk::child_of_kind(item, "variable_declaration") else {
        return;
    };
    let Some(name_node) = tk::child_of_kind(decl, "identifier") else {
        return;
    };
    let name = tk::text(name_node, source);
    let is_val = item.children(&mut item.walk()).any(|n| n.kind() == "val");
    let kind = match (is_val, ctx.owner.is_some()) {
        (true, _) => SymbolKind::Constant,
        (false, true) => SymbolKind::Variable,
        (false, false) => SymbolKind::Variable,
    };
    let id = out.declaration(name, kind, tk::span(item), reach_of(item));
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }
    // `override val` is dispatch machinery exactly like `override fun`.
    if has_modifier(item, "override") {
        out.root(
            RootTarget::Declaration(id),
            RootKind::Production,
            Confidence::Probable,
        );
    }
}

/// `import a.b.C` / `import a.b.C as D` / `import a.b.*`. Platform prefixes
/// (`java.`, `javax.`, `kotlin.`) produce no evidence — nothing in the project
/// resolves them. Kotlin has no static-import form: a top-level function or
/// `const val` imports exactly the way a class does.
fn import(item: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let Some(path_node) = tk::child_of_kind(item, "qualified_identifier")
        .or_else(|| tk::child_of_kind(item, "identifier"))
    else {
        return;
    };
    let full = tk::text(path_node, source);
    for prefix in ["java", "javax", "kotlin"] {
        if full == prefix || full.starts_with(&format!("{prefix}.")) {
            return;
        }
    }
    let sp = tk::span(item);
    let is_wildcard = item.children(&mut item.walk()).any(|n| n.kind() == "*");

    if is_wildcard {
        // Every top-level name of the package, opaquely and unqualified —
        // Glob over the package directory's files.
        out.import(
            ImportTarget::Package(SmolStr::new(full)),
            ImportShape::Glob,
            sp,
            Confidence::Certain,
        );
        return;
    }
    let Some((_, ty)) = full.rsplit_once('.') else {
        return;
    };
    let local = import_alias(item, source).unwrap_or(ty);
    out.import(
        ImportTarget::Package(SmolStr::new(full)),
        ImportShape::Bindings(vec![ImportBinding {
            imported: SmolStr::new(ty),
            local: SmolStr::new(local),
        }]),
        sp,
        Confidence::Certain,
    );
}

/// `import a.b.C as D` — the identifier after `as`, when present.
fn import_alias<'a>(item: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let mut saw_as = false;
    let mut c = item.walk();
    for child in item.children(&mut c) {
        if saw_as && child.kind() == "identifier" {
            return Some(tk::text(child, source));
        }
        saw_as = child.kind() == "as";
    }
    None
}

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    // Import and package paths already became import evidence (or deliberately
    // none); every segment inside would read as a false identifier use.
    tk::walk_pruned(root, &["import", "package_header"], &mut |n| {
        if matches!(n.kind(), "line_comment" | "block_comment") {
            tk::comment_evidence(n, source, &COMMENT_MARKERS, out);
            return;
        }
        if !matches!(n.kind(), "identifier" | "type_identifier") {
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
/// unambiguous declaration-name and binder positions are excluded.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        "class_declaration" | "object_declaration" | "function_declaration" => {
            parent.child_by_field_name("name") != Some(n)
        }
        // The identifier inside a variable/parameter binder is the new name.
        "variable_declaration"
        | "multi_variable_declaration"
        | "class_parameter"
        | "parameter"
        | "type_parameter"
        | "type_alias"
        | "enum_entry"
        | "label" => false,
        _ => true,
    }
}

fn classify(n: Node<'_>, parent: Node<'_>) -> RefKind {
    match parent.kind() {
        "call_expression" if parent.child_by_field_name("function") == Some(n) => RefKind::Call,
        "callable_reference" => RefKind::Call,
        "delegation_specifier" | "constructor_invocation" | "explicit_delegation" => {
            RefKind::Extend
        }
        "navigation_suffix" => {
            let called = parent
                .parent()
                .and_then(|nav| nav.parent())
                .is_some_and(|gp| gp.kind() == "call_expression");
            if called { RefKind::Call } else { RefKind::Read }
        }
        _ if n.kind() == "type_identifier" || parent.kind() == "user_type" => RefKind::TypeUse,
        _ => RefKind::Read,
    }
}

/// Metrics over one function, normalized as the other adapters normalize.
/// `when_entry` counts once per arm; elvis (`?:`) and not-null (`!!`) are
/// deliberately NOT branches — value-producing operators, not control forks.
fn function_metrics(item: Node<'_>, body: Node<'_>) -> kndo_contract::evidence::FunctionMetrics {
    let mut token_hashes: Vec<u64> = Vec::new();
    let mut cyclomatic = 1u32;
    tk::walk(body, &mut |n| {
        match n.kind() {
            "if_expression" | "when_entry" | "for_statement" | "while_statement"
            | "do_while_statement" | "catch_block" => cyclomatic += 1,
            "&&" | "||" => cyclomatic += 1,
            _ => {}
        }
        if n.child_count() == 0 {
            let class = match n.kind() {
                "identifier" | "type_identifier" => "id",
                "string_content" | "character_literal" => "str",
                "integer_literal" | "long_literal" | "hex_literal" | "bin_literal"
                | "real_literal" => "num",
                "line_comment" | "block_comment" => return,
                other => other,
            };
            token_hashes.push(tk::fnv1a(class.as_bytes()));
        }
    });
    let loc = (item.end_position().row - item.start_position().row + 1) as u32;
    kndo_contract::evidence::FunctionMetrics {
        cyclomatic,
        loc,
        token_count: token_hashes.len() as u32,
        fingerprints: tk::winnow(&token_hashes, 5, 4),
    }
}

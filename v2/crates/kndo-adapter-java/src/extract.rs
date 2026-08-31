//! Extraction: package/imports/types from the top level, members by nominal
//! ownership, and a pruned full-tree walk for references and comments.
//! Java-specific facts spelled here:
//!
//! - Visibility folds to the contract's binary reach: `public`/`protected` →
//!   Exported (`protected` is subclass-consumable API — an external subclass of
//!   a published library overrides it, and ruling that out needs a typechecker);
//!   package-private/`private` → Private, kept alive by the unit's pooled
//!   references. Interface and annotation members with no modifier are
//!   implicitly public (JLS 9.4).
//! - Dispatch the source never names is ROOTED, not guessed: `@Override` bodies
//!   and the serialization hooks (`readObject`, `writeObject`, `readResolve`,
//!   `writeReplace`, `readObjectNoData`) are invoked by machinery, `public
//!   static void main` by the JVM.
//! - Constructors are not declared: a constructor's liveness IS its type's
//!   (`new Widget()` references the type; no source line calls `<init>`), so
//!   declaring one would only manufacture dead symbols. Its body is still
//!   walked for references. Enum constants are not declared for the same
//!   reason: `values()`/`valueOf` reach every constant namelessly.
//! - `serialVersionUID` is never declared — the JVM reads it reflectively, so
//!   declaring it would accuse every `Serializable` class.
//! - `src/test/java/**` (the Maven/Gradle standard layout) and the Surefire
//!   filename conventions mark test files; `package-info.java` /
//!   `module-info.java` are javac/javadoc descriptors — Tooling. Every other
//!   file is importable published surface (Probable), Go's own library stance:
//!   Java has no `internal/` fence at all.

use kndo_contract::evidence::{
    DeclarationId, EvidenceSink, ImportBinding, ImportShape, ImportTarget, Reach, RefKind,
    RootKind, RootTarget, SymbolKind,
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
    let p = path.as_str();
    let file_name = p.rsplit('/').next().unwrap_or(p);
    let is_tooling = matches!(file_name, "package-info.java" | "module-info.java");
    // The standard layout's test directory is the build tool's own boundary —
    // Certain, and not published surface. A test-shaped NAME outside it is
    // convention only: Probable, and the file keeps its library-mode Production
    // root — a `LoadTest.java` on the main source path is still importable
    // surface.
    let test_dir = p.starts_with("src/test/java/") || p.contains("/src/test/java/");
    let test_name = file_name.ends_with("Test.java")
        || file_name.ends_with("Tests.java")
        || file_name.ends_with("TestCase.java");

    if is_tooling {
        out.root(
            RootTarget::WholeFile,
            RootKind::Tooling,
            Confidence::Certain,
        );
    } else if test_dir {
        out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Certain);
    } else {
        if test_name {
            out.root(RootTarget::WholeFile, RootKind::Test, Confidence::Probable);
        }
        // Library mode: any non-test class on the source path is importable
        // published surface, whether or not this repository imports it.
        // Probable — convention, not this file's statement.
        out.root(
            RootTarget::WholeFile,
            RootKind::Production,
            Confidence::Probable,
        );
    }

    // The `@generated` / `DO NOT EDIT` convention: generated code is the
    // generator's business — it declares nothing accusable here, while its
    // imports and references stay real evidence about YOUR code.
    let generated = is_generated(source);

    let root = tree.root_node();
    let mut cursor = root.walk();
    let children: Vec<Node<'_>> = root.named_children(&mut cursor).collect();
    for item in children {
        match item.kind() {
            "import_declaration" => import(item, source, out),
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration"
                if !generated =>
            {
                let ctx = Ctx {
                    owner: None,
                    implicit_public: false,
                };
                handle_type(item, source, &ctx, out);
            }
            _ => {}
        }
    }

    references_and_comments(root, source, out);
}

/// Member-walk context: the owning type's declaration id, and whether members
/// with no modifier are implicitly public (interface/annotation bodies).
struct Ctx {
    owner: Option<DeclarationId>,
    implicit_public: bool,
}

fn is_generated(source: &[u8]) -> bool {
    tk::generated_marked(source, tk::GENERATED_NEEDLES, &["//", "/*", "*"])
}

/// `public`/`protected` → Exported; `private`/package-private → Private, unless
/// the body makes members implicitly public.
fn reach_of(item: Node<'_>, ctx: &Ctx) -> Reach {
    let Some(modifiers) = modifiers_node(item) else {
        return if ctx.implicit_public {
            Reach::Exported
        } else {
            Reach::Private
        };
    };
    let mut c = modifiers.walk();
    let mut explicit = None;
    for child in modifiers.children(&mut c) {
        match child.kind() {
            "public" | "protected" => explicit = Some(Reach::Exported),
            "private" => explicit = Some(Reach::Private),
            _ => {}
        }
    }
    explicit.unwrap_or(if ctx.implicit_public {
        Reach::Exported
    } else {
        Reach::Private
    })
}

fn modifiers_node(item: Node<'_>) -> Option<Node<'_>> {
    let mut c = item.walk();
    item.children(&mut c).find(|n| n.kind() == "modifiers")
}

fn has_modifier(item: Node<'_>, keyword: &str) -> bool {
    modifiers_node(item).is_some_and(|m| {
        let mut c = m.walk();
        m.children(&mut c).any(|n| n.kind() == keyword)
    })
}

fn has_annotation(item: Node<'_>, source: &[u8], name: &str) -> bool {
    modifiers_node(item).is_some_and(|m| {
        let mut c = m.walk();
        m.children(&mut c).any(|child| {
            matches!(child.kind(), "marker_annotation" | "annotation")
                && child
                    .child_by_field_name("name")
                    .is_some_and(|n| tk::text(n, source) == name)
        })
    })
}

/// One type declaration (top-level or nested): the declaration, its owner link,
/// and every member of its body.
fn handle_type(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = tk::text(name_node, source);
    let id = out.declaration(name, SymbolKind::Type, tk::span(item), reach_of(item, ctx));
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }

    let body_ctx = |implicit_public| Ctx {
        owner: Some(id),
        implicit_public,
    };
    let mut c = item.walk();
    for child in item.children(&mut c) {
        match child.kind() {
            "class_body" | "enum_body" => handle_body(child, source, &body_ctx(false), out),
            "interface_body" | "annotation_type_body" => {
                handle_body(child, source, &body_ctx(true), out)
            }
            _ => {}
        }
    }
}

fn handle_body(body: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "method_declaration" => handle_method(member, source, ctx, out),
            // The interface-body spelling of a field (JLS 9.3, implicitly
            // public static final) shares the declarator shape.
            "field_declaration" | "constant_declaration" => handle_field(member, source, ctx, out),
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration" => handle_type(member, source, ctx, out),
            // Enum constants are deliberately NOT declared: `values()`,
            // `valueOf(..)` and `EnumSet.allOf(..)` reach every constant with no
            // source line naming it, so the grammar cannot prove one dead — the
            // same never-accuse-the-unprovable posture as constructors here and
            // methods in Go. A constant's liveness is its enum's.
            "enum_constant" => {}
            "enum_body_declarations" => handle_body(member, source, ctx, out),
            "annotation_type_element_declaration" => {
                if let Some(name_node) = member.child_by_field_name("name") {
                    let id = out.declaration(
                        tk::text(name_node, source),
                        SymbolKind::Method,
                        tk::span(member),
                        Reach::Exported,
                    );
                    if let Some(owner) = ctx.owner {
                        out.member_of(id, owner);
                    }
                }
            }
            _ => {}
        }
    }
}

const SERIALIZATION_HOOKS: [&str; 5] = [
    "readObject",
    "writeObject",
    "readResolve",
    "writeReplace",
    "readObjectNoData",
];

fn handle_method(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let Some(name_node) = item.child_by_field_name("name") else {
        return;
    };
    let name = tk::text(name_node, source);
    let id = out.declaration(
        name,
        SymbolKind::Method,
        tk::span(item),
        reach_of(item, ctx),
    );
    if let Some(owner) = ctx.owner {
        out.member_of(id, owner);
    }
    if item.child_by_field_name("body").is_some() {
        out.metrics(id, function_metrics(item, source));
    }

    // The JVM entry point, any class.
    if name == "main" && has_modifier(item, "static") && reach_of(item, ctx) == Reach::Exported {
        out.root(
            RootTarget::Declaration(id),
            RootKind::Production,
            Confidence::Probable,
        );
    }
    // Dispatch the source never names: an `@Override` body is reached through
    // its supertype's contract, a serialization hook reflectively by the JVM —
    // no call site can exist, by specification.
    if has_annotation(item, source, "Override") || SERIALIZATION_HOOKS.contains(&name) {
        out.root(
            RootTarget::Declaration(id),
            RootKind::Production,
            Confidence::Probable,
        );
    }
}

fn handle_field(item: Node<'_>, source: &[u8], ctx: &Ctx, out: &mut EvidenceSink) {
    let reach = reach_of(item, ctx);
    let mut c = item.walk();
    for declarator in item.children(&mut c) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_node) = declarator.child_by_field_name("name") else {
            continue;
        };
        let name = tk::text(name_node, source);
        if name == "serialVersionUID" {
            continue;
        }
        let kind = if has_modifier(item, "final") {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };
        let id = out.declaration(name, kind, tk::span(declarator), reach);
        if let Some(owner) = ctx.owner {
            out.member_of(id, owner);
        }
    }
}

/// One import declaration. `java.`/`javax.` platform imports produce no
/// evidence at all — there is nothing in the project to resolve them to, and an
/// unresolvable stdlib row would only be noise.
fn import(item: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    let mut c = item.walk();
    let is_static = item.children(&mut c).any(|n| n.kind() == "static");
    let mut c = item.walk();
    let is_wildcard = item.children(&mut c).any(|n| n.kind() == "asterisk");
    let Some(path_node) = item
        .named_child(0)
        .filter(|n| matches!(n.kind(), "scoped_identifier" | "identifier"))
    else {
        return;
    };
    let full = tk::text(path_node, source);
    if full == "java" || full.starts_with("java.") || full == "javax" || full.starts_with("javax.")
    {
        return;
    }
    let sp = tk::span(item);

    if is_static {
        if is_wildcard {
            // `import static com.foo.Bar.*;` — every static member of one
            // class, opaquely: the class path resolves to its file, Glob keeps
            // its exports alive without naming which one was taken.
            out.import(
                ImportTarget::Package(SmolStr::new(full)),
                ImportShape::Glob,
                sp,
                Confidence::Certain,
            );
        } else if let Some((class_path, member)) = full.rsplit_once('.') {
            // `import static com.foo.Bar.CONST;` — the member is the binding,
            // the class path is the target.
            out.import(
                ImportTarget::Package(SmolStr::new(class_path)),
                ImportShape::Bindings(vec![ImportBinding {
                    imported: SmolStr::new(member),
                    local: SmolStr::new(member),
                }]),
                sp,
                Confidence::Certain,
            );
        }
        return;
    }

    if is_wildcard {
        // `import com.foo.*;` — a type-import-on-demand (JLS 7.5.2): every
        // type in the package, opaquely. Resolves to the package directory's
        // files; Glob keeps their exports alive.
        out.import(
            ImportTarget::Package(SmolStr::new(full)),
            ImportShape::Glob,
            sp,
            Confidence::Certain,
        );
        return;
    }

    // `import com.foo.Bar;` — the type name is the binding a reference in this
    // file uses; the full dotted path is what resolution maps to a file.
    let Some((_, ty)) = full.rsplit_once('.') else {
        return;
    };
    out.import(
        ImportTarget::Package(SmolStr::new(full)),
        ImportShape::Bindings(vec![ImportBinding {
            imported: SmolStr::new(ty),
            local: SmolStr::new(ty),
        }]),
        sp,
        Confidence::Certain,
    );
}

/// Java's comment markers, declared where its grammar knowledge lives.
const COMMENT_MARKERS: tk::CommentMarkers<'static> = tk::CommentMarkers {
    line: &["//"],
    block: &[("/*", "*/")],
    line_doc: b"",
    block_doc: b"*",
};

fn references_and_comments(root: Node<'_>, source: &[u8], out: &mut EvidenceSink) {
    // Import/package paths already became import evidence (or deliberately
    // none); every segment inside would read as a false identifier use.
    tk::walk_pruned(
        root,
        &["import_declaration", "package_declaration"],
        &mut |n| {
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
        },
    );
}

/// Binding and naming positions are not uses; the bias stays keep-alive — only
/// unambiguous declaration-name and binder positions are excluded.
fn is_use(n: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration"
        | "method_declaration"
        | "constructor_declaration"
        | "variable_declarator"
        | "enum_constant"
        | "annotation_type_element_declaration"
        | "formal_parameter"
        | "spread_parameter"
        | "catch_formal_parameter"
        | "record_pattern_component"
        | "type_parameter"
        | "labeled_statement" => parent.child_by_field_name("name") != Some(n),
        // Everything else — including the `field` of a field access on an
        // arbitrary expression — counts as a use: pooled matching is the
        // keep-alive direction.
        _ => true,
    }
}

fn classify(n: Node<'_>, parent: Node<'_>) -> RefKind {
    match parent.kind() {
        "method_invocation" if parent.child_by_field_name("name") == Some(n) => RefKind::Call,
        "method_reference" => RefKind::Call,
        "object_creation_expression" if n.kind() == "type_identifier" => RefKind::Call,
        "superclass" | "super_interfaces" | "extends_interfaces" | "type_list" => RefKind::Extend,
        _ if n.kind() == "type_identifier" => RefKind::TypeUse,
        _ => RefKind::Read,
    }
}

const METRICS: tk::MetricsSpec = tk::MetricsSpec {
    is_branch: |n, source| match n.kind() {
        "if_statement"
        | "for_statement"
        | "enhanced_for_statement"
        | "while_statement"
        | "do_statement"
        | "catch_clause"
        | "ternary_expression" => true,
        // Colon groups AND arrow rules; either counts only when it carries a
        // real case label — the default arm is the catch-the-rest, not a new
        // predicate (the shared rule in the spec's contract).
        "switch_block_statement_group" | "switch_rule" => {
            let mut c = n.walk();
            n.children(&mut c).any(|ch| {
                ch.kind() == "switch_label" && !tk::text(ch, source).starts_with("default")
            })
        }
        "binary_expression" => {
            let mut c = n.walk();
            n.children(&mut c)
                .any(|ch| matches!(ch.kind(), "&&" | "||"))
        }
        _ => false,
    },
    token_class: |n| match n.kind() {
        "identifier" | "type_identifier" => Some("id"),
        "string_fragment" | "multiline_string_fragment" | "character_literal" => Some("str"),
        "decimal_integer_literal"
        | "hex_integer_literal"
        | "octal_integer_literal"
        | "binary_integer_literal"
        | "decimal_floating_point_literal"
        | "hex_floating_point_literal" => Some("num"),
        "line_comment" | "block_comment" => None,
        other => Some(other),
    },
};

fn function_metrics(item: Node<'_>, source: &[u8]) -> kndo_contract::evidence::FunctionMetrics {
    tk::function_metrics(item, &METRICS, source)
}

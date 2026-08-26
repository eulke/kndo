//! Go extraction — declarations, references, imports, roots. A manual
//! tree-sitter-go walk in the same style as `kndo-adapter-js`'s extraction (node/field names
//! verified via `parsing::introspect`, never guessed).
//!
//! Shape, contrasted with JS: Go's grammar needs no "which of several export forms is this"
//! disambiguation (there is exactly one: capitalize the identifier) and no relative-import
//! resolution pass — so this file is one flat walk plus one reference pass, not JS's several
//! sequential passes over hoisting-sensitive export/import surfaces.

use rustc_hash::FxHashSet as HashSet;

use kndo_adapter_toolkit::parsing::{span, text};
use kndo_core::adapter::{
    AdapterDiagnostic, Declaration, DiagnosticLevel, FileFacts, ImportKind, RawImport,
    RawReference, RawRoot, RawRootTarget, Span, VisibilityLevel,
};
use kndo_core::vocab::{Confidence, RefKind, RootKind, SymbolKind};
use smol_str::SmolStr;
use tree_sitter::Node;

/// Go's single authoritative generated-file convention (`go help generate`):
/// `^// Code generated .* DO NOT EDIT\.$`, before the first non-comment text — expressed as a
/// column-anchored prefix/suffix pair (the toolkit scanner's doc explains why
/// anchoring also keeps a generator's own source from matching its emitted marker string).
const GENERATED_MARKERS: kndo_adapter_toolkit::classify::ContentMarkers =
    kndo_adapter_toolkit::classify::ContentMarkers {
        generated_markers: &[kndo_adapter_toolkit::classify::LineMarker::PrefixSuffix {
            prefix: "// Code generated",
            suffix: "DO NOT EDIT.",
        }],
        scan_window_lines: 64,
        // PrefixSuffix never consults comment_openers (self-safe by column-anchoring instead).
        comment_openers: &[],
    };

pub(crate) fn extract(path: &str, content: &[u8]) -> FileFacts {
    let mut out = FileFacts::default();
    if GENERATED_MARKERS.detect_generated(content) {
        out.detected_origin = Some(kndo_core::vocab::FileOrigin::Generated);
    }

    let Some(tree) = crate::parsing::parse(content) else {
        // No tree means no `package` clause either — a bare-directory unit key is the honest
        // degenerate (still groups with nothing wrongly: real Go files always carry a clause).
        out.unit = Some(SmolStr::new(kndo_adapter_toolkit::paths::dirname(path)));
        out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: "failed to initialize the Go parser".to_string(),
            span: None,
        });
        return out;
    };
    let root = tree.root_node();
    if root.has_error() {
        out.diagnostics.push(AdapterDiagnostic {
            level: DiagnosticLevel::Warn,
            message: "syntax errors in this file — extraction is best-effort".to_string(),
            span: None,
        });
    }

    let declared_package = package_name(root, content);
    // Unit key = `dir#declared-package-name`: Go's *real* resolution unit is the
    // package, and one directory can legally hold two — `package foo` plus the external test
    // package `package foo_test`. Folding the declared name into the (opaque-to-the-core) key
    // splits them with zero core changes: a `foo_test` file cannot resolve `foo`'s
    // unexported symbols by proximity, exactly Go's own rule (it must import `foo` like any
    // other consumer). A file with no parseable package clause keys on the directory alone.
    out.unit = Some(match &declared_package {
        Some(name) => SmolStr::new(format!(
            "{}#{name}",
            kndo_adapter_toolkit::paths::dirname(path)
        )),
        None => SmolStr::new(kndo_adapter_toolkit::paths::dirname(path)),
    });
    // The name importers bind this package by: the declared package name —
    // assembly resolves unaliased qualified references against the *target's* value of this,
    // the correct-by-construction answer to the dir≠package problem (`gopkg.in/yaml.v3`
    // imports as `yaml`) that no per-file specifier guess could give.
    out.unit_name = declared_package.as_deref().map(SmolStr::new);

    let is_main_package = declared_package.as_deref() == Some("main");
    // Root-worthiness by path, computed once: `internal/` is
    // compiler-enforced, not externally consumed by definition, so its exports aren't
    // auto-promoted; a test file's declarations are never library-mode public API either.
    let is_internal = path.split('/').any(|seg| seg == "internal");
    let is_test_file = path.ends_with("_test.go");
    let flags = Flags {
        promote_exports: !is_internal && !is_test_file,
        // Only the compiler-enforced wall caps the *visibility rung* (ladder rung 1 —
        // lib.rs): a _test.go file's exports keep Public rung, its non-API nature is the
        // Test role's concern, not visibility's.
        is_internal,
    };

    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "import_declaration" => handle_import_declaration(child, content, &mut out),
            "function_declaration" => {
                handle_function(child, content, is_main_package, flags, &mut out)
            }
            "method_declaration" => handle_method(child, content, flags, &mut out),
            "type_declaration" => handle_type_declaration(child, content, flags, &mut out),
            "const_declaration" => handle_value_declaration(
                child,
                content,
                "const_spec",
                flags,
                SymbolKind::Const,
                &mut out,
            ),
            "var_declaration" => handle_value_declaration(
                child,
                content,
                "var_spec",
                flags,
                SymbolKind::Variable,
                &mut out,
            ),
            _ => {}
        }
    }

    collect_references(
        root,
        content,
        None, // top level: within is established by the walk itself
        &mut out.references,
    );
    kndo_adapter_toolkit::suppression::collect_suppressions(
        root,
        content,
        &["comment"],
        &mut out.suppressions,
    );

    out
}

// ---------------------------------------------------------------- shared helpers

/// Exported iff the first rune is uppercase — Go's entire visibility rule, no keyword
/// involved.
fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// Per-file extraction facts derived from the path once.
#[derive(Clone, Copy)]
struct Flags {
    /// Library-mode root promotion for exported declarations — off under `internal/` and in
    /// `_test.go` files (neither is externally consumed by definition).
    promote_exports: bool,
    /// Under an `internal/` path element: the compiler walls these packages off from external
    /// modules, so exported declarations sit on the capped middle rung (lib.rs ladder).
    is_internal: bool,
}

/// Rungs of the three-step ladder (lib.rs): unexported → 0; exported under `internal/` → 1
/// (Package scope — compiler-walled from external modules, the capped rung);
/// exported elsewhere → 2 (Public).
fn visibility(exported: bool, internal: bool) -> VisibilityLevel {
    VisibilityLevel(match (exported, internal) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    })
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
    signature_span: Option<Span>,
    flags: Flags,
) {
    let exported = is_exported(name);
    out.declarations.push(Declaration {
        name: SmolStr::new(name),
        kind,
        span: node_span,
        exported,
        visibility: visibility(exported, flags.is_internal),
        member_of: None,
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: Vec::new(),
        signature_span,
    });
    if exported && flags.promote_exports {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(name)),
            confidence: Confidence::Certain,
        });
    }
}

// ---------------------------------------------------------------- declarations

/// Everything before the body block: parameters and result types — the
/// declaration's *promise*, distinct from its implementation. `None` when the grammar has no
/// body (declarations inside `interface` blocks are handled elsewhere).
fn signature_span_of(node: Node) -> Option<Span> {
    let body = node.child_by_field_name("body")?;
    let start = node.start_position();
    let end = body.start_position();
    Some(Span {
        start: (start.row as u32 + 1, start.column as u32 + 1),
        end: (end.row as u32 + 1, end.column as u32 + 1),
    })
}

/// Go's metric-relevant node kinds (the machinery is
/// `kndo_adapter_toolkit::metrics`). Branch kinds: every `case` clause counts once (a switch
/// with n cases is n branches, matching McCabe), plus the short-circuit operator leaves.
const METRICS_SYNTAX: kndo_adapter_toolkit::metrics::MetricsSyntax =
    kndo_adapter_toolkit::metrics::MetricsSyntax {
        branch_kinds: &[
            "if_statement",
            "for_statement",
            "expression_case",
            "type_case",
            "communication_case",
            "&&",
            "||",
        ],
        identifier_kinds: &[
            "identifier",
            "field_identifier",
            "type_identifier",
            "package_identifier",
        ],
        literal_kinds: &[
            "interpreted_string_literal",
            "raw_string_literal",
            "int_literal",
            "float_literal",
            "rune_literal",
            "imaginary_literal",
        ],
        skip_kinds: &["comment"],
        // Each of these becomes its own shape when it is substantial enough to carry clone
        // evidence by itself; a small one stays an expression inside its owner.
        nested_callable_kinds: &["func_literal"],
        // A body that ONLY constructs a value carries no clone evidence: normalization erases the
        // field values (the whole authored content) and keeps the field list, which the type
        // declaration dictates.
        construction_kinds: &["composite_literal"],
    };

/// One callable's `FunctionMetrics`, computed over its *body* (signatures are promises,
/// bodies are the thing that gets copy-pasted). `symbol` uses the
/// same naming convention as roots/`within`: bare for free functions, qualified `T.Method`
/// for members, so assembly's lookup lands in the right table.
fn push_function_metrics(out: &mut FileFacts, symbol: &str, node: Node) {
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    kndo_adapter_toolkit::metrics::push_function_metrics(
        out,
        symbol,
        // `node` is the declaration node — the same span `push_declaration` recorded, which
        // is what assembly matches metrics against.
        span(node),
        body,
        &METRICS_SYNTAX,
        kndo_adapter_toolkit::metrics::MIN_CLONE_TOKENS,
    );
}

/// `func Name(...) ...` or `func init() {}` / `func main() {}` (roots —
/// unconditional regardless of the capitalization rule).
fn handle_function(
    node: Node,
    src: &[u8],
    is_main_package: bool,
    flags: Flags,
    out: &mut FileFacts,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(name_node, src);
    push_declaration(
        out,
        name,
        SymbolKind::Function,
        span(node),
        signature_span_of(node),
        flags,
    );
    push_function_metrics(out, name, node);
    if name == "init" || (name == "main" && is_main_package) {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            target: RawRootTarget::Declaration(SmolStr::new(name)),
            confidence: Confidence::Certain,
        });
    }
}

/// `func (t T) Name(...)` / `func (t *T) Name(...)` — a *member* declaration:
/// bare name `Name` with `member_of: Some("Type")`, never a `"Type.Name"` string. Ownership as
/// a structured fact is what lets the core resolve a bare method-call reference through the
/// duck-typed fallback instead of missing entirely — without it, an unexported method used
/// only in-package false-positives as `unused:method`.
fn handle_method(node: Node, src: &[u8], flags: Flags, out: &mut FileFacts) {
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
        visibility: visibility(exported, flags.is_internal),
        member_of: Some(SmolStr::new(&receiver_type)),
        signature_span: signature_span_of(node),
        // Stdlib-interface machinery: `encoding/json` calls `MarshalJSON`
        // reflectively, `fmt` calls `String`/`Error`/`GoString`, the encoding packages
        // call the Text/Binary pairs — no source call site ever writes these names (a
        // type's `MarshalJSON` is exercised by every `json.Marshal` of a value of that
        // type, yet is reachable by no reference).
        implicitly_invoked: matches!(
            method_name,
            "MarshalJSON"
                | "UnmarshalJSON"
                | "MarshalText"
                | "UnmarshalText"
                | "MarshalBinary"
                | "UnmarshalBinary"
                | "String"
                | "Error"
                | "GoString"
        ),
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers: Vec::new(),
    });
    push_function_metrics(out, &format!("{receiver_type}.{method_name}"), node);
    if exported && flags.promote_exports {
        out.roots.push(RawRoot {
            kind: RootKind::Production,
            // Member root targets use the qualified form — the
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

fn handle_type_declaration(node: Node, src: &[u8], flags: Flags, out: &mut FileFacts) {
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
        push_declaration(out, text(name_node, src), kind, span(spec), None, flags);
    }
}

fn handle_value_declaration(
    node: Node,
    src: &[u8],
    spec_kind: &str,
    flags: Flags,
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
            let name = text(name_node, src);
            // The blank identifier declares nothing referenceable — `var _ T = …` exists for
            // its side effect (the compile-time assertion below), and no source can ever name
            // it. Extracting it as a symbol is a guaranteed false `unused`, one per assertion,
            // and the idiom is everywhere: gin, hugo and go-redis all carry several.
            if name == "_" {
                continue;
            }
            push_declaration(out, name, symbol_kind.clone(), span(name_node), None, flags);
        }
        emit_explicit_witness(spec, src, out);
    }
}

/// An interface-typed var initialized with a composite literal — `var API Core = jsonApi{}`
/// — is Go's explicit dispatch witness: the declaration itself asserts `jsonApi` satisfies
/// `Core`, so calling through `Core` plausibly executes `jsonApi`'s methods (the one place
/// structural satisfaction is nameable without a typechecker).
/// Emits an Implement reference from the literal's type to the declared type; either name
/// failing to resolve drops the edge silently.
/// The single name inside a parenthesized conversion target — `defaultValidator` in
/// `(*defaultValidator)`. Returns `None` when the parens hold anything more complex than one
/// name, which keeps the witness to the shapes it can read honestly.
fn innermost_identifier<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut found = None;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if matches!(n.kind(), "identifier" | "type_identifier") {
            if found.is_some() {
                return None; // more than one name — not a plain conversion
            }
            found = Some(text(n, src));
            continue;
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    found
}

fn emit_explicit_witness(spec: Node, src: &[u8], out: &mut FileFacts) {
    let Some(declared) = spec
        .child_by_field_name("type")
        .filter(|t| t.kind() == "type_identifier")
    else {
        return;
    };
    let Some(value) = spec.child_by_field_name("value") else {
        return;
    };
    let mut stack = vec![value];
    while let Some(n) = stack.pop() {
        // `var _ Iface = (*T)(nil)` — the conversion form, and by far the most common way to
        // write the assertion (the composite-literal form below is the other). Tree shape is
        // `call_expression(parenthesized_expression(… T …), argument_list(nil))`, and `T` sits
        // there as a plain `identifier` because `*T` in expression position is not a type node.
        // Without this the idiom contributed nothing at all: the blank name is not extracted
        // and the assertion it exists to make was invisible.
        if n.kind() == "call_expression" {
            if let Some(witness) = n
                .child_by_field_name("function")
                .filter(|f| f.kind() == "parenthesized_expression")
                .and_then(|f| innermost_identifier(f, src))
            {
                out.references.push(RawReference {
                    name: SmolStr::new(text(declared, src)),
                    scope_context: None,
                    span: span(declared),
                    within: Some(SmolStr::new(witness)),
                    kind: RefKind::Implement,
                });
                continue;
            }
        }
        if n.kind() == "composite_literal" {
            if let Some(lit_ty) = n
                .child_by_field_name("type")
                .filter(|t| t.kind() == "type_identifier")
            {
                out.references.push(RawReference {
                    name: SmolStr::new(text(declared, src)),
                    scope_context: None,
                    span: span(declared),
                    within: Some(SmolStr::new(text(lit_ty, src))),
                    kind: RefKind::Implement,
                });
            }
            continue;
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
}

// ---------------------------------------------------------------- imports

fn handle_import_declaration(node: Node, src: &[u8], out: &mut FileFacts) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => handle_import_spec(child, src, out),
            "import_spec_list" => {
                let mut inner = child.walk();
                for spec in child.children(&mut inner) {
                    if spec.kind() == "import_spec" {
                        handle_import_spec(spec, src, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn handle_import_spec(node: Node, src: &[u8], out: &mut FileFacts) {
    let Some(path_node) = node.child_by_field_name("path") else {
        return;
    };
    // `interpreted_string_literal`'s own text still carries the surrounding quotes.
    let specifier = text(path_node, src).trim_matches('"').to_string();
    if specifier.is_empty() {
        return;
    }

    // Only the *explicit* alias is a per-file fact; an unaliased import binds
    // by the target package's declared name, which lives in the target — assembly derives it
    // from the resolved target's `unit_name` instead of this file guessing from the
    // specifier's last segment (a guess that fails on `gopkg.in/yaml.v3` → `yaml`).
    let name_node = node.child_by_field_name("name");
    let (side_effect_only, opaque_namespace_use, local_alias) = match name_node.map(|n| n.kind()) {
        Some("blank_identifier") => (true, false, None),
        Some("dot") => (false, true, None),
        Some("package_identifier") => (false, false, name_node.map(|n| SmolStr::new(text(n, src)))),
        _ => (false, false, None),
    };

    out.imports.push(RawImport {
        specifier: SmolStr::new(&specifier),
        kind: ImportKind::Package, // Go has no relative imports
        span: span(node),
        side_effect_only,
        type_only: false,
        confidence: Confidence::Certain, // no dynamic import surface in Go
        bindings: Vec::new(),            // Go imports bind a namespace, not names
        reexported: false,
        opaque_namespace_use,
        module_names_visible: false,
        local_alias,
        reconstructed: false,
    });
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

/// The attribution taxonomy, applied to Go's grammar: the symbol whose *use*
/// triggers this node's subtree, or `None` when the enclosing code runs at package load
/// (top-level `var`/`const` initializers — Go's package-init semantics). Functions and
/// methods cover their whole subtree, signature included: a dead function's parameter and
/// result types die with it. Type declarations own their bodies (struct field types,
/// interface method signatures — using the type requires them). Nested declarations keep the
/// *outermost* attribution — a local type inside a function runs when the function does.
fn within_for(node: Node, src: &[u8]) -> Option<SmolStr> {
    match node.kind() {
        "function_declaration" => node
            .child_by_field_name("name")
            .map(|n| SmolStr::new(text(n, src))),
        "method_declaration" => {
            let receiver = node.child_by_field_name("receiver")?;
            let receiver_type = receiver_type_name(receiver, src)?;
            let name = node.child_by_field_name("name")?;
            // Qualified `Owner.name` — the member convention, so assembly's
            // within-resolution lands in the same qualified table member roots use.
            Some(SmolStr::new(format!("{receiver_type}.{}", text(name, src))))
        }
        "type_spec" | "type_alias" => node
            .child_by_field_name("name")
            .map(|n| SmolStr::new(text(n, src))),
        _ => None,
    }
}

fn collect_references(
    node: Node,
    src: &[u8],
    within: Option<&SmolStr>,
    out: &mut Vec<RawReference>,
) {
    // Import specifiers are pure name-binding syntax — nothing inside is a reference (mirrors
    // js-ts's identical stance on `import_statement`).
    if matches!(node.kind(), "import_declaration" | "package_clause") {
        return;
    }

    // Outermost attribution wins (within_for's doc): only compute a new `within` when we're
    // not already inside one.
    let own_within = if within.is_none() {
        within_for(node, src)
    } else {
        None
    };
    let within = own_within.as_ref().or(within);

    // Qualified access `q.Name`: emitted as a structured
    // `{ name, scope_context: Some(q) }` fact — whether `q` is an import qualifier or a
    // receiver variable is *assembly's* call (it alone knows every import's alias and every
    // resolved target's declared package name), not a per-file guess. Synthesizing dotted
    // bindings here (`json.Marshal` string keys) would need a default-alias
    // approximation that fails on dir≠package specifiers (`gopkg.in/yaml.v3` binds as `yaml`).
    // The operand still gets its own plain reference — a receiver variable or package-level
    // var is genuinely used here; if it's an import qualifier instead, the bare name resolves
    // to nothing and drops, silently and safely.
    if node.kind() == "selector_expression" {
        if let (Some(operand), Some(field)) = (
            node.child_by_field_name("operand"),
            node.child_by_field_name("field"),
        ) {
            if operand.kind() == "identifier" {
                out.push(RawReference {
                    name: SmolStr::new(text(field, src)),
                    scope_context: Some(SmolStr::new(text(operand, src))),
                    span: span(node),
                    within: within.cloned(),
                    kind: RefKind::Read,
                });
                out.push(RawReference {
                    name: SmolStr::new(text(operand, src)),
                    scope_context: None,
                    span: span(operand),
                    within: within.cloned(),
                    kind: RefKind::Read,
                });
                return; // operand/field fully handled — don't also walk them generically
            }
        }
    }

    // The type-position mirror of the selector case: `pkg.Type` in a type position parses as
    // `qualified_type` (package/name fields), not `selector_expression` — same structured
    // fact, tagged TypeUse.
    if node.kind() == "qualified_type" {
        if let (Some(package), Some(name)) = (
            node.child_by_field_name("package"),
            node.child_by_field_name("name"),
        ) {
            out.push(RawReference {
                name: SmolStr::new(text(name, src)),
                scope_context: Some(SmolStr::new(text(package, src))),
                span: span(node),
                within: within.cloned(),
                kind: RefKind::TypeUse,
            });
            return;
        }
    }

    // `const`/`var` specs can name more than one identifier (`const A, B = 1, 2`) — skip every
    // `name`-field child, not just the first, before recursing into the rest (the values).
    if matches!(node.kind(), "const_spec" | "var_spec") {
        let mut skip_ids = HashSet::default();
        let mut name_cursor = node.walk();
        for n in node.children_by_field_name("name", &mut name_cursor) {
            skip_ids.insert(n.id());
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if skip_ids.contains(&child.id()) {
                continue;
            }
            collect_references(child, src, within, out);
        }
        return;
    }

    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "field_identifier"
    ) {
        // In tree-sitter-go, `type_identifier` *is* the type-position signal —
        // TypeUse falls out of the grammar. An embedded field (a `field_declaration` with no
        // `name`) is Go's inheritance-adjacent construct → Extend.
        let ref_kind = if node.kind() == "type_identifier" {
            let embedded = node.parent().is_some_and(|p| {
                p.kind() == "field_declaration" && p.child_by_field_name("name").is_none()
            });
            if embedded {
                RefKind::Extend
            } else {
                RefKind::TypeUse
            }
        } else {
            RefKind::Read
        };
        out.push(RawReference {
            name: SmolStr::new(text(node, src)),
            scope_context: None,
            span: span(node),
            within: within.cloned(),
            kind: ref_kind,
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
        collect_references(child, src, within, out);
    }
}

// ---------------------------------------------------------------- suppressions

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
    fn a_blank_var_declares_nothing_but_asserts_an_interface() {
        // `var _ StructValidator = (*defaultValidator)(nil)` is Go's compile-time interface
        // assertion, and it appears several times per repo in gin, hugo and go-redis. The
        // blank name is unreferenceable by definition, so extracting it as a symbol is a
        // guaranteed false `unused`; the assertion it exists to make was meanwhile invisible,
        // because the witness only read the composite-literal form.
        let f = extract(
            "a.go",
            b"package p\nvar _ StructValidator = (*defaultValidator)(nil)\n",
        );
        assert!(
            !f.declarations.iter().any(|d| d.name == "_"),
            "the blank identifier is not a declaration"
        );
        let witness = f
            .references
            .iter()
            .find(|r| r.kind == RefKind::Implement)
            .expect("the assertion must still contribute its Implement edge");
        assert_eq!(witness.name, "StructValidator");
        assert_eq!(witness.within.as_deref(), Some("defaultValidator"));

        // The composite-literal form keeps working — it is the other half of the same idiom.
        let f = extract("a.go", b"package p\nvar API Core = jsonApi{}\n");
        assert!(f.declarations.iter().any(|d| d.name == "API"));
        assert!(f
            .references
            .iter()
            .any(|r| r.kind == RefKind::Implement && r.within.as_deref() == Some("jsonApi")));
    }

    #[test]
    fn unit_is_directory_plus_declared_package_name() {
        let facts = extract("pkg/sub/a.go", b"package sub\n");
        assert_eq!(facts.unit.as_deref(), Some("pkg/sub#sub"));
        let facts = extract("a.go", b"package main\n");
        assert_eq!(facts.unit.as_deref(), Some("#main"));
    }

    #[test]
    fn external_test_package_gets_its_own_unit() {
        // One directory, two Go packages —
        // `foo` and its external test package `foo_test` — must
        // be two units, so the test package can't resolve foo's unexported symbols by
        // proximity (Go's own rule: it imports foo like any other consumer).
        let internal = extract("pkg/a.go", b"package foo\n");
        let external = extract("pkg/a_test.go", b"package foo_test\n\nimport \"testing\"\n");
        let in_package_test = extract("pkg/b_test.go", b"package foo\n\nimport \"testing\"\n");
        assert_eq!(internal.unit.as_deref(), Some("pkg#foo"));
        assert_eq!(external.unit.as_deref(), Some("pkg#foo_test"));
        // An ordinary in-package test file DOES share the unit — it sees unexported symbols.
        assert_eq!(in_package_test.unit, internal.unit);
    }

    #[test]
    fn plain_function_is_a_declaration() {
        let facts = extract("a.go", b"package p\n\nfunc Helper() int { return 1 }\n");
        let d = decl(&facts, "Helper");
        assert_eq!(d.kind, SymbolKind::Function);
        assert!(d.exported);
        assert_eq!(d.visibility, VisibilityLevel(2));
    }

    #[test]
    fn exported_function_under_internal_is_module_capped() {
        // Go's internal-package rule: exported, but the compiler walls it off from external
        // modules — the middle rung (Package scope), so the library-surface exemptions never
        // treat it as consumable API.
        let facts = extract(
            "internal/util/a.go",
            b"package util\n\nfunc Helper() int { return 1 }\n",
        );
        let d = decl(&facts, "Helper");
        assert!(d.exported);
        assert_eq!(d.visibility, VisibilityLevel(1));
    }

    #[test]
    fn exported_under_internal_sits_on_the_capped_middle_rung() {
        // Go's internal-package rule is compiler-enforced: exported, but never consumable
        // from outside the module — rung 1 (Package scope, surface_transitive: false), so
        // the core's library-surface machinery structurally cannot treat it as API while
        // `internal-only` can still advise narrowing it.
        let facts = extract(
            "internal/util/a.go",
            b"package util

func Helper() int { return 1 }
",
        );
        let d = decl(&facts, "Helper");
        assert!(d.exported);
        assert_eq!(d.visibility, VisibilityLevel(1));
        assert!(
            facts.roots.is_empty(),
            "internal/ exports are never promoted"
        );
    }

    #[test]
    fn test_file_exports_keep_the_public_rung() {
        // A _test.go file's non-API nature is the Test role's concern; its visibility rung
        // stays what the language says (capitalized = exported), so visibility analyses keep
        // full precision inside test packages.
        let facts = extract(
            "a_test.go",
            b"package p

func TestHelper() {}
",
        );
        let d = decl(&facts, "TestHelper");
        assert_eq!(d.visibility, VisibilityLevel(2));
        assert!(
            facts.roots.is_empty(),
            "test-file exports are never promoted"
        );
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
        // Bare name + member_of — never a dotted string. Value- and
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
        // Exported methods root themselves by the qualified form (member root targets)
        // — the core's bare-name table deliberately never holds members.
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
    fn plain_import_has_no_alias_and_qualified_access_is_a_structured_reference() {
        // No dotted-binding synthesis: the unaliased import carries NO
        // local_alias (assembly derives the qualifier from the target's declared package
        // name), and `json.Marshal` is a structured `{ name, scope_context }` fact.
        let src = b"package p\n\nimport \"encoding/json\"\n\nfunc F() { json.Marshal(nil) }\n";
        let facts = extract("a.go", src);
        assert_eq!(facts.imports.len(), 1);
        assert_eq!(facts.imports[0].specifier.as_str(), "encoding/json");
        assert!(!facts.imports[0].side_effect_only);
        assert_eq!(facts.imports[0].local_alias, None);
        assert!(facts.imports[0].bindings.is_empty());
        let qref = facts
            .references
            .iter()
            .find(|r| r.name.as_str() == "Marshal")
            .expect("qualified reference");
        assert_eq!(qref.scope_context.as_deref(), Some("json"));
    }

    #[test]
    fn aliased_import_carries_the_explicit_alias() {
        let src = b"package p\n\nimport j \"encoding/json\"\n\nfunc F() { j.Marshal(nil) }\n";
        let facts = extract("a.go", src);
        assert_eq!(facts.imports[0].local_alias.as_deref(), Some("j"));
        let qref = facts
            .references
            .iter()
            .find(|r| r.name.as_str() == "Marshal")
            .expect("qualified reference");
        assert_eq!(qref.scope_context.as_deref(), Some("j"));
    }

    #[test]
    fn unit_name_is_the_declared_package_name() {
        let facts = extract("pkg/v3/a.go", b"package yaml\n");
        assert_eq!(facts.unit_name.as_deref(), Some("yaml"));
    }

    #[test]
    fn qualified_type_positions_are_structured_type_use_references() {
        // `pkg.Type` in a type position parses as `qualified_type`, not selector_expression —
        // must yield the same structured fact, tagged TypeUse.
        let src = b"package p\n\nimport \"time\"\n\nfunc F(t time.Time) {}\n";
        let facts = extract("a.go", src);
        let qref = facts
            .references
            .iter()
            .find(|r| r.name.as_str() == "Time")
            .expect("qualified type reference");
        assert_eq!(qref.scope_context.as_deref(), Some("time"));
        assert_eq!(qref.kind, RefKind::TypeUse);
    }

    #[test]
    fn receiver_member_calls_carry_the_receiver_as_scope_context() {
        let src = b"package p\n\ntype T struct{}\n\nfunc (t T) helper() {}\n\nfunc F(t T) { t.helper() }\n";
        let facts = extract("a.go", src);
        let qref = facts
            .references
            .iter()
            .find(|r| r.name.as_str() == "helper")
            .expect("member reference");
        assert_eq!(qref.scope_context.as_deref(), Some("t"));
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

    // -------------------------------------------------- within attribution

    fn find_ref<'a>(facts: &'a FileFacts, name: &str) -> &'a RawReference {
        facts
            .references
            .iter()
            .find(|r| r.name.as_str() == name)
            .unwrap_or_else(|| panic!("no reference named {name:?} in {:?}", facts.references))
    }

    #[test]
    fn function_body_references_carry_the_function_as_within() {
        let facts = extract("a.go", b"package p\n\nfunc a() {}\nfunc caller() { a() }\n");
        assert_eq!(find_ref(&facts, "a").within.as_deref(), Some("caller"));
    }

    #[test]
    fn method_body_references_carry_the_qualified_member_as_within() {
        let facts = extract(
            "a.go",
            b"package p\n\ntype T struct{}\nfunc helper() {}\nfunc (t *T) Run() { helper() }\n",
        );
        assert_eq!(find_ref(&facts, "helper").within.as_deref(), Some("T.Run"));
    }

    #[test]
    fn function_signature_types_attribute_to_the_function() {
        // A dead function's parameter/result types die with it.
        let facts = extract(
            "a.go",
            b"package p\n\ntype Arg struct{}\nfunc F(x Arg) {}\n",
        );
        assert_eq!(find_ref(&facts, "Arg").within.as_deref(), Some("F"));
    }

    #[test]
    fn struct_field_types_attribute_to_the_struct() {
        let facts = extract(
            "a.go",
            b"package p\n\ntype Inner struct{}\ntype Outer struct {\n\tfield Inner\n}\n",
        );
        assert_eq!(find_ref(&facts, "Inner").within.as_deref(), Some("Outer"));
    }

    #[test]
    fn package_level_initializers_are_load_time_no_within() {
        // Go's package-init semantics: `var x = f()` runs when the package loads —
        // attribution None ⇒ the file.
        let facts = extract(
            "a.go",
            b"package p\n\nfunc f() int { return 1 }\n\nvar x = f()\n",
        );
        let init_ref = facts
            .references
            .iter()
            .find(|r| r.name.as_str() == "f" && r.within.is_none());
        assert!(
            init_ref.is_some(),
            "package-level initializer reference must have no within: {:?}",
            facts.references
        );
    }

    #[test]
    fn qualified_import_access_inside_a_function_carries_within_too() {
        let facts = extract(
            "a.go",
            b"package p\n\nimport \"fmt\"\n\nfunc F() { fmt.Println(1) }\n",
        );
        assert_eq!(find_ref(&facts, "Println").within.as_deref(), Some("F"));
        assert_eq!(
            find_ref(&facts, "Println").scope_context.as_deref(),
            Some("fmt")
        );
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

    // ------------------------------------------------- RefKind + signature_span

    #[test]
    fn function_signature_span_covers_params_and_result_but_not_the_body() {
        let src = b"package p\n\ntype secret struct{}\n\nfunc Exported(s secret) secret {\n\tvar other secret\n\t_ = other\n\treturn s\n}\n";
        let facts = extract("a.go", src);
        let sig = decl(&facts, "Exported").signature_span.expect("callable");
        // Line 5: `func Exported(s secret) secret {` — signature ends where the body block starts.
        assert_eq!(sig.start.0, 5);
        assert_eq!(sig.end.0, 5);
        // The two `secret` type uses in the signature fall inside it; the body's doesn't.
        let sig_uses: Vec<_> = facts
            .references
            .iter()
            .filter(|r| {
                r.name.as_str() == "secret" && r.span.start.0 == 5 && r.kind == RefKind::TypeUse
            })
            .collect();
        assert_eq!(sig_uses.len(), 2, "param + result type positions");
        assert!(
            facts
                .references
                .iter()
                .any(|r| r.name.as_str() == "secret" && r.span.start.0 == 6),
            "the body's type use is still a reference, just outside the signature span"
        );
    }

    #[test]
    fn methods_get_signature_spans_and_types_and_vars_do_not() {
        let src = b"package p\n\ntype T struct{}\n\nfunc (t T) M(x int) {}\n\nvar V = 1\n";
        let facts = extract("a.go", src);
        assert!(decl(&facts, "M").signature_span.is_some());
        assert!(decl(&facts, "T").signature_span.is_none());
        assert!(decl(&facts, "V").signature_span.is_none());
    }

    #[test]
    fn type_positions_are_tagged_type_use_and_value_positions_read() {
        let src = b"package p\n\ntype secret struct{}\n\nfunc F(s secret) {\n\tG(s)\n}\n\nfunc G(secret2 any) {}\n";
        let facts = extract("a.go", src);
        let by_name = |n: &str| {
            facts
                .references
                .iter()
                .find(|r| r.name.as_str() == n)
                .unwrap_or_else(|| panic!("no reference {n:?}"))
        };
        assert_eq!(by_name("secret").kind, RefKind::TypeUse);
        assert_eq!(by_name("G").kind, RefKind::Read);
        assert_eq!(by_name("s").kind, RefKind::Read);
    }

    // ------------------------------------------------- function metrics

    #[test]
    fn function_bodies_emit_metrics_with_fingerprints_when_big_enough() {
        let body: String = (0..12)
            .map(|i| format!("\tx{i} := compute({i}) + compute({i}+1)\n"))
            .collect();
        let src = format!(
            "package p\n\nfunc compute(n int) int {{ return n }}\n\nfunc Big() {{\n{body}}}\n"
        );
        let facts = extract("a.go", src.as_bytes());
        let big = facts
            .functions
            .iter()
            .find(|f| f.symbol.as_str() == "Big")
            .expect("metrics for Big");
        assert!(!big.fingerprints.is_empty());
        assert!(big.loc >= 12);
        // The tiny helper still gets metrics — just no fingerprints (min-tokens gate).
        let small = facts
            .functions
            .iter()
            .find(|f| f.symbol.as_str() == "compute")
            .expect("metrics for compute");
        assert!(small.fingerprints.is_empty());
        assert_eq!(small.cyclomatic, 1);
    }

    #[test]
    fn methods_emit_metrics_under_their_qualified_name() {
        let facts = extract(
            "a.go",
            b"package p\n\ntype T struct{}\n\nfunc (t T) M() int { return 1 }\n",
        );
        assert!(facts.functions.iter().any(|f| f.symbol.as_str() == "T.M"));
    }

    #[test]
    fn renamed_clone_bodies_fingerprint_identically() {
        let mk = |name: &str, var: &str| {
            let body: String = (0..12)
                .map(|i| format!("\t{var}{i} := work({i}) + work({i}+2)\n"))
                .collect();
            format!(
                "package p\n\nfunc work(n int) int {{ return n }}\n\nfunc {name}() {{\n{body}}}\n"
            )
        };
        let a = extract("a.go", mk("First", "x").as_bytes());
        let b = extract("b.go", mk("Second", "y").as_bytes());
        let fa = &a
            .functions
            .iter()
            .find(|f| f.symbol.as_str() == "First")
            .unwrap()
            .fingerprints;
        let fb = &b
            .functions
            .iter()
            .find(|f| f.symbol.as_str() == "Second")
            .unwrap()
            .fingerprints;
        assert!(!fa.is_empty());
        assert_eq!(fa, fb);
    }

    // ------------------------------------------------- detected_origin

    #[test]
    fn the_go_generated_banner_sets_detected_origin() {
        use kndo_core::vocab::FileOrigin;
        let src =
            b"// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage pb\n\nfunc Dead() {}\n";
        assert_eq!(
            extract("a.pb.go", src).detected_origin,
            Some(FileOrigin::Generated)
        );
        assert_eq!(
            extract("a.go", b"package p\n\nfunc F() {}\n").detected_origin,
            None
        );
    }

    #[test]
    fn a_generator_emitting_the_marker_in_a_string_is_not_generated() {
        let src = b"package gen\n\nfunc emit() {\n\tprintln(\"// Code generated by x. DO NOT EDIT.\")\n}\n";
        assert_eq!(extract("gen.go", src).detected_origin, None);
    }

    #[test]
    fn embedded_struct_field_is_tagged_extend() {
        let src =
            b"package p\n\ntype Base struct{}\n\ntype Derived struct {\n\tBase\n\tNamed Base\n}\n";
        let facts = extract("a.go", src);
        let kinds: Vec<_> = facts
            .references
            .iter()
            .filter(|r| r.name.as_str() == "Base")
            .map(|r| r.kind)
            .collect();
        // Line 6's bare `Base` is embedding (Extend); line 7's `Named Base` is a plain
        // field type (TypeUse).
        assert_eq!(kinds, vec![RefKind::Extend, RefKind::TypeUse]);
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

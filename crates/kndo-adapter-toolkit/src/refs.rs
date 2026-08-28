//! The reference-emission driver: one body walk, many grammars.
//!
//! Two adapters (Kotlin, Swift) had this whole walk copied between them — the recursion, the
//! call/identifier handling, the comment skip — differing only in which node kinds their
//! grammars spell things with. That is the same split [`crate::metrics::MetricsSyntax`] made
//! for metrics, and it is made the same way here: the **driver** is written once, the **kind
//! table** is the adapter's own report about its grammar.
//!
//! What deliberately does NOT live here is anything a grammar decides differently rather than
//! spells differently — a dotted access chain, for instance: Kotlin's grammar exposes its parts
//! positionally and Swift's by field name, so each adapter emits that reference itself and
//! hands the driver a function pointer to it. Forcing one shape over two unrelated grammars is
//! what the toolkit's charter forbids; sharing a traversal they genuinely share is what it is
//! for.

use kndo_core::adapter::{FileFacts, RawReference};
use kndo_core::vocab::RefKind;
use smol_str::SmolStr;
use tree_sitter::Node;

use crate::parsing::{handler_for, last_identifier_text, span, text};

/// One node kind's handler in the body walk. Receives the syntax table so it can recurse
/// through [`walk_body`] — a handler that consumes part of a subtree and walks the rest is the
/// normal case, not the exception.
pub type BodyHandler = fn(Node, &[u8], Option<&str>, &mut FileFacts, &BodySyntax);

/// A grammar's body-walk vocabulary, as data.
pub struct BodySyntax {
    /// Kinds skipped entirely, subtree and all — comments. A grammar spells these differently
    /// (`line_comment`/`multiline_comment` vs `comment`) and gets them wrong silently: a
    /// comment mentioning a symbol name would otherwise emit a phantom reference.
    pub comment_kinds: &'static [&'static str],
    /// The leaf kind a bare name is spelled with (`identifier` in Kotlin, `simple_identifier`
    /// in Swift) — both as a callee and as a standalone read.
    pub identifier_kind: &'static str,
    /// The kind a nested type expression has (`user_type`), used when reading a name out of a
    /// type position.
    pub type_nesting_kind: &'static str,
    /// The leaf kind a *type* name is spelled with — the same as `identifier_kind` in some
    /// grammars and a distinct `type_identifier` in others.
    pub type_identifier_kind: &'static str,
    /// The kind of a dotted access chain, whose parts only the adapter knows how to take
    /// apart (see the module doc).
    pub navigation_kind: &'static str,
    /// Emits the reference a dotted access chain carries, plus whatever its qualifier is
    /// worth. Takes no [`BodySyntax`]: only an adapter ever implements this, and an adapter
    /// reaches its own table directly — the parameter exists on [`BodyHandler`] precisely
    /// because a handler there MAY be toolkit code, and this never is.
    pub emit_navigation_ref: fn(Node, &[u8], Option<&str>, &mut FileFacts, RefKind),
    /// Whether an identifier at this position is a *reference* rather than a binding site —
    /// each grammar's own parent-kind rule, and the difference between reading a name and
    /// declaring it.
    pub is_reference_position: fn(Node) -> bool,
    /// Per-kind handlers; anything absent is walked through into its children.
    pub handlers: &'static [(&'static str, BodyHandler)],
}

/// Walks an expression/statement body, emitting the references it finds. A kind with a handler
/// is handed to it and NOT descended into (the handler owns its subtree); anything else is
/// descended through.
pub fn walk_body(
    node: Node,
    src: &[u8],
    within: Option<&str>,
    out: &mut FileFacts,
    syntax: &BodySyntax,
) {
    if syntax.comment_kinds.contains(&node.kind()) {
        return;
    }
    if let Some(handler) = handler_for(node.kind(), syntax.handlers) {
        handler(node, src, within, out, syntax);
        return;
    }
    for child in node.children(&mut node.walk()) {
        walk_body(child, src, within, out, syntax);
    }
}

/// A call: its callee becomes a `Call` reference, its arguments are ordinary body.
pub fn handle_call(
    node: Node,
    src: &[u8],
    within: Option<&str>,
    out: &mut FileFacts,
    syntax: &BodySyntax,
) {
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor);
    let Some(callee) = children.next() else {
        return;
    };
    emit_call_ref(callee, src, within, out, syntax);
    for child in children {
        walk_body(child, src, within, out, syntax);
    }
}

/// The `Call` reference a callee carries: a bare name directly, a dotted chain through the
/// adapter's own navigation emitter, anything else (a parenthesized expression, an immediately
/// invoked closure) by walking into it — a callee that isn't a name is still code.
pub fn emit_call_ref(
    callee: Node,
    src: &[u8],
    within: Option<&str>,
    out: &mut FileFacts,
    syntax: &BodySyntax,
) {
    let kind = callee.kind();
    if kind == syntax.identifier_kind {
        out.references.push(RawReference {
            name: SmolStr::new(text(callee, src)),
            scope_context: None,
            span: span(callee),
            within: within.map(SmolStr::new),
            kind: RefKind::Call,
        });
    } else if kind == syntax.navigation_kind {
        (syntax.emit_navigation_ref)(callee, src, within, out, RefKind::Call);
    } else {
        walk_body(callee, src, within, out, syntax);
    }
}

/// A bare identifier in reference position becomes a `Read`; in binding position it is the
/// declaration's own name and emits nothing.
pub fn handle_identifier_ref(
    node: Node,
    src: &[u8],
    within: Option<&str>,
    out: &mut FileFacts,
    syntax: &BodySyntax,
) {
    if !(syntax.is_reference_position)(node) {
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

/// The `Extend` reference a supertype/conformance clause carries, attributed to `owner` and
/// spanned at the clause itself. A type expression that names nothing usable (a closure type,
/// a tuple) emits nothing rather than guessing.
pub fn emit_extend_ref(
    ty: Option<Node>,
    site: Node,
    src: &[u8],
    owner: &str,
    out: &mut FileFacts,
    syntax: &BodySyntax,
) {
    let Some(name) = ty.and_then(|t| {
        last_identifier_text(
            t,
            src,
            syntax.type_identifier_kind,
            syntax.type_nesting_kind,
        )
    }) else {
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

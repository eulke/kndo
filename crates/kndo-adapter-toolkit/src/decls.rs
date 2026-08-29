//! Declaration emission — the base fact, written once.
//!
//! Every adapter pushes the same thirteen fields in the same order, differing only in
//! whether their grammar carries markers. The fields this does not take
//! (`implicitly_invoked`, `nested_scope`, `visibility_inherited`, `visible_in_unit`,
//! `implements`) are not decisions an adapter makes *here*: every adapter that reports one
//! sets it AFTERWARDS, by name, on the declaration it just pushed (`decl.nested_scope =
//! true`, `mark_implicitly_invoked(…)`) — naming a field only inside the constructor call
//! would let it go unset silently at every call site that forgets it.

use kndo_core::adapter::{Declaration, FileFacts, Span, VisibilityLevel};
use kndo_core::vocab::SymbolKind;
use smol_str::SmolStr;
use tree_sitter::Node;

use crate::parsing::span;

/// Pushes the base declaration for `item`: its name, kind, extent and visibility, plus the
/// markers its grammar carries (an empty `Vec` where the language has none). Richer facts are
/// added by their own named mutators after this returns.
///
/// `(level, exported)` arrives as one pair because it is one answer: every adapter computes
/// both from the same visibility read, and splitting them into two parameters invites a call
/// site that passes a level from one declaration and an export flag from another.
#[allow(clippy::too_many_arguments)]
pub fn push_declaration(
    out: &mut FileFacts,
    name: &str,
    kind: SymbolKind,
    item: Node,
    signature_span: Option<Span>,
    member_of: Option<&str>,
    (level, exported): (u8, bool),
    markers: Vec<SmolStr>,
) {
    out.declarations.push(Declaration {
        name: SmolStr::new(name),
        kind,
        span: span(item),
        exported,
        visibility: VisibilityLevel(level),
        member_of: member_of.map(SmolStr::new),
        implicitly_invoked: false,
        nested_scope: false,
        visibility_inherited: false,
        visible_in_unit: None,
        implements: None,
        markers,
        signature_span,
    });
}

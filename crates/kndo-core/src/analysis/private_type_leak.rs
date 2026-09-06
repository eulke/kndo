//! A public promise naming a type consumers cannot name: an exported callable
//! whose SIGNATURE references a private type — the other direction of the
//! visibility-mismatch pair (`internal-only` finds visibility above use; this
//! finds visibility below it). Only the signature is a promise: a private type
//! used inside the body is ordinary encapsulation, and the adapter marks the
//! promise region itself ([`kndo_contract::evidence::Declaration::signature_span`]
//! — absent means silence, never accusation).
//!
//! The precision floor, each rule bought with a measured oracle vice:
//! - **Exported-vs-Private only.** Any `Scoped` reach on either side is
//!   silence: v1 folded rust's `pub(crate)` to exported and accused crate-wide
//!   methods of leaking crate-wide types (ripgrep, four findings about nothing),
//!   and flagged Java package-private types in package-visible signatures. A
//!   scoped region compared against a scoped region needs region semantics,
//!   not a rung ladder — that judgment waits for a consumer who needs it.
//! - **The effective surface is the whole owner chain.** An exported-looking
//!   member of a non-exported container is not public API.
//! - **Same-file, unique resolution.** The leaked type must resolve to exactly
//!   one declaration in the same file — a name that could mean two things
//!   accuses neither.
//! - **A file that is itself a test** (carries its own Test root) makes no
//!   public promise — guava's oracle findings were test-support constructors.

use super::{Analysis, AnalysisContext, has_root_of};
use kndo_contract::evidence::{Reach, RefKind, RootKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::vocab::{Category, Confidence};

pub struct PrivateTypeLeak;

impl Analysis for PrivateTypeLeak {
    fn id(&self) -> &'static str {
        "private-type-leak"
    }

    fn category(&self) -> Category {
        Category::PRIVATE_TYPE_LEAK
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            if has_root_of(g, i, RootKind::Test) {
                continue;
            }
            let decls = &f.evidence.declarations;
            for (id, d) in f.evidence.declarations_with_ids() {
                let Some(sig) = d.signature_span else {
                    continue;
                };
                if !chain_is_exported(decls, d) {
                    continue;
                }
                for r in &f.evidence.references {
                    if r.kind != RefKind::TypeUse
                        || r.span.start < sig.start
                        || r.span.end > sig.end
                    {
                        continue;
                    }
                    let mut named = decls.iter().filter(|t| t.name == r.name);
                    let (Some(leaked), None) = (named.next(), named.next()) else {
                        continue; // unresolved or ambiguous — accuse neither
                    };
                    if !chain_is_private(decls, leaked) {
                        continue;
                    }
                    out.push(Finding::new(
                        Category::PRIVATE_TYPE_LEAK,
                        Severity::Warning,
                        Confidence::Certain,
                        f.evidence.subject_of(&f.path, id),
                        r.name.as_str(),
                        format!(
                            "exported, but its signature references `{}`, which is private — \
                             consumers can call this and never name that type",
                            r.name
                        ),
                    ));
                }
            }
        }
        out.sort_by(|a, b| {
            (a.subject.path(), a.id.as_str()).cmp(&(b.subject.path(), b.id.as_str()))
        });
        out.dedup_by(|a, b| a.id == b.id);
        out
    }
}

/// Every link of the owner chain, self included, is exported surface — which
/// is what an `Exported` effective reach says: an owner narrower than
/// exported would have capped it. A heirs member counts: what it promises,
/// it promises to subtypes anywhere.
fn chain_is_exported(
    decls: &[kndo_contract::evidence::Declaration],
    d: &kndo_contract::evidence::Declaration,
) -> bool {
    let mut cursor = d;
    loop {
        if !matches!(
            cursor.reach,
            Reach::Exported | Reach::Inherited | Reach::Heirs { .. }
        ) {
            return false;
        }
        match cursor.owner {
            Some(owner) => cursor = &decls[owner.index()],
            None => return matches!(cursor.reach, Reach::Exported),
        }
    }
}

/// The declaration's own reach — or any link of its chain — stops at its
/// owner or its file, and no link is bounded any other way (a namespace, a
/// unit, a token: not this analysis's judgment).
fn chain_is_private(
    decls: &[kndo_contract::evidence::Declaration],
    d: &kndo_contract::evidence::Declaration,
) -> bool {
    let mut cursor = d;
    let mut any_private = false;
    loop {
        match cursor.reach {
            Reach::Owner | Reach::File => any_private = true,
            Reach::Exported | Reach::Inherited => {}
            _ => return false, // bounded otherwise, or a future variant: silence
        }
        match cursor.owner {
            Some(owner) => cursor = &decls[owner.index()],
            None => return any_private,
        }
    }
}

//! Declared wider than it is used: a `Scoped` declaration whose every use sits
//! inside its OWN file could take the language's narrower rung. Fires only for
//! scope tokens the claiming adapter listed as narrowable — Java can demote
//! "package" to private, Go has nothing below "package", so identical evidence
//! is advice in one language and noise in the other. Always `Possible`/`Info`:
//! name pools cannot prove the absence of a qualified use the way a compiler
//! can, and `unused` outranks it — a declaration nothing uses at all is dead,
//! not demotable.

use super::{Analysis, AnalysisContext, RunContext};
use kndo_contract::evidence::{ImportShape, Reach, RootTarget, SymbolKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeSet;

pub struct InternalOnly;

impl Analysis for InternalOnly {
    fn id(&self) -> &'static str {
        "internal-only"
    }

    fn category(&self) -> Category {
        Category::INTERNAL_ONLY
    }

    fn abstains(&self, run: &RunContext<'_>) -> Option<super::AbstentionReason> {
        // Same posture as unused: a rootless graph judges nothing.
        let any_root = run.graph.files.iter().any(|f| f.is_rooted());
        (!run.graph.files.is_empty() && !any_root)
            .then_some(super::AbstentionReason::NoRootsAnywhere)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let reachable = |i: usize| cx.run.reach.any(i);

        // Names bound out of each file, and names referenced OUTSIDE each file —
        // one pass; a name in either set is used beyond its declaration site.
        let mut bound_names: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut per_file_refs: Vec<BTreeSet<&str>> = Vec::with_capacity(g.files.len());
        for f in &g.files {
            per_file_refs.push(
                f.evidence
                    .references
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect(),
            );
        }
        for (i, f) in g.files.iter().enumerate() {
            if !reachable(i) {
                continue;
            }
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                for &t in targets {
                    match &import.shape {
                        ImportShape::Bindings(bs)
                        | ImportShape::Reexport(bs)
                        | ImportShape::TypeOnly(bs) => {
                            for b in bs {
                                bound_names.insert((t, b.imported.as_str()));
                            }
                        }
                        // A namespace/glob importer may use anything: treat the
                        // whole target as used from outside.
                        _ => {
                            bound_names.insert((t, ""));
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] || !reachable(i) {
                continue;
            }
            let Some(narrowable) = cx
                .run
                .narrowables
                .iter()
                .find(|(coord, _)| *coord == f.adapter)
                .map(|(_, tokens)| tokens)
            else {
                continue;
            };
            if narrowable.is_empty() {
                continue;
            }
            // The whole file was namespace-imported: anything here may be used.
            if bound_names.contains(&(i as u32, "")) {
                continue;
            }
            let rooted: BTreeSet<usize> = f
                .evidence
                .roots
                .iter()
                .chain(&f.anchored)
                .filter_map(|r| match &r.target {
                    RootTarget::Declaration(id) => Some(id.index()),
                    _ => None,
                })
                .collect();
            let region_ids = |scope: &str| -> Option<&[u32]> {
                f.scoped_regions
                    .binary_search_by(|(t, _)| t.as_str().cmp(scope))
                    .ok()
                    .map(|ix| f.scoped_regions[ix].1.as_slice())
            };
            for (d_ix, d) in f.evidence.declarations.iter().enumerate() {
                let Reach::Scoped { scope } = &d.reach else {
                    continue;
                };
                if !narrowable.iter().any(|t| t == scope) {
                    continue;
                }
                // Unbounded token ⇒ not judgeable here either.
                if region_ids(scope).is_none() {
                    continue;
                }
                // A rooted declaration is used from outside the graph's sight.
                if rooted.contains(&d_ix) || d.owner.is_some_and(|o| rooted.contains(&o.index())) {
                    continue;
                }
                // Used INSIDE its own file at all? If not, `unused` owns it.
                let own_use = per_file_refs[i].contains(d.name.as_str());
                if !own_use {
                    continue;
                }
                // Any CONFIDENT use beyond the file disqualifies: a binding
                // importer, or a reference in another file of the REGION — the
                // only files that can legally resolve the name. Same-named
                // references OUTSIDE the region are exactly v1's "weaker
                // matches point outside": they cannot be this symbol, and
                // `Possible` carries the residual dispatch fuzz (a scoped
                // method reached through a public supertype stays exported by
                // its own modifiers, so it never sits here).
                let used_beyond = bound_names.contains(&(i as u32, d.name.as_str()))
                    || d.exported_as
                        .as_ref()
                        .is_some_and(|a| bound_names.contains(&(i as u32, a.as_str())))
                    || region_ids(scope).is_some_and(|r| {
                        r.iter().any(|&j| {
                            j as usize != i
                                && reachable(j as usize)
                                && per_file_refs[j as usize].contains(d.name.as_str())
                        })
                    });
                if used_beyond {
                    continue;
                }
                let selector = match d.owner {
                    Some(owner) => SymbolSelector::Member {
                        owner: f.evidence.declarations[owner.index()].name.clone(),
                        name: d.name.clone(),
                    },
                    None => SymbolSelector::Free(d.name.clone()),
                };
                let noun = match d.kind {
                    SymbolKind::Function | SymbolKind::Method => "function",
                    SymbolKind::Type => "type",
                    _ => "declaration",
                };
                out.push(Finding::new(
                    Category::INTERNAL_ONLY,
                    Severity::Info,
                    Confidence::Possible,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    format!(
                        "declared `{scope}`-scoped, but every use is within its own file — \
                         the narrower rung would suffice for this {noun}"
                    ),
                ));
            }
        }
        out
    }
}

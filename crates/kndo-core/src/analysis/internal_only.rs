//! Declared wider than it is used: a declaration whose every use sits inside
//! its OWN file could take the language's narrower visibility. Two rungs, one
//! claim, each gated by what the claiming adapter declared:
//!
//! - a `Scoped` declaration, for scope tokens listed as narrowable — Java can
//!   demote "package" to private, Go has nothing below "package", so identical
//!   evidence is advice in one language and noise in the other. Disqualified by
//!   any use across its ENUMERATED region (the only files that can legally
//!   resolve the name), pooling reachable files.
//! - an `Exported` declaration, where the adapter declared narrowing
//!   expressible ([`kndo_contract::extension::ExportNarrowing`]) — TypeScript
//!   can drop `export` and tsc turns any missed use into a compile error. An
//!   Exported name is nameable from ANYWHERE, so the disqualifier is total: a
//!   binding import, or a same-named reference in any other claimed file —
//!   reachable or not, because an unreachable file still compiles against the
//!   export it spells (vite's `__tests_dts__` type-tests proved that vice).
//!   A whole-file-rooted file is exempt: an entry's exports are the outside
//!   world's surface, and a test's are its runner's.
//!
//! `Probable`/`Info`, derived from this analysis's own evidence: the residuals
//! below `Certain` are reflection and dynamic access (out of static scope
//! everywhere in kndo), name-pool collisions, and — for the Exported rung —
//! importers in files no adapter claims. `unused` outranks it: a declaration
//! nothing uses at all is dead, not demotable.

use super::{Analysis, AnalysisContext, RunContext};
use kndo_contract::evidence::{ImportShape, Reach, RootTarget, SymbolKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::{BTreeMap, BTreeSet};

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
        // Two sets: the Scoped rung pools reachable importers (its region is
        // enumerated); the Exported rung pools EVERY importer, because even an
        // unreachable file compiles against what it imports.
        let mut bound_names: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut bound_all: BTreeSet<(u32, &str)> = BTreeSet::new();
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
        // How many claimed files spell each name at all — the Exported rung's
        // total-absence check: given a use in its own file, a count of two or
        // more means someone else names it too.
        let mut name_files: BTreeMap<&str, u32> = BTreeMap::new();
        for refs in &per_file_refs {
            for name in refs {
                *name_files.entry(name).or_insert(0) += 1;
            }
        }
        for (i, f) in g.files.iter().enumerate() {
            let from_reachable = reachable(i);
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                for &t in targets {
                    match &import.shape {
                        ImportShape::Bindings(bs) | ImportShape::Reexport(bs) => {
                            for b in bs {
                                bound_all.insert((t, b.imported.as_str()));
                                if from_reachable {
                                    bound_names.insert((t, b.imported.as_str()));
                                }
                            }
                        }
                        // A namespace/glob importer may use anything: treat the
                        // whole target as used from outside.
                        _ => {
                            bound_all.insert((t, ""));
                            if from_reachable {
                                bound_names.insert((t, ""));
                            }
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
            let narrowable: &[smol_str::SmolStr] = cx
                .run
                .narrowables
                .iter()
                .find(|(coord, _)| *coord == f.adapter)
                .map(|(_, tokens)| tokens.as_slice())
                .unwrap_or(&[]);
            let export_narrowable = cx.run.export_narrowables.contains(&f.adapter);
            if narrowable.is_empty() && !export_narrowable {
                continue;
            }
            // The whole file was namespace-imported: anything here may be used.
            // The Exported rung honors even an unreachable such importer.
            let scoped_open = !bound_names.contains(&(i as u32, ""));
            let exported_open = !bound_all.contains(&(i as u32, ""));
            // An entry, test, or tooling file: its exports ARE an outside
            // surface (a manifest's consumers, a runner), so the Exported rung
            // stays silent for the whole file.
            let whole_file_rooted = f
                .evidence
                .roots
                .iter()
                .chain(&f.anchored)
                .any(|r| matches!(r.target, RootTarget::WholeFile));
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
                let scope = match &d.reach {
                    Reach::Scoped { scope } => {
                        if !scoped_open || !narrowable.iter().any(|t| t == scope) {
                            continue;
                        }
                        // Unbounded token ⇒ not judgeable here either.
                        if region_ids(scope).is_none() {
                            continue;
                        }
                        Some(scope)
                    }
                    Reach::Exported => {
                        // Owned members wait for their own demand; the floor is
                        // the file's own top-level surface.
                        if !export_narrowable
                            || !exported_open
                            || whole_file_rooted
                            || d.owner.is_some()
                        {
                            continue;
                        }
                        None
                    }
                    _ => continue,
                };
                // A rooted declaration is used from outside the graph's sight.
                if rooted.contains(&d_ix) || d.owner.is_some_and(|o| rooted.contains(&o.index())) {
                    continue;
                }
                // Used INSIDE its own file at all? If not, `unused` owns it.
                let own_use = per_file_refs[i].contains(d.name.as_str());
                if !own_use {
                    continue;
                }
                let used_beyond = match scope {
                    // Any use beyond the file disqualifies: a binding importer,
                    // or a reference in another file of the REGION — the only
                    // files that can legally resolve the name. Same-named
                    // references OUTSIDE the region cannot be this symbol, so
                    // they neither keep nor disqualify. (A scoped method
                    // reached through a public supertype stays exported by its
                    // own modifiers, so it never sits here.)
                    Some(scope) => {
                        bound_names.contains(&(i as u32, d.name.as_str()))
                            || d.exported_as
                                .as_ref()
                                .is_some_and(|a| bound_names.contains(&(i as u32, a.as_str())))
                            || region_ids(scope).is_some_and(|r| {
                                r.iter().any(|&j| {
                                    j as usize != i
                                        && reachable(j as usize)
                                        && per_file_refs[j as usize].contains(d.name.as_str())
                                })
                            })
                    }
                    // Total absence for the Exported rung: any binding importer
                    // (reachable or not), or the name spelled in ANY other
                    // claimed file.
                    None => {
                        bound_all.contains(&(i as u32, d.name.as_str()))
                            || d.exported_as
                                .as_ref()
                                .is_some_and(|a| bound_all.contains(&(i as u32, a.as_str())))
                            || name_files.get(d.name.as_str()).copied().unwrap_or(0) >= 2
                    }
                };
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
                let message = match scope {
                    Some(scope) => format!(
                        "declared `{scope}`-scoped, but every use is within its own file — \
                         the narrower rung would suffice for this {noun}"
                    ),
                    None => format!(
                        "declared exported, but every use is within its own file — nothing \
                         else in the tree imports or names it; the narrower visibility \
                         would suffice for this {noun}"
                    ),
                };
                out.push(Finding::new(
                    Category::INTERNAL_ONLY,
                    Severity::Info,
                    Confidence::Probable,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    message,
                ));
            }
        }
        out
    }
}

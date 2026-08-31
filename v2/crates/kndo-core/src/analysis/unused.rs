//! Unreachable code: a claimed file no color reaches is unused; within reachable
//! files, a declaration nothing references, roots, or imports is unused. Dead is
//! always `Certain` — color outranks confidence.
//!
//! Reach decides WHICH evidence can keep a declaration: `Private` pools its own
//! file plus the files that see it; `Scoped { scope }` pools its REGION (the
//! adapter's answer for that token) and is never part of the surface an entry
//! or namespace import hands out from OUTSIDE the region; `Exported` — and any
//! Scoped token the adapter could not bound — is published surface, kept by
//! entries and namespace importers wholesale.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext};
use kndo_contract::evidence::{ImportShape, Reach, RootTarget, SymbolKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::{BTreeMap, BTreeSet};

pub struct Unused;

impl Analysis for Unused {
    fn id(&self) -> &'static str {
        "unused"
    }

    fn category(&self) -> Category {
        Category::UNUSED
    }

    fn abstains(&self, run: &RunContext<'_>) -> Option<AbstentionReason> {
        let any_root = run.graph.files.iter().any(|f| f.is_rooted());
        (!run.graph.files.is_empty() && !any_root).then_some(AbstentionReason::NoRootsAnywhere)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let n = g.files.len();
        let reachable = |i: usize| cx.run.reach.any(i);

        // Names each file's imports bind from THEIR resolved target, and WHO keeps
        // a target's whole surface alive (namespace/side-effect: the engine cannot
        // see through them, so it degrades toward keep-alive) — the importer set,
        // because a Scoped declaration is handed out only to importers inside its
        // region. Member references dispatch through values, not lexical scope,
        // so their evidence pool is every reachable file's references at once.
        let mut bound: BTreeMap<(u32, &str), Vec<u32>> = BTreeMap::new();
        let mut surface_importers: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut member_referenced: BTreeSet<&str> = BTreeSet::new();
        // Shared-scope units (a Go package): a file's declarations are visible to
        // its unit mates with no import naming them, so the references that can
        // keep a declaration include every reachable file that SEES its file —
        // exported and private alike. `seen_by` is the reverse of `sees`.
        let mut seen_by: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (i, f) in g.files.iter().enumerate() {
            if !reachable(i) {
                continue;
            }
            for r in &f.evidence.references {
                member_referenced.insert(r.name.as_str());
            }
            for &m in &f.sees {
                seen_by[m as usize].push(i as u32);
            }
            for (import, targets) in f.evidence.imports.iter().zip(&f.import_targets) {
                for &t in targets {
                    match &import.shape {
                        ImportShape::Bindings(bs)
                        | ImportShape::Reexport(bs)
                        | ImportShape::TypeOnly(bs) => {
                            for b in bs {
                                bound
                                    .entry((t, b.imported.as_str()))
                                    .or_default()
                                    .push(i as u32);
                            }
                        }
                        _ => surface_importers[t as usize].push(i as u32),
                    }
                }
            }
        }

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            if !reachable(i) {
                out.push(Finding::new(
                    Category::UNUSED,
                    Severity::Warning,
                    Confidence::Certain,
                    Subject::File {
                        path: f.path.clone(),
                    },
                    "",
                    "no root anchors this file and no reachable file imports it",
                ));
                continue;
            }
            let mut referenced: BTreeSet<&str> = f
                .evidence
                .references
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            for &viewer in &seen_by[i] {
                referenced.extend(
                    g.files[viewer as usize]
                        .evidence
                        .references
                        .iter()
                        .map(|r| r.name.as_str()),
                );
            }
            // Extraction evidence and outside anchors (manifest entries, plugin
            // roots) keep declarations the same way.
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
            // A whole-file root anchors the FILE's reachability AND hands its exported
            // surface to whoever rooted it (a package consumer, a test runner, a
            // tool). Private declarations are still judged individually — a private,
            // uncalled function in an entry point is dead code.
            let entry_surface = f
                .evidence
                .roots
                .iter()
                .chain(&f.anchored)
                .any(|r| matches!(r.target, RootTarget::WholeFile));
            // A region answered for a token maps to the pool of names its files
            // reference; an unanswered token stays None = Exported treatment.
            let region_of = |scope: &str| -> Option<&[u32]> {
                f.scoped_regions
                    .binary_search_by(|(t, _)| t.as_str().cmp(scope))
                    .ok()
                    .map(|ix| f.scoped_regions[ix].1.as_slice())
            };
            for (d_ix, d) in f.evidence.declarations.iter().enumerate() {
                // Judged-as-exported: truly Exported, or Scoped with no bounded
                // region (keep-alive: an unanswerable token accuses nothing).
                let (exported, region) = match &d.reach {
                    Reach::Exported => (true, None),
                    Reach::Scoped { scope } => match region_of(scope) {
                        Some(r) => (false, Some(r)),
                        None => (true, None),
                    },
                    Reach::Private => (false, None),
                };
                // Method-kind declarations are members even without an owner in this
                // file — receivers can name a type declared elsewhere — so they share
                // the member pool and lean on their own reach for the surface rule.
                let member = d.owner.is_some() || d.kind == SymbolKind::Method;
                let kept = if member {
                    // A member: kept by any reference to its name anywhere reachable
                    // (dispatch is not lexical), by a root, or by its owner's whole
                    // surface being kept from outside — the entry handing it out, or
                    // an importer binding the owner by name (a re-export chain makes
                    // the owner's exported members published surface).
                    let surface_reach = match d.owner {
                        Some(owner) => &f.evidence.declarations[owner.index()].reach,
                        None => &d.reach,
                    };
                    let owner_surface_exported = matches!(surface_reach, Reach::Exported)
                        || matches!(surface_reach, Reach::Scoped { scope }
                            if region_of(scope).is_none());
                    // A member with a BOUNDED region is not part of what an entry
                    // hands out, even on an exported owner: an `internal` method
                    // of a public Kotlin class is uncallable outside its module,
                    // a lowercase Go method outside its package. Private members
                    // keep the owner-surface caution (dispatch is not lexical).
                    let member_handed_out = !region.is_some();
                    let surface_exported = owner_surface_exported && member_handed_out;
                    // An importer binding the owner's name keeps its dispatchable
                    // members — but a member with a BOUNDED region rides only
                    // binders INSIDE that region: an out-of-module import of a
                    // public class hands out its public members, never its
                    // internal ones.
                    let binder_counts = |name: &str| -> bool {
                        bound
                            .get(&(i as u32, name))
                            .is_some_and(|importers| match region {
                                None => true,
                                Some(r) => importers.iter().any(|imp| r.contains(imp)),
                            })
                    };
                    // …and a PRIVATE member never rides at all: it is not handed
                    // out with the owner, only its own references keep it.
                    let owner_bound = !matches!(d.reach, Reach::Private)
                        && d.owner.is_some_and(|o| {
                            let od = &f.evidence.declarations[o.index()];
                            !matches!(od.reach, Reach::Private)
                                && (binder_counts(od.name.as_str())
                                    || od
                                        .exported_as
                                        .as_ref()
                                        .is_some_and(|a| binder_counts(a.as_str())))
                        });
                    member_referenced.contains(d.name.as_str())
                        || rooted.contains(&d_ix)
                        || d.owner.is_some_and(|o| rooted.contains(&o.index()))
                        || !surface_importers[i].is_empty()
                        || (entry_surface && surface_exported)
                        || owner_bound
                } else {
                    // Importers bind the module-system name: the local one, or the
                    // exported alias when the declaration carries one. A binding
                    // keeps its declaration whatever the reach — the adapter
                    // resolved that edge as legal (some module systems let a child
                    // bind its parent's private), and a binding into genuinely
                    // unreachable code is broken code, never license to accuse.
                    // A direct binding of the name keeps it whatever the reach —
                    // the adapter resolved that edge as legal (some module systems
                    // let a child bind a parent's private).
                    let bound_by_name = bound.contains_key(&(i as u32, d.name.as_str()))
                        || d.exported_as
                            .as_ref()
                            .is_some_and(|a| bound.contains_key(&(i as u32, a.as_str())));
                    // A Scoped declaration's extra lifelines: any reference in
                    // its region's files (qualified in-region uses need no
                    // import), or a namespace importer INSIDE the region.
                    let region_kept = region.is_some_and(|r| {
                        r.iter().any(|&m| {
                            reachable(m as usize)
                                && (g.files[m as usize]
                                    .evidence
                                    .references
                                    .iter()
                                    .any(|rf| rf.name == d.name)
                                    || surface_importers[i].contains(&m))
                        })
                    });
                    referenced.contains(d.name.as_str())
                        || rooted.contains(&d_ix)
                        || bound_by_name
                        || region_kept
                        || (exported && (!surface_importers[i].is_empty() || entry_surface))
                };
                if !kept {
                    let selector = match d.owner {
                        Some(owner) => SymbolSelector::Member {
                            owner: f.evidence.declarations[owner.index()].name.clone(),
                            name: d.name.clone(),
                        },
                        None => SymbolSelector::Free(d.name.clone()),
                    };
                    out.push(Finding::new(
                        Category::UNUSED,
                        Severity::Warning,
                        Confidence::Certain,
                        Subject::Symbol {
                            path: f.path.clone(),
                            selector,
                            span: d.span,
                        },
                        "",
                        "nothing references, roots, or imports this declaration",
                    ));
                }
            }
        }
        out
    }
}

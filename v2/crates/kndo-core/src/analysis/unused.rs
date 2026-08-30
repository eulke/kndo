//! Unreachable code: a claimed file no color reaches is unused; within reachable
//! files, a declaration nothing references, roots, or imports is unused. Dead is
//! always `Certain` — color outranks confidence.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext};
use kndo_contract::evidence::{ImportShape, Reach, RootTarget};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeSet;

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

        // Names each file's imports bind from THEIR resolved target, and whether an
        // importer keeps a target's whole exported surface alive (namespace/
        // side-effect: the engine cannot see through them, so it degrades toward
        // keep-alive). Member references dispatch through values, not lexical scope,
        // so their evidence pool is every reachable file's references at once.
        let mut bound: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut surface_kept = vec![false; n];
        let mut member_referenced: BTreeSet<&str> = BTreeSet::new();
        for (i, f) in g.files.iter().enumerate() {
            if !reachable(i) {
                continue;
            }
            for r in &f.evidence.references {
                member_referenced.insert(r.name.as_str());
            }
            for (import, target) in f.evidence.imports.iter().zip(&f.import_targets) {
                let Some(t) = *target else { continue };
                match &import.shape {
                    ImportShape::Bindings(bs)
                    | ImportShape::Reexport(bs)
                    | ImportShape::TypeOnly(bs) => {
                        for b in bs {
                            bound.insert((t, b.imported.as_str()));
                        }
                    }
                    _ => surface_kept[t as usize] = true,
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
            let referenced: BTreeSet<&str> = f
                .evidence
                .references
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            let rooted: BTreeSet<usize> = f
                .evidence
                .roots
                .iter()
                .filter_map(|r| match &r.target {
                    RootTarget::Declaration(id) => Some(id.index()),
                    _ => None,
                })
                .collect();
            // A whole-file root anchors the FILE's reachability AND hands its exported
            // surface to whoever rooted it (a package consumer, a test runner, a
            // tool). Private declarations are still judged individually — a private,
            // uncalled function in an entry point is dead code.
            let entry_surface = !f.anchored.is_empty()
                || f.evidence
                    .roots
                    .iter()
                    .any(|r| matches!(r.target, RootTarget::WholeFile));
            for (d_ix, d) in f.evidence.declarations.iter().enumerate() {
                let exported = d.reach == Reach::Exported;
                let kept = if let Some(owner) = d.owner {
                    // A member: kept by any reference to its name anywhere reachable
                    // (dispatch is not lexical), by a root, or by its owner's whole
                    // surface being kept from outside.
                    let owner_exported =
                        f.evidence.declarations[owner.index()].reach == Reach::Exported;
                    member_referenced.contains(d.name.as_str())
                        || rooted.contains(&d_ix)
                        || rooted.contains(&owner.index())
                        || surface_kept[i]
                        || (entry_surface && owner_exported)
                } else {
                    // Importers bind the module-system name: the local one, or the
                    // exported alias when the declaration carries one.
                    let bound_by_name = bound.contains(&(i as u32, d.name.as_str()))
                        || d.exported_as
                            .as_ref()
                            .is_some_and(|a| bound.contains(&(i as u32, a.as_str())));
                    referenced.contains(d.name.as_str())
                        || rooted.contains(&d_ix)
                        || (exported && (surface_kept[i] || bound_by_name || entry_surface))
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

//! Unreachable code: a claimed file no color reaches is unused; within reachable
//! files, a declaration nothing keeps is unused. Dead is always `Certain` —
//! color outranks confidence.
//!
//! The keep rules live in [`crate::navigate::keepers`] — ONE spelling shared
//! with the navigation verbs, so `used-by` lists exactly the evidence this
//! judgment counted. This analysis asks it with `limit 1`: emptiness is the
//! accusation.
//!
//! Dependency subjects are the same verdict on a manifest's production-scope
//! declarations, over the floor [`super::dependency`] shares with `test-only`:
//! nothing in the tree imports it and the manifest never names it. `Probable`,
//! not `Certain` — a dependency's uses are visible only where an import spells
//! them, and a runtime that injects one leaves no import to see.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext, dependency};
use crate::navigate;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};

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
        let index = &cx.run.index;

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            if !cx.run.reach.any(i) {
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
            for (id, _) in f.evidence.declarations_with_ids() {
                let d_ix = id.index();
                if !navigate::keepers(g, index, i, d_ix, 1).is_empty() {
                    continue;
                }
                out.push(Finding::new(
                    Category::UNUSED,
                    Severity::Warning,
                    Confidence::Certain,
                    f.evidence.subject_of(&f.path, id),
                    "",
                    "nothing references, roots, or imports this declaration",
                ));
            }
        }
        dependency::abstain(cx);
        for dep in dependency::judged(cx) {
            if !dep.users.is_empty() {
                continue;
            }
            out.push(Finding::new(
                Category::UNUSED,
                Severity::Warning,
                Confidence::Probable,
                Subject::Dependency {
                    owner_manifest: dep.manifest.manifest.clone(),
                    name: dep.declaration.name.clone(),
                },
                "",
                "declared as a production dependency, but no file imports it and the manifest never names it",
            ));
        }
        out
    }
}

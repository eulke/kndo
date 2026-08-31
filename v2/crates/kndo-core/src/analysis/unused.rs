//! Unreachable code: a claimed file no color reaches is unused; within reachable
//! files, a declaration nothing keeps is unused. Dead is always `Certain` —
//! color outranks confidence.
//!
//! The keep rules live in [`crate::navigate::keepers`] — ONE spelling shared
//! with the navigation verbs, so `used-by` lists exactly the evidence this
//! judgment counted. This analysis asks it with `limit 1`: emptiness is the
//! accusation.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext};
use crate::navigate;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
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
            for (d_ix, d) in f.evidence.declarations.iter().enumerate() {
                if !navigate::keepers(g, index, i, d_ix, 1).is_empty() {
                    continue;
                }
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
        out
    }
}

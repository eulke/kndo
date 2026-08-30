//! Production-reachable functions no test exercises. The graph form of the evidence:
//! a reference from a file that carries a Test root is what "a test exercises this"
//! looks like in the graph — name-level, so a same-named reference keeps a function
//! (under-accusing, never over). Coverage, when ingested, replaces this heuristic
//! with measured execution.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext, has_root_of};
use kndo_contract::evidence::{RootKind, SymbolKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use std::collections::BTreeSet;

pub struct Untested;

impl Analysis for Untested {
    fn id(&self) -> &'static str {
        "untested"
    }

    fn category(&self) -> Category {
        Category::UNTESTED
    }

    fn abstains(&self, run: &RunContext<'_>) -> Option<AbstentionReason> {
        let any_test_root =
            (0..run.graph.files.len()).any(|i| has_root_of(run.graph, i, RootKind::Test));
        (!run.graph.files.is_empty() && !any_test_root)
            .then_some(AbstentionReason::NoTestRootsAnywhere)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let reach = &cx.run.reach;

        // What tests reach, by name: every reference made inside a file that carries
        // a Test root. Coarse and keep-alive — a same-named reference anywhere in the
        // test surface counts.
        let mut tested: BTreeSet<&str> = BTreeSet::new();
        for (i, f) in g.files.iter().enumerate() {
            if has_root_of(g, i, RootKind::Test) {
                for r in &f.evidence.references {
                    tested.insert(r.name.as_str());
                }
            }
        }

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            // Only production-reachable, non-test files are judged: a test's own
            // helpers are the test's business, and unreachable code is `unused`'s
            // verdict, not two verdicts at once.
            if !reach.by(RootKind::Production)[i] || has_root_of(g, i, RootKind::Test) {
                continue;
            }
            for d in &f.evidence.declarations {
                if !matches!(d.kind, SymbolKind::Function | SymbolKind::Method) {
                    continue;
                }
                if tested.contains(d.name.as_str()) {
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
                    Category::UNTESTED,
                    Severity::Info,
                    Confidence::Probable,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    "no test references this function",
                ));
            }
        }
        out
    }
}

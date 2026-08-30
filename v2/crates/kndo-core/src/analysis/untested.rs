//! Production-reachable functions no test exercises. Two strengths of evidence:
//! ingested coverage measures execution (uncovered ⇒ `Certain`), and where coverage
//! is absent or silent about a declaration, the graph heuristic applies — a
//! reference from a file that carries a Test root, name-level, so a same-named
//! reference keeps a function (`Probable`, under-accusing, never over).

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
        // Ingested coverage is test evidence in its own right.
        (!run.graph.files.is_empty() && !any_test_root && run.coverage.is_none())
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
            let file_coverage = cx.run.coverage.as_ref().and_then(|c| c.files.get(&f.path));
            for d in &f.evidence.declarations {
                if !matches!(d.kind, SymbolKind::Function | SymbolKind::Method) {
                    continue;
                }
                // Coverage speaks first; where it is silent about this declaration,
                // the graph heuristic decides.
                let (untested, confidence, message) =
                    match file_coverage.and_then(|fc| fc.function_untested(d.span)) {
                        Some(untested) => (
                            untested,
                            Confidence::Certain,
                            "no test executes this function",
                        ),
                        None => (
                            !tested.contains(d.name.as_str()),
                            Confidence::Probable,
                            "no test references this function",
                        ),
                    };
                if !untested {
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
                    confidence,
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

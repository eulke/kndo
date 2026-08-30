//! Production-reachable code no test exercises. Two strengths of evidence, each at
//! its own granularity: ingested coverage measures execution per function
//! (uncovered ⇒ `Certain`, on the function), and where coverage is absent or silent
//! the graph decides per FILE — production-reachable but reachable from no Test
//! root (`Probable`, on the file). Anything a test imports, however indirectly,
//! counts as exercised: transitive execution without a name is still execution, so
//! the heuristic under-accuses, never over.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext, has_root_of};
use kndo_contract::evidence::{RootKind, SymbolKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};

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
            let mut coverage_spoke = false;
            for d in &f.evidence.declarations {
                if !matches!(d.kind, SymbolKind::Function | SymbolKind::Method) {
                    continue;
                }
                let Some(untested) = file_coverage.and_then(|fc| fc.function_untested(d.span))
                else {
                    continue;
                };
                coverage_spoke = true;
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
                    Confidence::Certain,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    "no test executes this function",
                ));
            }
            // Where coverage said nothing about this file, the graph decides, at the
            // file's own granularity. A file the manifest itself anchors as
            // Production is declared wiring — an entry point or a binary main, which
            // nothing can import — so the heuristic asks the question of what it
            // leads to instead; measured coverage above still judges it.
            if !coverage_spoke
                && !reach.by(RootKind::Test)[i]
                && !f.anchored.iter().any(|r| r.kind == RootKind::Production)
                && f.evidence
                    .declarations
                    .iter()
                    .any(|d| matches!(d.kind, SymbolKind::Function | SymbolKind::Method))
            {
                out.push(Finding::new(
                    Category::UNTESTED,
                    Severity::Info,
                    Confidence::Probable,
                    Subject::File {
                        path: f.path.clone(),
                    },
                    "",
                    "production-reachable, but no test reaches this file",
                ));
            }
        }
        out
    }
}

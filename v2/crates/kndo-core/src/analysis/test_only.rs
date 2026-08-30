//! Production-looking code that only tests keep alive. A file that IS a test (it
//! carries its own Test root) is doing its job; a file that is not, and that no
//! production or tooling color reaches while the test color does, exists only to be
//! tested — the waste signal this category names.

use super::{AbstentionReason, Analysis, AnalysisContext, RunContext, has_root_of};
use kndo_contract::evidence::RootKind;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};

pub struct TestOnly;

impl Analysis for TestOnly {
    fn id(&self) -> &'static str {
        "test-only"
    }

    fn category(&self) -> Category {
        Category::TEST_ONLY
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
        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            let is_a_test = has_root_of(g, i, RootKind::Test);
            let test_only = reach.by(RootKind::Test)[i]
                && !reach.by(RootKind::Production)[i]
                && !reach.by(RootKind::Tooling)[i]
                && !is_a_test;
            if test_only {
                // Test roots come from conventions (`Probable`), and name-level edges
                // carry that uncertainty through.
                out.push(Finding::new(
                    Category::TEST_ONLY,
                    Severity::Info,
                    Confidence::Probable,
                    Subject::File {
                        path: f.path.clone(),
                    },
                    "",
                    "only tests reach this file — no production or tooling root does",
                ));
            }
        }
        out
    }
}

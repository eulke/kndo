//! `crap` — Change Risk Anti-Patterns: per function, `cc² × (1 − cov)³ + cc`,
//! with `cc` the adapter-extracted cyclomatic complexity and `cov` the fraction
//! of the function's instrumented body lines a real test run executed, from
//! ingested coverage. Complex and barely tested is the change-risk signal;
//! complex and fully tested scores its complexity alone, simple and untested
//! scores small.
//!
//! Two measured inputs or nothing: with no coverage ingested the analysis
//! abstains for the whole run (the coverage factor would be a guess for every
//! function at once), and a file the report never instrumented is unmeasured
//! and says so. A function no test executed at all is `untested`'s subject —
//! one verdict per fact — so `crap` names only the partially covered.
//!
//! The threshold is the metric's own: its authors call a function above 30
//! crappy, and the corpus agrees with the line — flask's well-tested tree puts
//! three functions over it, vite's unit-tested node core 129, seventy of them
//! partially covered.

use super::{
    AbstentionReason, AbstentionScope, Analysis, AnalysisContext, RunContext, has_root_of,
};
use kndo_contract::evidence::{EvidenceStream, RootKind};
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};

/// The metric's own line: a function scoring at or above it is a finding.
pub const CRAP_THRESHOLD: f64 = 30.0;

pub fn crap_score(cyclomatic: u32, coverage: f64) -> f64 {
    let cc = f64::from(cyclomatic);
    cc * cc * (1.0 - coverage).powi(3) + cc
}

pub struct Crap {
    pub threshold: f64,
}

impl Analysis for Crap {
    fn id(&self) -> &'static str {
        "crap"
    }

    fn category(&self) -> Category {
        Category::CRAP
    }

    fn requires(&self) -> &'static [EvidenceStream] {
        &[EvidenceStream::Metrics]
    }

    fn abstains(&self, run: &RunContext<'_>) -> Option<AbstentionReason> {
        run.coverage
            .is_none()
            .then_some(AbstentionReason::NoCoverageIngested)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let Some(coverage) = cx.run.coverage.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut silent = 0u32;
        for (i, f) in g.files.iter().enumerate() {
            // A test's own coverage is meaningless to its change risk.
            if !cx.measured[i] || has_root_of(g, i, RootKind::Test) {
                continue;
            }
            let Some(file_coverage) = coverage.files.get(&f.path) else {
                if !f.evidence.metrics.is_empty() {
                    silent += 1;
                }
                continue;
            };
            for (id, metrics) in &f.evidence.metrics {
                let d = &f.evidence.declarations[id.index()];
                let Some(cov) = file_coverage.function_coverage(d.span) else {
                    continue;
                };
                if cov <= 0.0 {
                    continue;
                }
                let score = crap_score(metrics.cyclomatic, cov);
                if score < self.threshold {
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
                    Category::CRAP,
                    Severity::Info,
                    Confidence::Probable,
                    Subject::Symbol {
                        path: f.path.clone(),
                        selector,
                        span: d.span,
                    },
                    "",
                    format!(
                        "complexity {} with {:.0}% of its lines executed by tests scores {:.0} on \
                         the CRAP scale, at or above {:.0}",
                        metrics.cyclomatic,
                        cov * 100.0,
                        score,
                        self.threshold
                    ),
                ));
            }
        }
        if silent > 0 {
            cx.abstain(
                AbstentionReason::NoCoverageRecord,
                AbstentionScope::Files { unmeasured: silent },
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_is_the_metrics_formula() {
        // flask's `load_dotenv`: complexity 13, 31.6% covered.
        let score = crap_score(13, 0.316);
        assert!((score - 67.1).abs() < 0.2, "{score}");
        // Fully covered, the complexity alone.
        assert_eq!(crap_score(5, 1.0), 5.0);
        // Never executed, the cube bites in full.
        assert_eq!(crap_score(5, 0.0), 30.0);
    }
}

//! Analyses weigh evidence and return verdicts; the engine derives abstention before
//! any analysis runs — from the pairing rule (an analysis NAMES the streams it weighs
//! in [`Analysis::requires`]; files whose claiming adapter did not declare them form
//! its unmeasured set) and from each analysis's own whole-run precondition
//! ([`Analysis::abstains`]). Never an `if adapter == …`, never a per-analysis flag.
//!
//! Reachability is computed once, per root color, and shared: every analysis reads
//! the same [`Reachability`] instead of building its own.

mod test_only;
mod untested;
mod unused;

pub use test_only::TestOnly;
pub use untested::Untested;
pub use unused::Unused;

use crate::graph::Graph;
use kndo_contract::evidence::{EvidenceStream, RootKind};
use kndo_contract::finding::{Finding, sort_findings};
use kndo_contract::vocab::Category;
use serde::Serialize;
use std::fmt;

/// Which root colors reach each file: seeded by the file's own roots of that kind
/// (extraction evidence and manifest anchors alike), propagated over resolved import
/// edges. Computed once per run; every analysis reads the same answer.
pub struct Reachability {
    production: Vec<bool>,
    test: Vec<bool>,
    tooling: Vec<bool>,
}

impl Reachability {
    pub fn compute(graph: &Graph) -> Self {
        Reachability {
            production: flood(graph, RootKind::Production),
            test: flood(graph, RootKind::Test),
            tooling: flood(graph, RootKind::Tooling),
        }
    }

    pub fn by(&self, kind: RootKind) -> &[bool] {
        match kind {
            RootKind::Production => &self.production,
            RootKind::Test => &self.test,
            RootKind::Tooling => &self.tooling,
        }
    }

    /// Reached by any color at all.
    pub fn any(&self, file: usize) -> bool {
        self.production[file] || self.test[file] || self.tooling[file]
    }
}

/// Does this file itself carry a root of `kind` (its own evidence or an anchor)?
pub fn has_root_of(graph: &Graph, file: usize, kind: RootKind) -> bool {
    let f = &graph.files[file];
    f.evidence
        .roots
        .iter()
        .chain(&f.anchored)
        .any(|r| r.kind == kind)
}

fn flood(graph: &Graph, kind: RootKind) -> Vec<bool> {
    let n = graph.files.len();
    let mut reached = vec![false; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| has_root_of(graph, i, kind)).collect();
    for &i in &queue {
        reached[i] = true;
    }
    while let Some(i) = queue.pop() {
        for &t in &graph.files[i].imports {
            let t = t as usize;
            if !reached[t] {
                reached[t] = true;
                queue.push(t);
            }
        }
    }
    reached
}

/// Everything one run shares across analyses. Grows fields as evidence sources land
/// (coverage joins here); each analysis reads what it needs.
pub struct RunContext<'a> {
    pub graph: &'a Graph,
    pub reach: Reachability,
}

pub struct AnalysisContext<'a> {
    pub run: &'a RunContext<'a>,
    /// Per-file, for THIS analysis: did the claiming adapter declare every stream it
    /// requires? Unmeasured files must produce no findings.
    pub measured: &'a [bool],
}

impl<'a> AnalysisContext<'a> {
    pub fn graph(&self) -> &'a Graph {
        self.run.graph
    }
}

pub trait Analysis: Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn requires(&self) -> &'static [EvidenceStream] {
        &[]
    }
    /// A precondition on the run as a whole. `Some` means this run cannot be judged
    /// at all — the engine records the abstention and never calls [`Analysis::run`].
    /// Degrade-toward-keep-alive at analysis scale: silence over accusation.
    fn abstains(&self, _run: &RunContext<'_>) -> Option<AbstentionReason> {
        None
    }
    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbstentionScope {
    WholeRun,
    Files { unmeasured: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbstentionReason {
    StreamsNotDeclared {
        missing: Vec<EvidenceStream>,
    },
    /// No root anchors anything in the whole graph. Reachability judged from zero
    /// roots would accuse every file at once; that is a missing-evidence condition,
    /// not a verdict.
    NoRootsAnywhere,
    /// No test root anchors anything: with zero test evidence, "tests never reach
    /// this" describes every declaration equally and accuses none.
    NoTestRootsAnywhere,
}

impl fmt::Display for AbstentionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbstentionReason::StreamsNotDeclared { missing } => {
                write!(f, "required evidence streams not declared: {missing:?}")
            }
            AbstentionReason::NoRootsAnywhere => {
                write!(f, "no root anchors any file in this graph")
            }
            AbstentionReason::NoTestRootsAnywhere => {
                write!(f, "no test root anchors any file in this graph")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Abstention {
    pub category: Category,
    pub reason: AbstentionReason,
    pub scope: AbstentionScope,
}

pub fn run_all(graph: &Graph, analyses: &[&dyn Analysis]) -> (Vec<Finding>, Vec<Abstention>) {
    let run = RunContext {
        graph,
        reach: Reachability::compute(graph),
    };
    let mut findings = Vec::new();
    let mut abstained = Vec::new();

    for analysis in analyses {
        if let Some(reason) = analysis.abstains(&run) {
            abstained.push(Abstention {
                category: analysis.category(),
                reason,
                scope: AbstentionScope::WholeRun,
            });
            continue;
        }
        let requires = analysis.requires();
        let measured: Vec<bool> = graph
            .files
            .iter()
            .map(|f| requires.iter().all(|s| f.evidence.declared.contains(*s)))
            .collect();
        let unmeasured = measured.iter().filter(|m| !**m).count() as u32;
        if unmeasured > 0 {
            let missing: Vec<EvidenceStream> = requires.to_vec();
            let scope = if unmeasured as usize == graph.files.len() {
                AbstentionScope::WholeRun
            } else {
                AbstentionScope::Files { unmeasured }
            };
            abstained.push(Abstention {
                category: analysis.category(),
                reason: AbstentionReason::StreamsNotDeclared { missing },
                scope,
            });
            if unmeasured as usize == graph.files.len() {
                // Nothing is measured — the analysis has nothing to run on.
                continue;
            }
        }
        let cx = AnalysisContext {
            run: &run,
            measured: &measured,
        };
        findings.extend(analysis.run(&cx));
    }

    sort_findings(&mut findings);
    (findings, abstained)
}

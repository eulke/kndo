//! Analyses weigh evidence and return verdicts; the engine derives abstention from
//! the pairing rule before any analysis runs. An analysis NAMES the streams it weighs
//! in [`Analysis::requires`]; files whose claiming adapter did not declare them form
//! that analysis's unmeasured set — excluded from findings, surfaced as an
//! [`Abstention`] with scope. Never an `if adapter == …`, never a per-analysis flag.

use crate::graph::Graph;
use kndo_contract::evidence::{EvidenceStream, ImportShape, RootTarget};
use kndo_contract::finding::{Finding, Severity, sort_findings};
use kndo_contract::subject::{Subject, SymbolSelector};
use kndo_contract::vocab::{Category, Confidence};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt;

pub struct AnalysisContext<'a> {
    pub graph: &'a Graph,
    /// Per-file, for THIS analysis: did the claiming adapter declare every stream it
    /// requires? Unmeasured files must produce no findings.
    pub measured: &'a [bool],
}

pub trait Analysis: Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn requires(&self) -> &'static [EvidenceStream] {
        &[]
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
    StreamsNotDeclared { missing: Vec<EvidenceStream> },
}

impl fmt::Display for AbstentionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbstentionReason::StreamsNotDeclared { missing } => {
                write!(f, "required evidence streams not declared: {missing:?}")
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
    let mut findings = Vec::new();
    let mut abstained = Vec::new();

    for analysis in analyses {
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
        }
        let cx = AnalysisContext {
            graph,
            measured: &measured,
        };
        findings.extend(analysis.run(&cx));
    }

    sort_findings(&mut findings);
    (findings, abstained)
}

/// Unreachable code, at M1 scale: a claimed file no root anchors and no reachable
/// file imports is unused; within reachable files, a declaration nothing references,
/// roots, or imports is unused. Dead is always `Certain` — color outranks confidence.
pub struct Unused;

impl Analysis for Unused {
    fn id(&self) -> &'static str {
        "unused"
    }

    fn category(&self) -> Category {
        Category::UNUSED
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph;
        let n = g.files.len();

        // Reachability: root-anchored files, then everything they transitively import.
        let mut reachable = vec![false; n];
        let mut queue: Vec<usize> = (0..n)
            .filter(|&i| !g.files[i].evidence.roots.is_empty())
            .collect();
        for &i in &queue {
            reachable[i] = true;
        }
        while let Some(i) = queue.pop() {
            for &t in &g.files[i].imports {
                let t = t as usize;
                if !reachable[t] {
                    reachable[t] = true;
                    queue.push(t);
                }
            }
        }

        // Names each file's imports bind from each target, and whether an importer
        // keeps a target's whole exported surface alive (namespace/side-effect: the
        // engine cannot see through them, so it degrades toward keep-alive).
        let mut bound: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut surface_kept = vec![false; n];
        for (i, f) in g.files.iter().enumerate() {
            if !reachable[i] {
                continue;
            }
            for import in &f.evidence.imports {
                for &t in &f.imports {
                    // M1: bindings apply to every resolved target of the file; real
                    // per-import targeting arrives with the first real language.
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
        }

        let mut out = Vec::new();
        for (i, f) in g.files.iter().enumerate() {
            if !cx.measured[i] {
                continue;
            }
            if !reachable[i] {
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
            // A whole-file root anchors the FILE's reachability; its declarations are
            // still judged individually — a private, uncalled function in an entry
            // point is dead code.
            for (d_ix, d) in f.evidence.declarations.iter().enumerate() {
                let kept = referenced.contains(d.name.as_str())
                    || rooted.contains(&d_ix)
                    || (d.reach == kndo_contract::evidence::Reach::Exported
                        && (surface_kept[i] || bound.contains(&(i as u32, d.name.as_str()))));
                if !kept {
                    out.push(Finding::new(
                        Category::UNUSED,
                        Severity::Warning,
                        Confidence::Certain,
                        Subject::Symbol {
                            path: f.path.clone(),
                            selector: SymbolSelector::Free(d.name.clone()),
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

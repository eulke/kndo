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
    /// A precondition on the graph as a whole. `Some` means this run cannot be judged
    /// at all — the engine records the abstention and never calls [`Analysis::run`].
    /// Degrade-toward-keep-alive at analysis scale: silence over accusation.
    fn abstains(&self, _graph: &Graph) -> Option<AbstentionReason> {
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
        if let Some(reason) = analysis.abstains(graph) {
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

    fn abstains(&self, graph: &Graph) -> Option<AbstentionReason> {
        let any_root = graph.files.iter().any(|f| f.is_rooted());
        (!graph.files.is_empty() && !any_root).then_some(AbstentionReason::NoRootsAnywhere)
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph;
        let n = g.files.len();

        // Reachability: root-anchored files, then everything they transitively import.
        let mut reachable = vec![false; n];
        let mut queue: Vec<usize> = (0..n).filter(|&i| g.files[i].is_rooted()).collect();
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
        // Member references dispatch through values, not lexical scope, so their
        // evidence pool is every reachable file's references at once.
        let mut bound: BTreeSet<(u32, &str)> = BTreeSet::new();
        let mut surface_kept = vec![false; n];
        let mut member_referenced: BTreeSet<&str> = BTreeSet::new();
        for (i, f) in g.files.iter().enumerate() {
            if !reachable[i] {
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
                let exported = d.reach == kndo_contract::evidence::Reach::Exported;
                let kept = if let Some(owner) = d.owner {
                    // A member: kept by any reference to its name anywhere reachable
                    // (dispatch is not lexical), by a root, or by its owner's whole
                    // surface being kept from outside.
                    let owner_exported = f.evidence.declarations[owner.index()].reach
                        == kndo_contract::evidence::Reach::Exported;
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

//! Import cycles: strongly connected components of size ≥ 2 over resolved
//! LOAD-TIME import edges — never over `sees`, whose regions are mutual by
//! construction, and never over a lazy or erased import: an initialization
//! hazard needs initialization, and a dynamic `import()`, a function-scoped
//! `require`, an `import type` or a `TYPE_CHECKING` block runs after linking or
//! never ([`kndo_contract::evidence::Timing`]).
//! ONE finding per cycle, anchored at the lexicographically-first participant,
//! with the shortest loop through the anchor spelled in the message as the
//! evidence chain.
//!
//! Whether a cycle is worth a finding is a LANGUAGE fact, declared per adapter
//! as [`kndo_contract::extension::CycleTolerance`] and read here from
//! [`super::RunContext::cycle_hazards`]: a cycle fires iff at least one
//! participant's language calls cycles a hazard — the hazard is real for that
//! language — and a cycle whose every participant tolerates them is true
//! information about legal, routine structure, which is never dressed up as a
//! defect.
//!
//! Confidence is the weakest import on the REPORTED loop — a cycle is only as
//! real as its weakest link, the same honesty rule `trace` applies to paths.

use super::{Analysis, AnalysisContext};
use kndo_contract::evidence::Timing;
use kndo_contract::finding::{Finding, Severity};
use kndo_contract::subject::Subject;
use kndo_contract::vocab::{Category, Confidence};

pub struct Cyclic;

impl Analysis for Cyclic {
    fn id(&self) -> &'static str {
        "cyclic"
    }

    fn category(&self) -> Category {
        Category::CYCLIC
    }

    fn run(&self, cx: &AnalysisContext<'_>) -> Vec<Finding> {
        let g = cx.graph();
        let hazard: Vec<bool> = g
            .files
            .iter()
            .map(|f| {
                cx.run.capabilities_of(&f.adapter).is_some_and(|caps| {
                    caps.import_cycles == kndo_contract::extension::CycleTolerance::Hazard
                })
            })
            .collect();
        // Self-edges out: a file importing itself (Python's `from . import x`
        // inside `__init__.py` resolves to the package's own file) is not a
        // cycle BETWEEN modules, and it must not shadow the real loop. Only
        // load-time imports draw an edge here; an unknown timing reads as lazy.
        let adjacency: Vec<Vec<u32>> = g
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let mut out: Vec<u32> = f
                    .evidence
                    .imports
                    .iter()
                    .zip(&f.import_targets)
                    .filter(|(import, _)| matches!(import.timing, Timing::Load))
                    .flat_map(|(_, targets)| targets.iter().copied())
                    .filter(|&t| t != i as u32)
                    .collect();
                out.sort_unstable();
                out.dedup();
                out
            })
            .collect();
        let adjacency: Vec<&[u32]> = adjacency.iter().map(|v| v.as_slice()).collect();
        let mut out = Vec::new();
        for mut scc in sccs(&adjacency) {
            if scc.len() < 2 {
                continue;
            }
            if !scc.iter().any(|&i| hazard[i as usize]) {
                continue;
            }
            scc.sort_unstable_by(|&a, &b| g.files[a as usize].path.cmp(&g.files[b as usize].path));
            let anchor = scc[0] as usize;
            let loop_path = shortest_loop(&adjacency, &scc, scc[0]);
            let mut hops: Vec<(u32, u32)> = loop_path.windows(2).map(|p| (p[0], p[1])).collect();
            if let (Some(&first), Some(&last)) = (loop_path.first(), loop_path.last()) {
                hops.push((last, first)); // the edge that closes the loop
            }
            let confidence = hops
                .iter()
                .filter_map(|&(from, to)| import_confidence(g, from, to))
                .min()
                .unwrap_or(Confidence::Certain);
            out.push(Finding::new(
                Category::CYCLIC,
                Severity::Warning,
                confidence,
                Subject::File {
                    path: g.files[anchor].path.clone(),
                },
                "",
                format!(
                    "{} files form an import cycle: {}",
                    scc.len(),
                    render_loop(g, &loop_path)
                ),
            ));
        }
        out.extend(package_cycles(g));
        out.sort_by(|a, b| a.subject.path().cmp(b.subject.path()));
        out
    }
}

/// Workspace-package cycles over DECLARED sibling dependencies: P → Q when
/// P's anchoring manifest declares a dependency spelling Q's name at any
/// scope but `Dev` — a dev-only mutual pair never blocks a publish (npm
/// ignores devDependencies at install), while an unknown scope (the JVM
/// name-only stream) counts, which is exactly where the demand lives.
/// Ecosystem-generic by construction: no language tolerance applies, because
/// a declared mutual dependency between publishable units breaks publish
/// ordering wherever packages publish at all.
fn package_cycles(g: &crate::graph::Graph) -> Vec<Finding> {
    use kndo_contract::adapter::DependencyScope;
    if g.packages.len() < 2 {
        return Vec::new();
    }
    let declared_names: Vec<std::collections::BTreeSet<&str>> = g
        .packages
        .iter()
        .map(|p| {
            g.manifest_declarations
                .iter()
                .find(|d| d.manifest == p.manifest)
                .map(|d| {
                    d.declarations
                        .iter()
                        .filter(|dd| dd.scope != Some(DependencyScope::Dev))
                        .map(|dd| dd.name.as_str())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    let adjacency: Vec<Vec<u32>> = (0..g.packages.len())
        .map(|p| {
            (0..g.packages.len())
                .filter(|&q| q != p && declared_names[p].contains(g.packages[q].name.as_str()))
                .map(|q| q as u32)
                .collect()
        })
        .collect();
    let slices: Vec<&[u32]> = adjacency.iter().map(|v| v.as_slice()).collect();
    let mut out = Vec::new();
    for mut scc in sccs(&slices) {
        if scc.len() < 2 {
            continue;
        }
        scc.sort_unstable_by(|&a, &b| {
            g.packages[a as usize]
                .name
                .cmp(&g.packages[b as usize].name)
        });
        let anchor = &g.packages[scc[0] as usize];
        let loop_path = shortest_loop(&slices, &scc, scc[0]);
        let mut names: Vec<&str> = loop_path
            .iter()
            .map(|&i| g.packages[i as usize].name.as_str())
            .collect();
        names.push(anchor.name.as_str());
        out.push(Finding::new(
            Category::CYCLIC,
            Severity::Warning,
            Confidence::Certain,
            Subject::Package {
                manifest: anchor.manifest.clone(),
                name: anchor.name.clone(),
            },
            "",
            format!(
                "{} workspace packages form a dependency cycle: {} — a cycle between                  publishable units breaks publish ordering",
                scc.len(),
                names.join(" → ")
            ),
        ));
    }
    out
}

/// The strongest-confidence import edge from `from` to `to`, if one exists —
/// the same lookup the query verbs render on trace hops.
fn import_confidence(g: &crate::graph::Graph, from: u32, to: u32) -> Option<Confidence> {
    let f = &g.files[from as usize];
    f.evidence
        .imports
        .iter()
        .zip(&f.import_targets)
        .filter(|(import, targets)| matches!(import.timing, Timing::Load) && targets.contains(&to))
        .map(|(import, _)| import.confidence)
        .max()
}

/// `a → b → … → a`, capped: long loops show the first five hops and say how
/// many more close them.
fn render_loop(g: &crate::graph::Graph, loop_path: &[u32]) -> String {
    const SHOWN: usize = 6; // the anchor plus five hops
    let names: Vec<&str> = loop_path
        .iter()
        .take(SHOWN)
        .map(|&i| g.files[i as usize].path.as_str())
        .collect();
    let shown = names.join(" → ");
    let elided = loop_path.len().saturating_sub(SHOWN);
    if elided > 0 {
        format!(
            "{shown} → … {elided} more → {}",
            g.files[loop_path[0] as usize].path.as_str()
        )
    } else {
        format!("{shown} → {}", g.files[loop_path[0] as usize].path.as_str())
    }
}

/// BFS shortest cycle through `start`, restricted to the SCC (one always
/// exists there); deterministic because adjacency is sorted.
fn shortest_loop(adjacency: &[&[u32]], scc: &[u32], start: u32) -> Vec<u32> {
    let members: std::collections::BTreeSet<u32> = scc.iter().copied().collect();
    let mut prev: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut frontier = vec![start];
    loop {
        let mut next = Vec::new();
        for &at in &frontier {
            for &t in adjacency[at as usize] {
                if t == start {
                    // Every prev chain roots at `start` (the BFS origin), so
                    // the walk itself ends by pushing it — the reversed path
                    // begins at the anchor and the closing edge back to it is
                    // implicit.
                    let mut path = vec![at];
                    let mut cursor = at;
                    while let Some(&p) = prev.get(&cursor) {
                        path.push(p);
                        cursor = p;
                    }
                    path.reverse();
                    return path;
                }
                if members.contains(&t) && t != start && !prev.contains_key(&t) {
                    prev.insert(t, at);
                    next.push(t);
                }
            }
        }
        debug_assert!(!next.is_empty(), "an SCC always closes its loop");
        frontier = next;
    }
}

/// Iterative Tarjan over `0..n` with the given adjacency; returns every SCC.
fn sccs(adjacency: &[&[u32]]) -> Vec<Vec<u32>> {
    let n = adjacency.len();
    let mut index = vec![u32::MAX; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<u32> = Vec::new();
    let mut next_index = 0u32;
    let mut out = Vec::new();
    // Explicit call stack: (node, next child position).
    let mut call: Vec<(u32, usize)> = Vec::new();
    for root in 0..n as u32 {
        if index[root as usize] != u32::MAX {
            continue;
        }
        call.push((root, 0));
        while let Some(&mut (v, ref mut child)) = call.last_mut() {
            let vi = v as usize;
            if *child == 0 {
                index[vi] = next_index;
                low[vi] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[vi] = true;
            }
            if let Some(&w) = adjacency[vi].get(*child) {
                *child += 1;
                let wi = w as usize;
                if index[wi] == u32::MAX {
                    call.push((w, 0));
                } else if on_stack[wi] {
                    low[vi] = low[vi].min(index[wi]);
                }
            } else {
                call.pop();
                if let Some(&(parent, _)) = call.last() {
                    let pi = parent as usize;
                    low[pi] = low[pi].min(low[vi]);
                }
                if low[vi] == index[vi] {
                    let mut scc = Vec::new();
                    loop {
                        let w = stack.pop().expect("tarjan stack underflow");
                        on_stack[w as usize] = false;
                        scc.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(scc);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sccs;

    #[test]
    fn tarjan_finds_the_components() {
        // 0 → 1 → 2 → 0 is one SCC; 3 → 4 is two singletons.
        let adj: Vec<Vec<u32>> = vec![vec![1], vec![2], vec![0], vec![4], vec![]];
        let refs: Vec<&[u32]> = adj.iter().map(|v| v.as_slice()).collect();
        let mut components: Vec<Vec<u32>> = sccs(&refs)
            .into_iter()
            .map(|mut c| {
                c.sort_unstable();
                c
            })
            .collect();
        components.sort();
        assert!(components.contains(&vec![0, 1, 2]));
        assert_eq!(components.iter().filter(|c| c.len() == 1).count(), 2);
    }
}

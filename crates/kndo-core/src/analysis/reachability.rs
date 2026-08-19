//! Reachability with confidence (RFC 0005 §1) — the tiered algorithm every "is this alive"
//! analysis is built on. This is a **literal, direct** implementation of the formalized
//! algorithm (three explicit confidence-tier passes per root kind, not a cleverer
//! single-pass widest-path algorithm): correctness and auditability win over performance
//! for logic this load-bearing, and three tiers make the literal version cheap anyway.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, EdgeKind, FileId, NodeRef, RootKind, SymbolId};

/// The four colors, named exactly as RFC 0005 §1's table names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reachability {
    Production,
    TestOnly,
    ToolingOnly,
    Unreachable,
}

/// Every graph node's resolved `(color, confidence)` — total over every file and symbol.
/// Absence from every tier resolves to `(Unreachable, Certain)`, per RFC 0005 §1's
/// consequence: dead is always certain.
pub struct ReachabilityMap {
    colors: HashMap<NodeRef, (Reachability, Confidence)>,
}

impl ReachabilityMap {
    pub fn get(&self, node: NodeRef) -> (Reachability, Confidence) {
        self.colors
            .get(&node)
            .copied()
            .unwrap_or((Reachability::Unreachable, Confidence::Certain))
    }
}

/// Strongest to weakest — the order rule 1–3 of RFC 0005 §1 checks a node against.
const TIERS: [Confidence; 3] = [
    Confidence::Certain,
    Confidence::Probable,
    Confidence::Possible,
];
const ROOT_KINDS: [(RootKind, Reachability); 3] = [
    (RootKind::Production, Reachability::Production),
    (RootKind::Test, Reachability::TestOnly),
    (RootKind::Tooling, Reachability::ToolingOnly),
];

pub fn compute(graph: &ProjectGraph) -> ReachabilityMap {
    // Adjacency for reachability-relevant edges only. Declares (ownership) and
    // ImportsDependency (a fact about dependency usage, not code reachability) never
    // participate — only Root (seeds, handled separately), References, ImportsFile, and
    // Wildcard's expansion contribute traversable edges.
    let mut declared_in: HashMap<FileId, Vec<SymbolId>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::Declares { file, symbol } = edge.kind {
            declared_in.entry(file).or_default().push(symbol);
        }
    }

    let mut adjacency: HashMap<NodeRef, Vec<(NodeRef, Confidence)>> = HashMap::new();
    for edge in &graph.edges {
        match edge.kind {
            EdgeKind::References { from, to, .. } => adjacency
                .entry(NodeRef::Symbol(from))
                .or_default()
                .push((NodeRef::Symbol(to), edge.confidence)),
            EdgeKind::ImportsFile { from, to } => adjacency
                .entry(NodeRef::File(from))
                .or_default()
                .push((NodeRef::File(to), edge.confidence)),
            EdgeKind::Wildcard { from } => {
                // Plausible target set, absent narrower DynamicUse metadata (RFC 0005 §1):
                // every symbol declared in the same file, at `possible`.
                if let Some(syms) = declared_in.get(&from) {
                    let entry = adjacency.entry(NodeRef::File(from)).or_default();
                    for &s in syms {
                        entry.push((NodeRef::Symbol(s), Confidence::Possible));
                    }
                }
            }
            EdgeKind::Declares { .. }
            | EdgeKind::ImportsDependency { .. }
            | EdgeKind::Root { .. } => {}
        }
    }

    let mut seeds: HashMap<RootKind, Vec<(NodeRef, Confidence)>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::Root { kind, target } = edge.kind {
            seeds
                .entry(kind)
                .or_default()
                .push((target, edge.confidence));
        }
    }

    // R(kind, tau) for every (kind, tau), literally: BFS seeded only by roots whose own
    // confidence is >= tau, traversing only edges with confidence >= tau.
    let mut reached: HashMap<(RootKind, Confidence), HashSet<NodeRef>> = HashMap::new();
    for &(kind, _) in &ROOT_KINDS {
        let kind_seeds = seeds.get(&kind).cloned().unwrap_or_default();
        for &tau in &TIERS {
            let mut visited: HashSet<NodeRef> = HashSet::new();
            let mut queue: VecDeque<NodeRef> = VecDeque::new();
            for &(node, conf) in &kind_seeds {
                if conf >= tau && visited.insert(node) {
                    queue.push_back(node);
                }
            }
            while let Some(node) = queue.pop_front() {
                for &(next, conf) in adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
                    if conf >= tau && visited.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
            reached.insert((kind, tau), visited);
        }
    }

    // First-match precedence over every node: Production > TestOnly > ToolingOnly; within a
    // color, the strongest tau achieved. Absent nodes are left out (ReachabilityMap::get
    // supplies the (Unreachable, Certain) default).
    let mut colors: HashMap<NodeRef, (Reachability, Confidence)> = HashMap::new();
    let all_nodes = (0..graph.files.len())
        .map(|i| NodeRef::File(FileId(i as u32)))
        .chain((0..graph.symbols.len()).map(|i| NodeRef::Symbol(SymbolId(i as u32))));
    for node in all_nodes {
        let resolved = ROOT_KINDS.iter().find_map(|&(kind, color)| {
            TIERS
                .iter()
                .find(|&&tau| reached.get(&(kind, tau)).is_some_and(|s| s.contains(&node)))
                .map(|&tau| (color, tau))
        });
        if let Some(result) = resolved {
            colors.insert(node, result);
        }
    }

    ReachabilityMap { colors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, VisibilityLevel};
    use crate::graph::{DependencyNode, FileNode, ProjectGraph, SymbolNode};
    use crate::vocab::{Edge, FileClass, FileOrigin, FileRole, Provenance, RefKind, SymbolKind};
    use smol_str::SmolStr;

    fn file(path: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Authored,
            }),
        }
    }

    fn symbol(file: FileId, name: &str) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Default::default(),
            exported: true,
            visibility: VisibilityLevel(1),
        }
    }

    fn edge(kind: EdgeKind, confidence: Confidence) -> Edge {
        Edge {
            kind,
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
        }
    }

    #[test]
    fn root_file_is_production_certain() {
        let files = vec![file("a.ts")];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(0))),
            (Reachability::Production, Confidence::Certain)
        );
    }

    #[test]
    fn transitively_imported_file_is_reachable() {
        let files = vec![file("a.ts"), file("b.ts")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(1))).0,
            Reachability::Production
        );
    }

    #[test]
    fn disconnected_file_is_unreachable_at_certain_confidence() {
        // "Dead is always certain" — RFC 0005 §1 consequence.
        let files = vec![file("orphan.ts")];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(0))),
            (Reachability::Unreachable, Confidence::Certain)
        );
    }

    #[test]
    fn color_precedence_beats_confidence_tier() {
        // The RFC 0005 §1 worked example: S is production-reachable only via a probable
        // (duck-typed) edge, AND test-reachable via a certain edge. Production must still
        // win, at probable confidence — "maybe still used for real" beats "definitely only
        // used by tests."
        let prod_root = file("prod_root.ts");
        let test_root = file("test_root.ts");
        let target = file("target.ts");
        let files = vec![prod_root, test_root, target];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(0),
                    to: FileId(2),
                },
                Confidence::Probable,
            ),
            edge(
                EdgeKind::ImportsFile {
                    from: FileId(1),
                    to: FileId(2),
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(2))),
            (Reachability::Production, Confidence::Probable)
        );
    }

    #[test]
    fn wildcard_expands_to_possible_edges_over_same_file_symbols() {
        let f = file("dynamic.ts");
        let files = vec![f];
        let sym = symbol(FileId(0), "handler");
        let symbols = vec![sym];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Declares {
                    file: FileId(0),
                    symbol: SymbolId(0),
                },
                Confidence::Certain,
            ),
            edge(EdgeKind::Wildcard { from: FileId(0) }, Confidence::Possible),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = compute(&graph);
        // Kept alive, not reported dead — at possible confidence, never certain.
        assert_eq!(
            reach.get(NodeRef::Symbol(SymbolId(0))),
            (Reachability::Production, Confidence::Possible)
        );
    }

    #[test]
    fn references_edges_carry_symbol_level_reachability() {
        let files = vec![file("a.ts")];
        let symbols = vec![symbol(FileId(0), "main"), symbol(FileId(0), "helper")];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: SymbolId(0),
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::Symbol(SymbolId(1))).0,
            Reachability::Production
        );
    }

    #[test]
    fn unreferenced_symbol_stays_unreachable() {
        let files = vec![file("a.ts")];
        let symbols = vec![symbol(FileId(0), "main"), symbol(FileId(0), "dead")];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(SymbolId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::Symbol(SymbolId(1))).0,
            Reachability::Unreachable
        );
    }

    #[test]
    fn dependency_nodes_are_unaffected_bystanders() {
        // ImportsDependency never participates in reachability — sanity check that adding
        // one doesn't crash or spuriously mark anything reachable.
        let files = vec![file("a.ts")];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![edge(
            EdgeKind::ImportsDependency {
                from: FileId(0),
                to: crate::vocab::DependencyId(0),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(0))).0,
            Reachability::Unreachable
        );
    }
}

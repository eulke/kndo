//! Reachability with confidence (RFC 0005 §1) — the tiered algorithm every "is this alive"
//! analysis is built on. This is a **literal, direct** implementation of the formalized
//! algorithm (three explicit confidence-tier passes per root kind, not a cleverer
//! single-pass widest-path algorithm): correctness and auditability win over performance
//! for logic this load-bearing.
//!
//! The *data layout* is where performance lives (RFC 0008 §3, applied to the hottest
//! algorithm): nodes get dense indices (files first, then symbols), the traversable edges are
//! built once into CSR-style columnar adjacency (offsets + targets + confidences — BFS walks
//! contiguous `u32` columns, not hash buckets), and each of the nine per-`(kind, tier)`
//! reached sets is a bitset. The module-load rule (reaching a symbol reaches its owning file,
//! RFC 0012 §4) becomes an ordinary implicit CSR edge `symbol → owner` at `Certain` — the
//! same semantics the special-cased visit had, since a certain edge passes every tier's
//! filter exactly like the unconditional visit did. Its counterpart, the execution rule,
//! still needs no code: symbol-attributed references hang off the symbol node and traverse
//! only once it's reached. (The symbol→owner edge was originally caught dogfooding the Go
//! adapter: `func main()` was a root but `main.go` was never enqueued.)

use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, EdgeKind, NodeRef, RootKind};

/// The four colors, named exactly as RFC 0005 §1's table names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reachability {
    Production,
    TestOnly,
    ToolingOnly,
    Unreachable,
}

/// Minimal fixed-size bitset — dense node indices make membership a shift and a mask.
#[derive(Clone)]
struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    fn new(len: usize) -> BitSet {
        BitSet {
            words: vec![0; len.div_ceil(64)],
        }
    }

    #[inline]
    fn insert(&mut self, i: usize) -> bool {
        let (w, b) = (i / 64, 1u64 << (i % 64));
        let fresh = self.words[w] & b == 0;
        self.words[w] |= b;
        fresh
    }

    #[inline]
    fn contains(&self, i: usize) -> bool {
        self.words[i / 64] & (1u64 << (i % 64)) != 0
    }
}

/// Every graph node's resolved `(color, confidence)` — total over every file and symbol.
/// Absence from every tier resolves to `(Unreachable, Certain)`, per RFC 0005 §1's
/// consequence: dead is always certain.
pub struct ReachabilityMap {
    files_len: usize,
    /// Dense per-node winning `(color, strongest tau)`.
    colors: Vec<(Reachability, Confidence)>,
    /// Per root kind, the `Possible`-tier reached set — the loosest tier's BFS, whose set is
    /// the union of every stronger tier's. Kept because `colors` only records each node's
    /// *winning* color (Production beats TestOnly beats ToolingOnly), which is exactly wrong
    /// for a query like `untested` (RFC 0005 §9): "is this Production-colored node *also*
    /// reachable from a test root" needs the kind that lost the precedence race.
    reached_possible: [BitSet; 3],
}

impl ReachabilityMap {
    #[inline]
    fn index(&self, node: NodeRef) -> usize {
        match node {
            NodeRef::File(f) => f.0 as usize,
            NodeRef::Symbol(s) => self.files_len + s.0 as usize,
        }
    }

    pub fn get(&self, node: NodeRef) -> (Reachability, Confidence) {
        self.colors[self.index(node)]
    }

    /// Whether `node` is reachable from any root of `kind`, at any confidence whatsoever —
    /// independent of which color `node` actually won (see the struct doc).
    pub fn reachable_from(&self, kind: RootKind, node: NodeRef) -> bool {
        self.reached_possible[kind_index(kind)].contains(self.index(node))
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

fn kind_index(kind: RootKind) -> usize {
    match kind {
        RootKind::Production => 0,
        RootKind::Test => 1,
        RootKind::Tooling => 2,
    }
}

pub fn compute(graph: &ProjectGraph) -> ReachabilityMap {
    let files_len = graph.files.len();
    let n = files_len + graph.symbols.len();
    let node_index = |node: NodeRef| -> usize {
        match node {
            NodeRef::File(f) => f.0 as usize,
            NodeRef::Symbol(s) => files_len + s.0 as usize,
        }
    };

    // ---- CSR construction: count, offset, fill. Traversable edges only — Declares
    // (ownership) and ImportsDependency (a dependency-usage fact) never participate; Root
    // edges are seeds, handled separately.
    let mut declared_in: Vec<Vec<u32>> = vec![Vec::new(); files_len];
    for edge in &graph.edges {
        if let EdgeKind::Declares { file, symbol } = edge.kind {
            declared_in[file.0 as usize].push(symbol.0);
        }
    }

    // The invoked-program rule's target sets (RFC 0005 §1): per file, its Production `Root`
    // symbols. *Executing* a file as a program runs its entry point — unlike importing it,
    // which runs only load-time code — so an `InvokesFile` edge fans out to these implicit
    // targets besides the file itself. Test/Tooling roots inside the invoked file stay out:
    // running the binary does not run its inline tests.
    let mut prod_roots_by_file: Vec<Vec<(u32, Confidence)>> = vec![Vec::new(); files_len];
    for edge in &graph.edges {
        if let EdgeKind::Root {
            kind: RootKind::Production,
            target: NodeRef::Symbol(s),
        } = edge.kind
        {
            let owner = graph.symbols[s.0 as usize].file.0 as usize;
            prod_roots_by_file[owner].push(((files_len + s.0 as usize) as u32, edge.confidence));
        }
    }

    let mut degree: Vec<u32> = vec![0; n];
    let count = |degree: &mut Vec<u32>, from: usize, extra: usize| degree[from] += extra as u32;
    for edge in &graph.edges {
        match edge.kind {
            EdgeKind::References { from, .. } => count(&mut degree, node_index(from), 1),
            EdgeKind::ImportsFile { from, .. } => count(&mut degree, from.0 as usize, 1),
            // RFC 0017 §5.4's plugin file-liveness edge: traversed exactly like ImportsFile
            // (from alive ⇒ to in use), just from a NodeRef and only ever plugin-contributed.
            EdgeKind::ReferencesFile { from, .. } => count(&mut degree, node_index(from), 1),
            // Invoked-program rule: the file plus its Production root symbols.
            EdgeKind::InvokesFile { from, to } => count(
                &mut degree,
                node_index(from),
                1 + prod_roots_by_file[to.0 as usize].len(),
            ),
            EdgeKind::Wildcard { from } => count(
                &mut degree,
                from.0 as usize,
                declared_in[from.0 as usize].len(),
            ),
            _ => {}
        }
    }
    // The module-load rule's implicit symbol → owner edge, one per symbol.
    for i in 0..graph.symbols.len() {
        degree[files_len + i] += 1;
    }

    let mut offsets: Vec<u32> = Vec::with_capacity(n + 1);
    offsets.push(0);
    for &d in &degree {
        offsets.push(offsets.last().unwrap() + d);
    }
    let total = *offsets.last().unwrap() as usize;
    let mut targets: Vec<u32> = vec![0; total];
    let mut confs: Vec<Confidence> = vec![Confidence::Certain; total];
    let mut cursor: Vec<u32> = offsets[..n].to_vec();
    let mut push_edge = |cursor: &mut Vec<u32>, from: usize, to: usize, conf: Confidence| {
        let at = cursor[from] as usize;
        targets[at] = to as u32;
        confs[at] = conf;
        cursor[from] += 1;
    };
    for edge in &graph.edges {
        match edge.kind {
            EdgeKind::References { from, to, .. } => push_edge(
                &mut cursor,
                node_index(from),
                files_len + to.0 as usize,
                edge.confidence,
            ),
            EdgeKind::ImportsFile { from, to } => {
                push_edge(&mut cursor, from.0 as usize, to.0 as usize, edge.confidence)
            }
            EdgeKind::ReferencesFile { from, to } => push_edge(
                &mut cursor,
                node_index(from),
                to.0 as usize,
                edge.confidence,
            ),
            EdgeKind::InvokesFile { from, to } => {
                push_edge(
                    &mut cursor,
                    node_index(from),
                    to.0 as usize,
                    edge.confidence,
                );
                // Executing the program runs its entry points: each Production root in the
                // invoked file, at the weaker of the invocation and the root's own strength.
                for &(root, root_conf) in &prod_roots_by_file[to.0 as usize] {
                    push_edge(
                        &mut cursor,
                        node_index(from),
                        root as usize,
                        edge.confidence.min(root_conf),
                    );
                }
            }
            EdgeKind::Wildcard { from } => {
                // Plausible target set, absent narrower DynamicUse metadata (RFC 0005 §1):
                // every symbol declared in the same file, at `possible`.
                for &sym in &declared_in[from.0 as usize] {
                    push_edge(
                        &mut cursor,
                        from.0 as usize,
                        files_len + sym as usize,
                        Confidence::Possible,
                    );
                }
            }
            _ => {}
        }
    }
    for (i, symbol) in graph.symbols.iter().enumerate() {
        push_edge(
            &mut cursor,
            files_len + i,
            symbol.file.0 as usize,
            Confidence::Certain,
        );
    }

    let mut seeds: [Vec<(u32, Confidence)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for edge in &graph.edges {
        if let EdgeKind::Root { kind, target } = edge.kind {
            seeds[kind_index(kind)].push((node_index(target) as u32, edge.confidence));
        }
    }

    // R(kind, tau) for every (kind, tau), literally: BFS seeded only by roots whose own
    // confidence is >= tau, traversing only edges with confidence >= tau. The module-load
    // rule needs no special-case here any more — it's the implicit CSR edge above.
    let mut reached: Vec<BitSet> = Vec::with_capacity(9);
    for (k, _) in ROOT_KINDS.iter().enumerate().map(|(i, _)| (i, ())) {
        for &tau in &TIERS {
            let mut visited = BitSet::new(n);
            let mut queue: Vec<u32> = Vec::new();
            for &(node, conf) in &seeds[k] {
                if conf >= tau && visited.insert(node as usize) {
                    queue.push(node);
                }
            }
            while let Some(node) = queue.pop() {
                let (start, end) = (
                    offsets[node as usize] as usize,
                    offsets[node as usize + 1] as usize,
                );
                for at in start..end {
                    if confs[at] >= tau {
                        let next = targets[at];
                        if visited.insert(next as usize) {
                            queue.push(next);
                        }
                    }
                }
            }
            reached.push(visited);
        }
    }

    // First-match precedence over every node: Production > TestOnly > ToolingOnly; within a
    // color, the strongest tau achieved. (Traversal order above is scheduling; membership is
    // a set — so a stack-based visit order changes nothing observable.)
    let mut colors: Vec<(Reachability, Confidence)> =
        vec![(Reachability::Unreachable, Confidence::Certain); n];
    for (idx, color) in colors.iter_mut().enumerate() {
        'resolve: for (k, &(_, kind_color)) in ROOT_KINDS.iter().enumerate() {
            for (t, &tau) in TIERS.iter().enumerate() {
                if reached[k * 3 + t].contains(idx) {
                    *color = (kind_color, tau);
                    break 'resolve;
                }
            }
        }
    }

    let reached_possible = [reached[2].clone(), reached[5].clone(), reached[8].clone()];
    ReachabilityMap {
        files_len,
        colors,
        reached_possible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, VisibilityLevel};
    use crate::graph::{DependencyNode, FileNode, ProjectGraph, SymbolNode};
    use crate::vocab::{
        Edge, FileClass, FileId, FileOrigin, FileRole, Provenance, RefKind, SymbolId, SymbolKind,
    };
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
            package: crate::vocab::PackageId(0),
            unit: None,
            test_spans: Vec::new(),
            string_call_sites: Vec::new(),
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
            member_of: None,
            signature_span: None,
        }
    }

    fn edge(kind: EdgeKind, confidence: Confidence) -> Edge {
        Edge {
            owner: crate::vocab::FileId(0),
            kind,
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
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
    fn reachable_from_sees_past_color_precedence() {
        // Same worked example as `color_precedence_beats_confidence_tier`: target's *color* is
        // Production (it wins the precedence race), but it's independently reachable from the
        // test root too — `reachable_from` must report that even though `get`'s color hides it.
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
                Confidence::Certain,
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
            reach.get(NodeRef::File(FileId(2))).0,
            Reachability::Production
        );
        assert!(reach.reachable_from(RootKind::Test, NodeRef::File(FileId(2))));
        assert!(!reach.reachable_from(RootKind::Test, NodeRef::File(FileId(0))));
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
                    from: NodeRef::Symbol(SymbolId(0)),
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
    fn a_symbol_only_root_pulls_its_owning_file_into_reachability_too() {
        // The exact shape a symbol-targeted root produces (RFC 0011 §5's per-export library-
        // mode promotion; Go's `func main`/exported-declaration promotion, docs/adapters/go.md
        // §2): `main` is a root, declared in `main.ts`, and `main.ts` (not the `main` symbol
        // itself — references are file-granular, this module's own adjacency doc) references
        // `helper`, declared in a *different* file. `helper` must end up reachable — which
        // requires `main.ts` itself to be visited by the BFS, not just the `main` symbol.
        let files = vec![file("main.ts"), file("helper.ts")];
        let symbols = vec![symbol(FileId(0), "main"), symbol(FileId(1), "helper")];
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
                    from: NodeRef::File(FileId(0)), // file-attributed, not symbol-attributed
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = compute(&graph);
        assert_eq!(
            reach.get(NodeRef::File(FileId(0))).0,
            Reachability::Production,
            "the root symbol's owning file must itself become reachable"
        );
        assert_eq!(
            reach.get(NodeRef::Symbol(SymbolId(1))).0,
            Reachability::Production,
            "helper, referenced from main.ts, must be reachable through it"
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
    fn invoking_a_program_reaches_its_production_roots_and_their_call_tree() {
        // The invoked-program rule: a test file executing `bin.ts` as a subprocess reaches
        // the file AND its Production root (`main`), and through `main`'s references the
        // whole call tree — while a non-Production root in the same file stays untouched
        // (executing the program runs its entry point, not its tooling entries).
        let files = vec![file("e2e.test.ts"), file("bin.ts")];
        let symbols = vec![
            symbol(FileId(1), "main"),
            symbol(FileId(1), "helper"),
            symbol(FileId(1), "codegen"),
        ];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Test,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::Symbol(SymbolId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::Root {
                    kind: RootKind::Tooling,
                    target: NodeRef::Symbol(SymbolId(2)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::InvokesFile {
                    from: NodeRef::File(FileId(0)),
                    to: FileId(1),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::Symbol(SymbolId(0)),
                    to: SymbolId(1),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = compute(&graph);
        for node in [
            NodeRef::File(FileId(1)),
            NodeRef::Symbol(SymbolId(0)),
            NodeRef::Symbol(SymbolId(1)),
        ] {
            assert!(
                reach.reachable_from(RootKind::Test, node),
                "the invoked bin, its entry, and the entry's callees are all test-reached"
            );
        }
        assert!(
            !reach.reachable_from(RootKind::Test, NodeRef::Symbol(SymbolId(2))),
            "a non-Production root in the invoked file is not part of the program's execution"
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

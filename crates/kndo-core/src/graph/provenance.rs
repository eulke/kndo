//! Which components a node's facts came from — the `sources` field of both the query
//! envelopes (`describe`) and every finding.
//!
//! [`crate::vocab::Provenance`] lives on graph *edges*, not on nodes and not on findings, so
//! answering "who contributed this?" is a derivation, not a copy. `describe` and every
//! finding's `sources` field both need that same derivation; a per-call scan of every edge in
//! the graph would cost real time at scale (a `query` batch of sixty describes would scan the
//! edge list sixty times). This is the one index both read.
//!
//! **What counts as a source of a node**, normatively:
//!
//! 1. The adapter that **claimed** the node's file. A claimed file's declarations, spans and
//!    metrics are that adapter's facts whether or not a single edge ever touched it — an
//!    isolated file reporting no sources would be saying "nobody looked at this", which is
//!    the opposite of true.
//! 2. Every adapter or plugin that contributed an **edge touching** the node, in either
//!    direction. Both directions, because provenance answers "whose facts is this verdict
//!    resting on" and an incoming reference is as load-bearing as an outgoing one — that is
//!    exactly what makes a `kndo:thymeleaf` edge show up on the controller it keeps alive.
//!
//! Note what this deliberately does NOT claim: it names components whose facts are *present*,
//! never a component that would have contributed had it activated. A verdict of absence
//! (`unused` is the whole category) rests on the silence of every component that ran, and no
//! field can name the one that did not.

use rustc_hash::FxHashMap as HashMap;

use crate::graph::ProjectGraph;
use crate::vocab::{DependencyId, FileId, NodeRef, PackageId, Provenance, SymbolId};

/// A node's sources, resolvable in `O(log n)` after one `O(edges log edges)` build.
///
/// Stored as three sorted `(node index, label index)` pair lists rather than a map of vectors:
/// a graph has tens of thousands of symbols and almost every one of them has exactly one
/// source, so a `Vec` per node would be tens of thousands of allocations to hold one `u16`
/// each. Flat lists make the build three sorts and the lookup a `partition_point`.
pub(crate) struct ProvenanceIndex {
    labels: Vec<String>,
    files: Vec<(u32, u16)>,
    symbols: Vec<(u32, u16)>,
    dependencies: Vec<(u32, u16)>,
    packages: Vec<(u32, u16)>,
}

impl ProvenanceIndex {
    pub(crate) fn build(graph: &ProjectGraph) -> Self {
        let mut interner: HashMap<Provenance, u16> = HashMap::default();
        let mut labels: Vec<String> = Vec::new();
        let mut files: Vec<(u32, u16)> = Vec::new();
        let mut symbols: Vec<(u32, u16)> = Vec::new();
        let mut dependencies: Vec<(u32, u16)> = Vec::new();
        let mut packages: Vec<(u32, u16)> = Vec::new();

        let mut intern = |p: &Provenance, labels: &mut Vec<String>| -> u16 {
            if let Some(&id) = interner.get(p) {
                return id;
            }
            // `u16` because the label set is the registered adapters plus the active plugins:
            // tens, and a project with 65k distinct components has a different problem. The
            // saturating cast keeps a pathological case wrong-but-bounded rather than
            // panicking mid-run.
            let id = u16::try_from(labels.len()).unwrap_or(u16::MAX);
            labels.push(label(p));
            interner.insert(p.clone(), id);
            id
        };

        // (1) The claim. `FileNode.language` IS the claiming adapter's id — the engine's own
        // per-adapter file counts compare the two directly.
        for (i, file) in graph.files.iter().enumerate() {
            if let Some(language) = &file.language {
                let id = intern(&Provenance::Adapter(language.clone()), &mut labels);
                files.push((i as u32, id));
            }
        }

        // (1b) The manifest's claim, the package-shaped half of the same rule. A manifest is
        // a `Package`, not a `File`, so nothing above reaches it — and it is the only answer
        // available for a dependency that is *declared and never imported*, which has no edge
        // anywhere in the graph and is precisely what `unused` reports.
        for (i, package) in graph.packages.iter().enumerate() {
            for language in &package.manifest_claim_languages {
                let id = intern(&Provenance::Adapter(language.clone()), &mut labels);
                packages.push((i as u32, id));
            }
        }

        // (2) Every edge, marking both endpoints. Both, because provenance answers "whose
        // facts is this resting on": an incoming reference is as load-bearing as an outgoing
        // one, and it is what puts a plugin's name on the node it keeps alive.
        for edge in &graph.edges {
            let id = intern(&edge.source, &mut labels);
            let node = |n: NodeRef, files: &mut Vec<(u32, u16)>, syms: &mut Vec<(u32, u16)>| match n
            {
                NodeRef::File(f) => files.push((f.0, id)),
                NodeRef::Symbol(s) => syms.push((s.0, id)),
            };
            match edge.kind {
                crate::vocab::EdgeKind::ImportsFile { from, to } => {
                    files.push((from.0, id));
                    files.push((to.0, id));
                }
                crate::vocab::EdgeKind::ImportsDependency { from, to } => {
                    files.push((from.0, id));
                    dependencies.push((to.0, id));
                }
                crate::vocab::EdgeKind::References { from, to, .. } => {
                    node(from, &mut files, &mut symbols);
                    symbols.push((to.0, id));
                }
                crate::vocab::EdgeKind::Declares { file, symbol } => {
                    files.push((file.0, id));
                    symbols.push((symbol.0, id));
                }
                crate::vocab::EdgeKind::Root { target, .. } => {
                    node(target, &mut files, &mut symbols)
                }
                crate::vocab::EdgeKind::Wildcard { from } => files.push((from.0, id)),
                crate::vocab::EdgeKind::ReferencesFile { from, to }
                | crate::vocab::EdgeKind::InvokesFile { from, to } => {
                    node(from, &mut files, &mut symbols);
                    files.push((to.0, id));
                }
            }
        }

        for list in [&mut files, &mut symbols, &mut dependencies, &mut packages] {
            list.sort_unstable();
            list.dedup();
        }
        ProvenanceIndex {
            labels,
            files,
            symbols,
            dependencies,
            packages,
        }
    }

    /// The union over any set of nodes — one node for a symbol finding, several for a finding
    /// that spans places (a directory rollup, a clone group across files). Sorted by *label*,
    /// not by the interner's insertion order, so the list a consumer reads is stable across
    /// runs, thread counts and cache states.
    pub(crate) fn union(&self, nodes: impl IntoIterator<Item = Node>) -> Vec<&str> {
        let mut out: Vec<&str> = nodes
            .into_iter()
            .flat_map(|n| match n {
                Node::File(f) => lookup(&self.files, f.0),
                Node::Symbol(s) => lookup(&self.symbols, s.0),
                Node::Dependency(d) => lookup(&self.dependencies, d.0),
                Node::Package(p) => lookup(&self.packages, p.0),
            })
            .map(|&(_, id)| self.labels[id as usize].as_str())
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// A node to ask about, independent of `query`'s own `NavNode` — that type is private to that
/// module and carries selector-resolution concerns this index has no business knowing.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Node {
    File(FileId),
    Symbol(SymbolId),
    Dependency(DependencyId),
    /// A manifest's own node. Its sources are every adapter that would claim the manifest
    /// (`PackageNode::manifest_claim_languages`) — plural on purpose: a `package.json` in a
    /// polyglot repo is read by more than one.
    Package(PackageId),
}

/// The contiguous run of pairs belonging to one node. The lists are sorted by `(node, label)`,
/// so a node's labels are adjacent and two `partition_point`s bound them.
fn lookup(list: &[(u32, u16)], node: u32) -> &[(u32, u16)] {
    let start = list.partition_point(|&(n, _)| n < node);
    let len = list[start..].partition_point(|&(n, _)| n == node);
    &list[start..start + len]
}

/// The `sources` string for one provenance, and the single place the spelling is decided:
/// `adapter:<id>`, `plugin:<coordinate>`, `core:surface`.
fn label(p: &Provenance) -> String {
    match p {
        Provenance::Adapter(id) => format!("adapter:{id}"),
        Provenance::Plugin(id) => format!("plugin:{id}"),
        Provenance::Surface => "core:surface".to_string(),
    }
}

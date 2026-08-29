//! The surface-member closure: promoting transitive members of surface types.
//! Idempotent; re-run once per run on every path.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::vocab::{Confidence, Edge, EdgeKind, NodeRef, SymbolId};

#[allow(unused_imports)]
use super::*;

/// Surface-member closure — completes "which symbols
/// can an external consumer name?". Manifest promotion, phase 2.7's whole-surface expansion,
/// and the barrel-indirection promotion already root every *directly* re-exported symbol; what
/// none of them cover is **members**: a `pub` method of a surface type is consumer-callable
/// API even though nothing in-package references it. One rule,
/// to a fixpoint for nested containers: a member of a surface symbol whose own rung is
/// surface-transitive is surface.
///
/// Idempotent by construction: strips every `Provenance::Surface` edge and recomputes from the
/// current graph — the engine calls it once per run on every path (cold, patch, warm snapshot
/// hit) before analysis. A snapshot may carry a previous run's closure edges; strip-first
/// makes the carryover irrelevant, and the incremental patch needs no cross-file ownership
/// story for them. O(symbols + edges).
pub(crate) fn recompute_surface_closure(graph: &mut ProjectGraph) {
    graph
        .edges
        .retain(|e| e.source != crate::vocab::Provenance::Surface);

    // Seed: every symbol already rooted as production surface; a project with no members at
    // all has nothing to close over. Borrowed keys throughout — this runs on every path
    // including warm no-ops, so it allocates no strings per symbol.
    let surface: HashSet<SymbolId> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Root {
                kind: crate::vocab::RootKind::Production,
                target: NodeRef::Symbol(s),
            } => Some(s),
            _ => None,
        })
        .collect();
    if surface.is_empty() || !graph.symbols.iter().any(|s| s.member_of.is_some()) {
        return;
    }

    let members_of = surface_member_index(graph);
    let emitted = propagate_surface_members(graph, surface, &members_of);
    graph.edges.extend(emitted.into_iter().map(|m| Edge {
        kind: EdgeKind::Root {
            kind: crate::vocab::RootKind::Production,
            target: NodeRef::Symbol(m),
        },
        // Derived, not declared: the member is API because its container is — Probable, the
        // derived-promotion tier.
        confidence: Confidence::Probable,
        source: crate::vocab::Provenance::Surface,
        span: Some(graph.symbols[m.0 as usize].span),
        owner: graph.symbols[m.0 as usize].file,
    }));
}

/// Members grouped under their same-file container (`member_of` is a name —
/// prefer the container whose span encloses the member, the nesting the name refers to).
pub(crate) fn surface_member_index(graph: &ProjectGraph) -> HashMap<SymbolId, Vec<SymbolId>> {
    let mut by_file_name: HashMap<(u32, &str), Vec<SymbolId>> = HashMap::default();
    for (i, sym) in graph.symbols.iter().enumerate() {
        by_file_name
            .entry((sym.file.0, sym.name.as_str()))
            .or_default()
            .push(SymbolId(i as u32));
    }
    let mut members_of: HashMap<SymbolId, Vec<SymbolId>> = HashMap::default();
    for (i, sym) in graph.symbols.iter().enumerate() {
        let Some(container_name) = sym.member_of.as_deref() else {
            continue;
        };
        let Some(candidates) = by_file_name.get(&(sym.file.0, container_name)) else {
            continue;
        };
        let container = candidates
            .iter()
            .find(|c| {
                let cs = &graph.symbols[c.0 as usize];
                cs.span.start <= sym.span.start && sym.span.end <= cs.span.end
            })
            .or_else(|| candidates.first());
        if let Some(&c) = container {
            members_of.entry(c).or_default().push(SymbolId(i as u32));
        }
    }
    members_of
}

/// One membership test of the fixpoint: not already in, exported, and on a
/// surface-transitive rung of its language's ladder.
pub(crate) fn member_joins_surface(
    graph: &ProjectGraph,
    ladders: &std::collections::BTreeMap<&str, &[crate::adapter::VisibilityRung]>,
    m: SymbolId,
    surface: &HashSet<SymbolId>,
) -> bool {
    let sym = &graph.symbols[m.0 as usize];
    let ladder = graph.files[sym.file.0 as usize]
        .language
        .as_deref()
        .and_then(|l| ladders.get(l).copied());
    !surface.contains(&m) && sym.exported && rung_surface_transitive(ladder, sym.visibility)
}

/// The fixpoint (deterministic: seeds sorted, output sorted): a member of a surface symbol
/// joins the surface when its own rung is surface-transitive; nested containers cascade.
pub(crate) fn propagate_surface_members(
    graph: &ProjectGraph,
    mut surface: HashSet<SymbolId>,
    members_of: &HashMap<SymbolId, Vec<SymbolId>>,
) -> Vec<SymbolId> {
    let ladders: std::collections::BTreeMap<&str, &[crate::adapter::VisibilityRung]> = graph
        .visibility_ladders
        .iter()
        .map(|(l, r)| (l.as_str(), r.as_slice()))
        .collect();
    let mut queue: Vec<SymbolId> = surface.iter().copied().collect();
    queue.sort_unstable();
    let mut emitted: Vec<SymbolId> = Vec::new();
    while let Some(container) = queue.pop() {
        for &m in members_of.get(&container).into_iter().flatten() {
            if member_joins_surface(graph, &ladders, m, &surface) {
                surface.insert(m);
                emitted.push(m);
                queue.push(m);
            }
        }
    }
    emitted.sort_unstable();
    emitted
}

/// Whether a declaration's rung can travel through a re-export chain to outside its package
/// (`VisibilityRung::surface_transitive`). No ladder (or an index the ladder
/// doesn't cover) falls back to `true`: for a binary-visibility language, the `exported` bit
/// alone decides membership, since there is no ladder to consult.
pub(crate) fn rung_surface_transitive(
    ladder: Option<&[crate::adapter::VisibilityRung]>,
    vis: crate::adapter::VisibilityLevel,
) -> bool {
    match ladder {
        Some(l) => l.get(vis.0 as usize).is_none_or(|r| r.surface_transitive),
        None => true,
    }
}

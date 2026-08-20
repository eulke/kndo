//! `internal-only` — declared visibility wider than any real usage requires (RFC 0005 §7, group
//! `waste`): the "declared > required" half of the visibility-mismatch pair (`private-type-leak`
//! is the other direction). "The analysis computes the **tightest sufficient visibility** — the
//! lowest ladder level that still covers the origin of every incoming reference. Declared above
//! it ⇒ finding."
//!
//! **Ladder scope, honestly**: the adapter-declared visibility ladder (RFC 0005 §7: private →
//! file → package/crate → public) is currently binary in the only adapter that exists —
//! [`crate::adapter::VisibilityLevel`] is `0` (unexported, file-private) or `1` (exported) for
//! JS/TS (`kndo-adapter-js/src/extraction.rs`), and no graph fact yet distinguishes "same
//! package, different file" from "different package" for a reference's origin. So this analysis
//! only ever computes one of two tightest-sufficient answers — file-private (an exported symbol
//! referenced only from its own declaring file) or "declared is already tightest" (some
//! reference crosses the file boundary) — not the fuller package/crate middle rung the RFC's
//! ladder names. It's the common, useful case (RFC 0005 §7's own headline example: "exported
//! symbol referenced only within its own file"); the middle rung needs a richer per-reference
//! fact (which package the reference came from) and a language ladder with an actual middle
//! rung (Rust `pub(crate)`) before it's honestly buildable. Likewise "public member used only
//! inside its own type" (`internal-only:method`) needs a *type*-scoped declaration fact (finer
//! than "declared in this file") the graph doesn't carry yet — also not attempted here.
//!
//! Exemptions: a symbol that is itself a root target (library-mode public API, a test
//! file's exported fixtures, a tooling config's exports — RFC 0011 §5's promotion, already
//! wired in `graph::assemble`'s phase 3a) is externally consumed by definition, regardless of
//! whether any in-graph reference reaches it — never a candidate. A symbol with *zero* incoming
//! references at all, or one that's [`Reachability::Unreachable`] outright (dead code can have a
//! same-file self-reference and nothing else — a same-file `console.log(helper())` in a file
//! nothing imports), is `unused`'s verdict, not this one: suggesting "narrow the visibility" on
//! code already flagged for deletion is redundant noise (same rollup-taxonomy reasoning
//! `unused.rs` itself documents for skipping symbols in an already-unreachable file).
//!
//! Confidence: `Certain` when every incoming reference is same-file; if the only cross-file
//! evidence keeping it exported is a `Possible`-confidence edge (RFC 0005 §1's wildcard/dynamic
//! tier — unreliable either way), the verdict still fires but demoted to `Possible`, mirroring
//! "confidence demotes through wildcard edges like every reachability verdict."

use std::collections::{HashMap, HashSet};

use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::engine::{Finding, Location, Severity};
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, EdgeKind, FileId, FileOrigin, NodeRef, SymbolId};

fn origin_file(graph: &ProjectGraph, node: NodeRef) -> FileId {
    match node {
        NodeRef::File(f) => f,
        NodeRef::Symbol(s) => graph.symbols[s.0 as usize].file,
    }
}

pub fn find_internal_only(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let root_targets: HashSet<NodeRef> = graph
        .edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::Root { target, .. } => Some(target),
            _ => None,
        })
        .collect();

    let mut refs_by_target: HashMap<SymbolId, Vec<(FileId, Confidence)>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::References { from, to, .. } = edge.kind {
            refs_by_target
                .entry(to)
                .or_default()
                .push((origin_file(graph, from), edge.confidence));
        }
    }

    let mut findings = Vec::new();
    for (index, symbol) in graph.symbols.iter().enumerate() {
        let file = &graph.files[symbol.file.0 as usize];
        let Some(class) = file.class else {
            continue; // unclaimed — out of scope, not a verdict
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        if symbol.visibility.0 == 0 {
            continue; // already the tightest level there is — nothing to narrow
        }

        let symbol_id = SymbolId(index as u32);
        if root_targets.contains(&NodeRef::Symbol(symbol_id)) {
            continue; // roots are externally consumed by definition (RFC 0005 §7 exemption)
        }
        if reach.get(NodeRef::Symbol(symbol_id)).0 == Reachability::Unreachable {
            continue; // dead code — `unused`'s verdict, not this one
        }

        let Some(refs) = refs_by_target.get(&symbol_id) else {
            continue; // zero incoming references at all — `unused`'s verdict, not this one
        };
        let cross_file_strong = refs
            .iter()
            .any(|&(f, c)| f != symbol.file && c >= Confidence::Probable);
        if cross_file_strong {
            continue; // a Certain/Probable cross-file reference justifies the declared visibility
        }
        let cross_file_weak = refs.iter().any(|&(f, _)| f != symbol.file);
        let confidence = if cross_file_weak {
            Confidence::Possible
        } else {
            Confidence::Certain
        };

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        findings.push(Finding {
            id: finding_id("internal-only", facet, path, symbol.name.as_str(), ""),
            category: "internal-only".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.to_string(),
            severity: Severity::Info, // RFC 0005 §7: info
            confidence,
            message: format!(
                "{path}#{} is exported but only used within its own file — consider not exporting this {facet}",
                symbol.name
            ),
            location: Location {
                path: Some(file.path.clone()),
                range: Some(symbol.span),
                symbol: Some(symbol.name.to_string()),
                package: graph.package_name(file.package).map(str::to_string),
            },
            delta: None,
            delta_origin: None,
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ProjectPath, VisibilityLevel};
    use crate::graph::{FileNode, ProjectGraph, SymbolNode};
    use crate::vocab::{
        Edge, FileClass, FileOrigin, FileRole, Provenance, RefKind, RootKind, SymbolKind,
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
        }
    }

    fn symbol(file: FileId, name: &str, visibility: u8) -> SymbolNode {
        SymbolNode {
            file,
            name: SmolStr::new(name),
            kind: SymbolKind::Function,
            span: Default::default(),
            exported: visibility > 0,
            visibility: VisibilityLevel(visibility),
        }
    }

    fn edge(kind: EdgeKind, confidence: Confidence) -> Edge {
        Edge {
            kind,
            confidence,
            source: Provenance::Adapter(SmolStr::new("mock")),
            span: None,
        }
    }

    #[test]
    fn exported_symbol_used_only_in_its_own_file_is_internal_only() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "internal-only");
        assert_eq!(findings[0].group, "waste");
        assert_eq!(findings[0].confidence, Confidence::Certain);
        assert!(findings[0].message.contains("src/a.ts#helper"));
    }

    #[test]
    fn exported_symbol_referenced_cross_file_is_not_internal_only() {
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(1)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn dead_code_with_a_same_file_self_reference_is_exempt_not_double_flagged() {
        // No root anywhere — the whole graph is unreachable. `helper` still has an incoming
        // same-file reference (e.g. `console.log(helper())` in a file nothing imports), which
        // without the reachability gate would read as internal-only — redundant with `unused`
        // already covering the same dead code (found via real-CLI dogfooding).
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn unexported_symbol_is_already_tightest_and_never_a_candidate() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 0)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn zero_references_is_unused_s_verdict_not_this_one() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let graph = ProjectGraph::for_test(files, symbols, vec![], vec![]);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_root_symbol_is_exempt_even_if_only_self_file_referenced() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "publicApi", 1)];
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
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn a_root_symbol_with_no_references_at_all_is_still_exempt() {
        // Library-mode promotion: nothing in the graph calls it, but it's the package's public
        // API surface — externally consumed by definition, never a candidate for either verdict.
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "publicApi", 1)];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::Symbol(SymbolId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn only_possible_confidence_cross_file_reference_demotes_not_exempts() {
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(1)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(1)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Possible,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let findings = find_internal_only(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Possible);
    }

    #[test]
    fn reference_from_within_another_symbol_resolves_to_that_symbol_s_file() {
        // The referencing edge's `from` is a Symbol, not a File — origin_file must resolve it
        // through the symbol's own `file` field, same cross-file logic either way.
        let files = vec![file("src/a.ts"), file("src/b.ts")];
        let symbols = vec![
            symbol(FileId(0), "helper", 1),
            symbol(FileId(1), "caller", 0),
        ];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::Symbol(SymbolId(1)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn generated_origin_is_exempt() {
        let files = vec![FileNode {
            path: ProjectPath(SmolStr::new("dist/a.ts")),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass {
                role: FileRole::Production,
                origin: FileOrigin::Generated,
            }),
            package: crate::vocab::PackageId(0),
        }];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![edge(
            EdgeKind::References {
                from: NodeRef::File(FileId(0)),
                to: SymbolId(0),
                kind: RefKind::Call,
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        assert!(find_internal_only(&graph, &reach).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("src/a.ts")];
        let symbols = vec![symbol(FileId(0), "helper", 1)];
        let edges = vec![
            edge(
                EdgeKind::Root {
                    kind: RootKind::Production,
                    target: NodeRef::File(FileId(0)),
                },
                Confidence::Certain,
            ),
            edge(
                EdgeKind::References {
                    from: NodeRef::File(FileId(0)),
                    to: SymbolId(0),
                    kind: RefKind::Call,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = crate::analysis::reachability::compute(&graph);
        let a = find_internal_only(&graph, &reach);
        let b = find_internal_only(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }
}

//! `unused` — unreachable files and symbols (RFC 0005 §2, §4).
//!
//! Symbol-level findings only fire for symbols in files granularity has already ruled *in*
//! scope: a symbol whose owning file is itself `unreachable` is skipped, because the file-level
//! finding already covers it (taxonomy rollup rule — "the file finding replaces the per-symbol
//! findings it summarizes"; reporting both would be redundant, not more informative). Reference
//! evidence is file-granular, not per-enclosing-symbol (see `graph::assemble`'s phase 3b and
//! `EdgeKind::References`'s doc) — safe for this verdict either way: a symbol is `unreachable`
//! only when *nothing*, from *any* file, references it.
//!
//! Directory rollup (§ taxonomy: "a directory whose every file carries the same verdict rolls
//! up once more") is not implemented here — every unused file/symbol is reported individually
//! for now.

use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileId, FileOrigin, NodeRef, SymbolId};

pub fn find_unused_files(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, file) in graph.files.iter().enumerate() {
        // Unclaimed: no adapter recognized this file, so no adapter has an opinion on whether
        // it can be a root or a target — out of scope, not a verdict.
        let Some(class) = file.class else {
            continue;
        };
        // Generated/vendored origins are exempt by default (RFC 0005 §4).
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }

        let file_id = FileId(index as u32);
        let (color, confidence) = reach.get(NodeRef::File(file_id));
        // Rule 4 (RFC 0005 §1): unreachable at every root kind and every confidence tier —
        // dead is always certain, so this is the only case `unused` ever fires for.
        if color != Reachability::Unreachable {
            continue;
        }
        debug_assert_eq!(confidence, Confidence::Certain);

        let path = file.path.0.as_str();
        findings.push(Finding {
            id: finding_id("unused", "file", path, "", ""),
            category: "unused".to_string(),
            group: "waste".to_string(),
            subject_kind: "file".to_string(),
            message: format!("{path} is unreachable: no root or import reaches it"),
        });
    }
    findings
}

pub fn find_unused_symbols(graph: &ProjectGraph, reach: &ReachabilityMap) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (index, symbol) in graph.symbols.iter().enumerate() {
        let file = &graph.files[symbol.file.0 as usize];
        // Symbols only ever exist for claimed files (graph::assemble only extracts
        // declarations through an adapter) — defensive, not expected to actually skip.
        let Some(class) = file.class else {
            continue;
        };
        if matches!(class.origin, FileOrigin::Generated | FileOrigin::Vendored) {
            continue;
        }
        if reach.get(NodeRef::File(symbol.file)).0 == Reachability::Unreachable {
            continue; // rollup: the file-level finding already covers every symbol in it
        }

        let symbol_id = SymbolId(index as u32);
        let (color, confidence) = reach.get(NodeRef::Symbol(symbol_id));
        if color != Reachability::Unreachable {
            continue;
        }
        debug_assert_eq!(confidence, Confidence::Certain);

        let path = file.path.0.as_str();
        let facet = symbol.kind.facet();
        findings.push(Finding {
            id: finding_id("unused", facet, path, symbol.name.as_str(), ""),
            category: "unused".to_string(),
            group: "waste".to_string(),
            subject_kind: facet.to_string(),
            message: format!(
                "{path}#{} is unreachable: nothing references this {facet}",
                symbol.name
            ),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::analysis::reachability;
    use crate::graph::FileNode;
    use crate::vocab::{Edge, EdgeKind, FileClass, FileRole, NodeRef, Provenance, RootKind};
    use smol_str::SmolStr;

    fn file(path: &str, class: Option<FileClass>) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: class.map(|_| SmolStr::new("mock")),
            class,
            package: crate::vocab::PackageId(0),
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
    fn orphan_file_is_reported_unused() {
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let findings = find_unused_files(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].subject_kind, "file");
        assert_eq!(findings[0].group, "waste");
        assert!(findings[0].message.contains("orphan.ts"));
    }

    #[test]
    fn root_file_is_not_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let edges = vec![edge(
            EdgeKind::Root {
                kind: RootKind::Production,
                target: NodeRef::File(FileId(0)),
            },
            Confidence::Certain,
        )];
        let graph = ProjectGraph::for_test(files, vec![], vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn imported_file_is_not_reported() {
        let files = vec![
            file("main.ts", Some(FileClass::default())),
            file("lib.ts", Some(FileClass::default())),
        ];
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
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn unclaimed_file_is_out_of_scope() {
        let files = vec![file("README.md", None)];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn generated_orphan_is_exempt() {
        let class = FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Generated,
        };
        let files = vec![file("dist/bundle.js", Some(class))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn vendored_orphan_is_exempt() {
        let class = FileClass {
            role: FileRole::Production,
            origin: FileOrigin::Vendored,
        };
        let files = vec![file("vendor/lib.js", Some(class))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_files(&graph, &reach).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let graph = ProjectGraph::for_test(files, vec![], vec![], vec![]);
        let reach = reachability::compute(&graph);
        let a = find_unused_files(&graph, &reach);
        let b = find_unused_files(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }

    // ---------------------------------------------------------------- symbols

    use crate::adapter::VisibilityLevel;
    use crate::graph::SymbolNode;
    use crate::vocab::SymbolKind;

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

    #[test]
    fn dead_symbol_in_a_reachable_file_is_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used"), symbol(FileId(0), "dead")];
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
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let findings = find_unused_symbols(&graph, &reach);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "unused");
        assert_eq!(findings[0].subject_kind, "function");
        assert!(findings[0].message.contains("main.ts#dead"));
    }

    #[test]
    fn referenced_symbol_is_not_reported() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used")];
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
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        assert!(find_unused_symbols(&graph, &reach).is_empty());
    }

    #[test]
    fn symbol_in_an_already_unused_file_is_not_double_reported() {
        // The file itself has no root and no importer — file-level `unused` already fires;
        // the rollup rule says the symbol finding must not duplicate it.
        let files = vec![file("orphan.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "dead")];
        let graph = ProjectGraph::for_test(files, symbols, vec![], vec![]);
        let reach = reachability::compute(&graph);
        assert!(find_unused_symbols(&graph, &reach).is_empty());
        assert_eq!(find_unused_files(&graph, &reach).len(), 1);
    }

    #[test]
    fn symbol_finding_id_is_stable_across_runs() {
        let files = vec![file("main.ts", Some(FileClass::default()))];
        let symbols = vec![symbol(FileId(0), "used"), symbol(FileId(0), "dead")];
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
                    to: crate::vocab::SymbolId(0),
                    kind: crate::vocab::RefKind::Read,
                },
                Confidence::Certain,
            ),
        ];
        let graph = ProjectGraph::for_test(files, symbols, vec![], edges);
        let reach = reachability::compute(&graph);
        let a = find_unused_symbols(&graph, &reach);
        let b = find_unused_symbols(&graph, &reach);
        assert_eq!(a[0].id, b[0].id);
    }
}

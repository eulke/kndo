//! `unused` — unreachable files (RFC 0005 §2, §4). File granularity only for now: symbol-level
//! `unused` needs `References` edges, which no adapter extracts yet (identifier-usage tracking
//! is explicitly deferred, see `kndo-adapter-js`'s extraction module doc). The reachability
//! engine underneath is already generic over files and symbols — this analysis just doesn't
//! ask it about symbols yet, so there's nothing to rewrite once it can.
//!
//! Directory rollup (§ taxonomy: "a directory whose every file carries the same verdict rolls
//! up once more") is not implemented here either — every unused file is reported individually
//! for now.

use crate::analysis::finding_id;
use crate::analysis::reachability::{Reachability, ReachabilityMap};
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::vocab::{Confidence, FileId, FileOrigin, NodeRef};

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
}

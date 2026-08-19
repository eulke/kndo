//! `undeclared` — an import resolves to a dependency no manifest declares (RFC 0005 §5): a
//! phantom dependency that only works today because hoisting or transitive resolution happens
//! to make it resolvable at all — the kind of thing that breaks on a clean install elsewhere.
//!
//! Declared-ness is checked project-wide (`graph.declared_dependencies`), not per-owning-
//! package: RFC 0011's `Package` node/ownership hasn't landed yet, so in a monorepo where
//! sibling package B declares a name that package A merely imports, this reads as "declared"
//! for A too — a false negative. Correct for the single-manifest case M1 treats as
//! foundational (RFC 0011 §3: "a repo with no manifest at all is one implicit Package"); the
//! monorepo imprecision is a known gap, closed once ownership exists to scope the check.

use std::collections::{HashMap, HashSet};

use crate::analysis::finding_id;
use crate::engine::Finding;
use crate::graph::ProjectGraph;
use crate::vocab::{DependencyId, EdgeKind};

pub fn find_undeclared_dependencies(graph: &ProjectGraph) -> Vec<Finding> {
    let declared: HashSet<&str> = graph
        .declared_dependencies
        .iter()
        .map(|d| d.name.as_str())
        .collect();

    let mut importers_by_dep: HashMap<DependencyId, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if let EdgeKind::ImportsDependency { from, to } = edge.kind {
            importers_by_dep
                .entry(to)
                .or_default()
                .push(graph.files[from.0 as usize].path.0.as_str());
        }
    }

    let mut findings = Vec::new();
    for (index, dep) in graph.dependencies.iter().enumerate() {
        let name = dep.name.as_str();
        if declared.contains(name) {
            continue;
        }
        let dep_id = DependencyId(index as u32);
        let importers = importers_by_dep
            .get(&dep_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        findings.push(Finding {
            id: finding_id("undeclared", "dependency", name, "", ""),
            category: "undeclared".to_string(),
            group: "defect".to_string(),
            subject_kind: "dependency".to_string(),
            message: format!(
                "{name} is imported but not declared in any manifest (phantom dependency — likely resolving via hoisting/transitivity){}",
                importer_summary(importers)
            ),
        });
    }
    findings
}

fn importer_summary(importers: &[&str]) -> String {
    if importers.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&str> = importers.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() <= 3 {
        format!(": imported by {}", sorted.join(", "))
    } else {
        format!(
            ": imported by {}, {}, {} and {} more",
            sorted[0],
            sorted[1],
            sorted[2],
            sorted.len() - 3
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ProjectPath;
    use crate::graph::{DependencyNode, FileNode};
    use crate::vocab::{Confidence, Edge, FileClass, Provenance};
    use smol_str::SmolStr;

    fn file(path: &str) -> FileNode {
        FileNode {
            path: ProjectPath(SmolStr::new(path)),
            content_hash: [0; 32],
            language: Some(SmolStr::new("mock")),
            class: Some(FileClass::default()),
        }
    }

    fn imports_dep_edge(from: crate::vocab::FileId, to: DependencyId) -> Edge {
        Edge {
            kind: EdgeKind::ImportsDependency { from, to },
            confidence: Confidence::Certain,
            source: Provenance::Adapter(SmolStr::new("mock")),
        }
    }

    #[test]
    fn undeclared_dependency_is_reported() {
        let files = vec![file("a.ts")];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("left-pad"),
        }];
        let edges = vec![imports_dep_edge(crate::vocab::FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let findings = find_undeclared_dependencies(&graph);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, "undeclared");
        assert_eq!(findings[0].group, "defect");
        assert_eq!(findings[0].subject_kind, "dependency");
        assert!(findings[0].message.contains("left-pad"));
        assert!(findings[0].message.contains("a.ts"));
    }

    #[test]
    fn declared_dependency_is_not_reported() {
        use crate::graph::DeclaredDependency;
        use crate::vocab::DependencyScope;

        let files = vec![file("a.ts")];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("lodash"),
        }];
        let edges = vec![imports_dep_edge(crate::vocab::FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges)
            .with_declared_dependencies(vec![DeclaredDependency {
                manifest: ProjectPath(SmolStr::new("package.json")),
                name: SmolStr::new("lodash"),
                version_req: SmolStr::new("^4.0.0"),
                scope: DependencyScope::Prod,
            }]);
        assert!(find_undeclared_dependencies(&graph).is_empty());
    }

    #[test]
    fn finding_id_is_stable_across_runs() {
        let files = vec![file("a.ts")];
        let dependencies = vec![DependencyNode {
            name: SmolStr::new("left-pad"),
        }];
        let edges = vec![imports_dep_edge(crate::vocab::FileId(0), DependencyId(0))];
        let graph = ProjectGraph::for_test(files, vec![], dependencies, edges);
        let a = find_undeclared_dependencies(&graph);
        let b = find_undeclared_dependencies(&graph);
        assert_eq!(a[0].id, b[0].id);
    }
}
